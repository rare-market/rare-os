//! The Share screen's work off the UI thread (share/store.ts prepare, price, share, copyText and
//! the Telegram hook): build the quilt for a capture, price it from live Walrus prices, upload it
//! (Rare Wallet signs and pays, two approvals; or the sponsor stores it), copy the link.

use crate::screens::share::format::{self, CostEstimate, DurationId, Network, QuiltFile, SponsorQuote};
use crate::screens::share::store::{ShareResult, Step};
use crate::screens::wallet::live;
use rare_share::{Capture, Kind};
use rare_sui::saves::store as walrus;

/// A capture ready to upload: the quilt bytes and the files in it.
#[derive(Clone)]
pub(super) struct Prepared {
    pub id: String,
    pub files: Vec<QuiltFile>,
    pub quilt: Vec<u8>,
}

pub(super) struct Priced {
    pub size: u64,
    pub cost: Option<CostEstimate>,
    pub sponsor: Option<SponsorQuote>,
}

pub(super) fn network() -> Network {
    match live::sui().map(|s| s.network()) {
        Some(rare_sui::Network::Testnet) => Network::Testnet,
        _ => Network::Mainnet,
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().timeout(std::time::Duration::from_secs(300)).build().unwrap_or_default()
}

/// RARE_SHARE_SPONSOR: the sponsor's base URL (sponsor.ts VITE_RARE_SHARE_SPONSOR). Unset: no "Rare covers it".
fn sponsor_base() -> Option<String> {
    std::env::var("RARE_SHARE_SPONSOR").ok().map(|s| s.trim_end_matches('/').to_string()).filter(|s| !s.is_empty())
}

/// A clip's poster: the PNG frame the page cached, as JPEG at most 1280 px on the long edge.
fn poster_jpeg(id: &str) -> Option<Vec<u8>> {
    let img = image::open(crate::screens::share::video::poster_cache(id)).ok()?;
    let (w, h) = format::fit_within(img.width(), img.height(), 1280);
    let img = if (w, h) != (img.width(), img.height()) { img.resize_exact(w, h, image::imageops::FilterType::Triangle) } else { img };
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80).encode_image(&img.to_rgb8()).ok()?;
    Some(out)
}

/// Reads the capture, builds share.json and the quilt.
pub(super) fn prepare(c: &Capture, slug: Option<String>) -> Result<Prepared, String> {
    let sui = live::sui().ok_or("Couldn't reach Sui. Check the connection and try again.")?;
    let media = rare_share::read(&c.id)?;
    let poster = if c.kind == Kind::Clip { poster_jpeg(&c.id) } else { None };
    let json = format::build_share_json(c, media.len() as u64, slug.as_deref(), poster.is_some());
    let files = format::share_files(&json, media, poster);
    let sys = live::rt().block_on(walrus::system_info(sui, sui.network())).map_err(|e| e.to_string())?;
    let quilt = walrus::encode_quilt(&files.iter().map(|f| (f.identifier.clone(), f.bytes.clone())).collect::<Vec<_>>(), sys.n_shards).map_err(|e| e.to_string())?;
    Ok(Prepared { id: c.id.clone(), files, quilt })
}

/// The price of `size` for the chosen duration, from live Walrus prices (None when they can't be read).
pub(super) fn price(size: u64, duration: DurationId) -> Priced {
    let n = network();
    let days = format::duration(duration).days;
    let cost = live::sui().and_then(|sui| {
        let r = live::rt().block_on(walrus::estimate(sui, &http(), sui.network(), size, days)).ok()?;
        Some(CostEstimate { wal_frost: r.wal_frost as u128, tip_mist: r.tip_mist as u128, gas_mist: r.gas_mist as u128 })
    });
    let sponsor = sponsor_base().map(|base| {
        let (epochs, _) = format::share_epochs(duration, n);
        match ureq::get(&format!("{base}/quote?bytes={size}&epochs={epochs}")).call() {
            Ok(mut r) => format::parse_sponsor_quote(true, &r.body_mut().read_to_string().unwrap_or_default()),
            Err(_) => SponsorQuote::default(),
        }
    });
    Priced { size, cost, sponsor }
}

/// Stores the quilt. `Err((rejected, message))`: rejected means the person declined in the wallet.
pub(super) fn upload(p: &Prepared, kind: Kind, duration: DurationId, sponsored: bool, on_step: &(dyn Fn(Step) + Sync)) -> Result<ShareResult, (bool, String)> {
    let n = network();
    let (epochs, _) = format::share_epochs(duration, n);
    let fail = |m: String| (m.contains("declined"), m);
    if sponsored {
        let base = sponsor_base().ok_or_else(|| (false, "Rare can't cover this one right now.".to_string()))?;
        let (quilt_id, blob) = sponsor_store(&base, &p.files, epochs).map_err(|m| (false, m))?;
        return Ok(ShareResult { link: format::share_link(&quilt_id, n), quilt_id, blob_object_id: blob });
    }
    let signer = crate::features::signer::current().ok_or((false, "Add a Rare Wallet account to share.".to_string()))?;
    let sui = live::sui().ok_or((false, "Couldn't reach Sui. Check the connection and try again.".to_string()))?;
    let mut attributes: Vec<(String, String)> = vec![("_walrusBlobType".into(), "quilt".into())];
    attributes.extend(format::share_attributes(kind).into_iter().map(|(k, v)| (k.to_string(), v.to_string())));
    let step = move |s: walrus::Step| {
        let m = match s {
            walrus::Step::Encode => Step::Encode,
            walrus::Step::Register => Step::Register,
            walrus::Step::Upload => Step::Upload,
            walrus::Step::Certify | walrus::Step::Done => Step::Certify,
        };
        on_step(m);
    };
    let title = if kind == Kind::Clip { "Share clip" } else { "Share screenshot" };
    // Permanent for its epochs: rare.market finds the player from the blob's certify event, which
    // deletable blobs don't carry.
    let o = walrus::StoreOpts { epochs, attributes, title: title.into(), tag_blob_id: false, deletable: false, titles: Some((title.into(), "Confirm share".into())), on_step: Some(&step) };
    let (blob_id, blob_object) = live::rt().block_on(walrus::store_blob(sui, &http(), signer.as_ref(), p.quilt.clone(), o)).map_err(|e| fail(e.to_string()))?;
    Ok(ShareResult { link: format::share_link(&blob_id, n), quilt_id: blob_id, blob_object_id: Some(blob_object) })
}

/// One file (a logs bundle for a bug report) stored as a one-file quilt for a month: Rare covers it
/// when the sponsor is set up and takes it, else Rare Wallet pays. Returns the aggregator link.
pub(crate) fn upload_file(name: &str, mime: &str, bytes: Vec<u8>) -> Result<String, String> {
    let n = network();
    let (epochs, _) = format::share_epochs(DurationId::Month, n);
    let file = QuiltFile { identifier: name.into(), mime: mime.into(), bytes };
    let mut sponsor_err = None;
    if let Some(base) = sponsor_base() {
        match sponsor_store(&base, std::slice::from_ref(&file), epochs) {
            Ok((quilt_id, _)) => return Ok(format::quilt_file_url(n, &quilt_id, name)),
            Err(e) => sponsor_err = Some(e),
        }
    }
    let Some(signer) = crate::features::signer::current() else {
        return Err(sponsor_err.unwrap_or_else(|| "Add a Rare Wallet account to get a link, or save the logs instead.".into()));
    };
    let sui = live::sui().ok_or("Couldn't reach Sui. Check the connection and try again.")?;
    let sys = live::rt().block_on(walrus::system_info(sui, sui.network())).map_err(|e| e.to_string())?;
    let quilt = walrus::encode_quilt(&[(file.identifier.clone(), file.bytes)], sys.n_shards).map_err(|e| e.to_string())?;
    let attributes: Vec<(String, String)> = vec![("_walrusBlobType".into(), "quilt".into())];
    let o = walrus::StoreOpts { epochs, attributes, title: "Upload logs".into(), tag_blob_id: false, deletable: false, titles: Some(("Upload logs".into(), "Confirm upload".into())), on_step: None };
    let (blob_id, _) = live::rt().block_on(walrus::store_blob(sui, &http(), signer.as_ref(), quilt, o)).map_err(|e| e.to_string())?;
    Ok(format::quilt_file_url(n, &blob_id, name))
}

/// Copies text to the clipboard (for screens outside the app module).
pub(crate) fn copy_link(text: &str) -> bool {
    copy_text(text)
}

/// POST <base>/store?epochs=<n>, one multipart part per quilt file (sponsor.ts httpSponsor.store).
fn sponsor_store(base: &str, files: &[QuiltFile], epochs: u32) -> Result<(String, Option<String>), String> {
    let boundary = format!("----rare{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    let mut body = Vec::new();
    for f in files {
        body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{0}\"; filename=\"{0}\"\r\nContent-Type: {1}\r\n\r\n", f.identifier, f.mime).into_bytes());
        body.extend(&f.bytes);
        body.extend(b"\r\n");
    }
    body.extend(format!("--{boundary}--\r\n").into_bytes());
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(300))).http_status_as_error(false).build().into();
    let mut r = agent.post(&format!("{base}/store?epochs={epochs}")).header("Content-Type", &format!("multipart/form-data; boundary={boundary}")).send(&body[..]).map_err(|_| "Couldn't reach Walrus. Check the connection and try again.".to_string())?;
    let status = r.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("Rare couldn't store this share ({status})"));
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct R {
        quilt_id: String,
        blob_object_id: Option<String>,
    }
    let j: R = serde_json::from_str(&r.body_mut().read_to_string().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok((j.quilt_id, j.blob_object_id))
}

/// Copies text to the system clipboard (Wayland, then X). False when neither is there.
pub(super) fn copy_text(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    for (bin, args) in [("wl-copy", &[][..]), ("xclip", &["-selection", "clipboard"][..]), ("xsel", &["--clipboard", "--input"][..])] {
        let Ok(mut child) = Command::new(bin).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { continue };
        if let Some(mut si) = child.stdin.take() {
            if si.write_all(text.as_bytes()).is_err() {
                continue;
            }
        }
        // wl-copy and xclip stay up to serve the selection; do not wait on them.
        match child.try_wait() {
            Ok(Some(s)) if !s.success() => continue,
            _ => return true,
        }
    }
    false
}

/// post.ts's Telegram hook: opens Telegram's share page for the link.
pub(super) fn post_telegram(link: &str) -> Result<(), String> {
    let q: String = link.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    for url in [format!("tg://msg_url?url={q}"), format!("https://t.me/share/url?url={q}")] {
        if std::process::Command::new("xdg-open").arg(&url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success()) {
            return Ok(());
        }
    }
    Err("Telegram would not open".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quilt_holds_the_files_in_order() {
        let files = vec![("share.json".to_string(), b"{}".to_vec()), ("shot.webp".to_string(), vec![7u8; 5000])];
        let q = walrus::encode_quilt(&files, 1000).unwrap();
        assert!(q.len() >= 5000);
        assert_eq!(q, walrus::encode_quilt(&files, 1000).unwrap(), "same files, same quilt");
    }

    #[test]
    fn telegram_link_is_encoded() {
        let q: String = "https://rare.market/s/abc?n=testnet".bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
        assert_eq!(q, "https%3A%2F%2Frare.market%2Fs%2Fabc%3Fn%3Dtestnet");
    }
}
