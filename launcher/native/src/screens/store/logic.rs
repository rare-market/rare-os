//! The Store's pure decisions and words (store/logic.ts, parts.tsx linkWhere, tx.ts suiQuote,
//! actions.ts plain). The installer makes the update and grace decisions again in rare-store.

use super::model::{ChainBuild, ChainRelease, InstalledAt, Licence, StoreGame};
use crate::kit::format::grouped;

pub const DAY_MS: f64 = 86_400_000.0;
/// A positive launch check keeps a game launching offline this long.
pub const LAUNCH_GRACE_MS: f64 = rare_store::update::LAUNCH_GRACE_MS as f64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateDecision {
    Install,
    Update,
    Current,
    Unavailable,
}

/// Install when nothing is installed, Update only when the chain has a strictly higher version for the edition.
pub fn update_decision(installed: Option<InstalledAt>, release: Option<(u64, u64)>) -> UpdateDecision {
    let Some((version, edition)) = release else {
        return if installed.is_some() { UpdateDecision::Current } else { UpdateDecision::Unavailable };
    };
    let Some(i) = installed else { return UpdateDecision::Install };
    if i.edition != edition {
        return UpdateDecision::Update;
    }
    if version > i.version {
        UpdateDecision::Update
    } else {
        UpdateDecision::Current
    }
}

/// The build this device should download: the first platform it prefers that the release has.
pub fn pick_build<'a>(release: &'a ChainRelease, platforms: &[&str]) -> Option<&'a ChainBuild> {
    platforms.iter().find_map(|p| release.builds.iter().find(|b| b.platform == *p))
}

pub fn refundable(l: &Licence, now: f64) -> bool {
    l.kind == "licence" && l.refundable_until_ms.is_some_and(|t| now < t)
}

/// Make tradable is possible now (resale on, hold over, still a plain licence).
pub fn tradable_now(l: &Licence, now: f64) -> bool {
    l.kind == "licence" && l.tradable_from_ms.is_some_and(|t| now >= t)
}

/// The offline grace: a check passed within the last 7 days.
pub fn within_grace(checked_ms: f64, now: f64) -> bool {
    checked_ms > 0.0 && now - checked_ms <= LAUNCH_GRACE_MS
}

/// The price a purchase pays now, in micro-USD (game::current_price_usd on the API's numbers).
pub fn price_now(g: &StoreGame, now: f64) -> Option<u64> {
    let p = g.price.as_ref()?;
    let sale = p.sale_usd.unwrap_or(0);
    if sale > 0 && p.sale_ends_ms.is_some_and(|e| now < e) {
        return Some(sale);
    }
    Some(p.usd)
}

pub fn on_sale(g: &StoreGame, now: f64) -> bool {
    match (price_now(g, now), g.price.as_ref()) {
        (Some(n), Some(p)) => n < p.usd,
        _ => false,
    }
}

/// micro-USD as dollars ("$14.99", "Free").
pub fn usd_text(micro: u64) -> String {
    if micro == 0 {
        return "Free".into();
    }
    let cents = (micro + 5_000) / 10_000;
    format!("${}.{:02}", grouped((cents / 100) as f64, 0, 0), cents % 100)
}

/// "2 h", "1 day", "3 days".
pub fn time_left(ms: f64) -> String {
    let h = (ms / 3_600_000.0).round().max(1.0);
    if h < 48.0 {
        return format!("{h} h");
    }
    let d = (h / 24.0).round();
    format!("{d} {}", if d == 1.0 { "day" } else { "days" })
}

pub fn steam_app_id(url: Option<&str>) -> Option<u64> {
    let u = url?;
    let i = u.find("store.steampowered.com/app/")? + "store.steampowered.com/app/".len();
    let digits: String = u[i..].chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn platform_name(p: &str) -> &str {
    match p {
        "linux-x86_64" => "Linux",
        "windows-x86_64" => "Windows",
        "web" => "Web",
        "pc" => "PC",
        "mobile" => "Mobile",
        "suiplay" => "SuiPlay",
        other => other,
    }
}

pub fn platform_text(ps: &[String]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for p in ps {
        let n = platform_name(p);
        if !seen.contains(&n) {
            seen.push(n);
        }
    }
    seen.join(", ")
}

/// Bytes as "734 MB" / "1.2 GB".
pub fn size_text(b: u64) -> String {
    let n = b as f64;
    if n >= 1e9 {
        format!("{:.1} GB", n / 1e9)
    } else {
        format!("{} MB", (n / 1e6).round().max(1.0))
    }
}

/// Where a link listing lives, for its tile and page.
pub fn link_where(g: &StoreGame) -> &'static str {
    if g.links.steam.is_some() {
        "On Steam"
    } else if g.links.epic.is_some() {
        "On Epic Games"
    } else if g.links.play.is_some() {
        "Plays in the browser"
    } else {
        "Website"
    }
}

pub fn status_word(s: &str) -> &str {
    match s {
        "live" => "Live",
        "beta" => "Beta",
        other => other,
    }
}

/// "Northbound Games · Racing"
pub fn studio_line(g: &StoreGame) -> String {
    [Some(g.studio.name.as_str()), g.genre.as_deref()].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// The tile's status line: "Live · Linux", "Beta · Website".
pub fn card_status(g: &StoreGame) -> String {
    let st = match g.status.as_deref() {
        Some("live") => "Live",
        Some("beta") => "Beta",
        _ => "",
    };
    let where_ = if g.is_link() { link_where(g).to_string() } else { platform_text(&g.platforms) };
    [st.to_string(), where_].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// USD (micro) to MIST at a Pyth price (e8), rounded up (tx.ts usdToMist).
pub fn usd_to_mist(usd: u64, price_e8: u64) -> Option<u128> {
    if price_e8 == 0 {
        return None;
    }
    let num = usd as u128 * 100_000_000_000;
    Some(num.div_ceil(price_e8 as u128))
}

/// What a SUI purchase costs at the quoted price, and the most it may take (quote plus 1%).
pub fn sui_quote(price_usd: u64, price_e8: u64) -> Option<(u128, u128)> {
    let mist = usd_to_mist(price_usd, price_e8)?;
    Some((mist, mist + mist.div_ceil(100)))
}

/// Build and network errors in plain words (actions.ts plain).
pub fn plain(m: &str) -> String {
    let low = m.to_ascii_lowercase();
    let net = ["failed to fetch", "networkerror", "load failed", "error sending request", "dns"].iter().any(|k| low.contains(k));
    let rm = low.strip_prefix("rare.market ").is_some_and(|r| r.chars().next().is_some_and(|c| c.is_ascii_digit()));
    if net || rm {
        return "Couldn't reach rare.market. Check the connection and try again.".into();
    }
    if m.starts_with("no_licence") {
        return "This wallet doesn't hold a licence for this game.".into();
    }
    if m.starts_with("no_build") {
        return "There's no build for this device yet.".into();
    }
    if m.starts_with("uploads_closed") {
        return "Downloads open soon.".into();
    }
    // A leading "code: " goes.
    let out = match m.split_once(": ") {
        Some((code, rest)) if !code.is_empty() && code.chars().all(|c| c.is_ascii_lowercase() || c == '_') => rest,
        _ => m,
    };
    if out.is_empty() {
        "That didn't go through. Try again.".into()
    } else {
        out.to_string()
    }
}

/// "Nov 3" for a UTC time in ms (toLocaleDateString month short, day numeric; the launcher runs in UTC here).
pub fn month_day(ms: f64) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let days = (ms / DAY_MS).floor() as i64;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{} {}", M[(m - 1) as usize], d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::store::model::StorePrice;

    fn release(version: u64, edition: u64) -> ChainRelease {
        ChainRelease {
            version,
            label: format!("1.{version}"),
            edition,
            published_ms: 0.0,
            builds: vec![
                ChainBuild { platform: "windows-x86_64".into(), sha256: "aa".repeat(32), size: 10, path: "w".into() },
                ChainBuild { platform: "linux-x86_64".into(), sha256: "bb".repeat(32), size: 20, path: "l".into() },
            ],
        }
    }
    fn rel(r: &ChainRelease) -> Option<(u64, u64)> {
        Some((r.version, r.edition))
    }
    fn at(edition: u64, version: u64) -> Option<InstalledAt> {
        Some(InstalledAt { edition, version })
    }

    #[test]
    fn installs_when_nothing_is_installed() {
        assert_eq!(update_decision(None, rel(&release(2, 1))), UpdateDecision::Install);
    }

    #[test]
    fn updates_only_for_a_strictly_higher_version() {
        assert_eq!(update_decision(at(1, 2), rel(&release(3, 1))), UpdateDecision::Update);
        assert_eq!(update_decision(at(1, 3), rel(&release(3, 1))), UpdateDecision::Current);
        assert_eq!(update_decision(at(1, 4), rel(&release(3, 1))), UpdateDecision::Current);
        assert_eq!(update_decision(at(2, 4), rel(&release(3, 1))), UpdateDecision::Update);
    }

    #[test]
    fn has_nothing_to_offer_without_a_release() {
        assert_eq!(update_decision(None, None), UpdateDecision::Unavailable);
        assert_eq!(update_decision(at(1, 1), None), UpdateDecision::Current);
    }

    #[test]
    fn takes_the_devices_first_platform_the_release_has() {
        let r = release(1, 1);
        assert_eq!(pick_build(&r, &["linux-x86_64", "windows-x86_64"]).map(|b| b.path.as_str()), Some("l"));
        assert_eq!(pick_build(&r, &["windows-x86_64"]).map(|b| b.path.as_str()), Some("w"));
        assert!(pick_build(&r, &["macos-arm64"]).is_none());
        assert!(pick_build(&r, &[]).is_none());
    }

    fn lic(kind: &str, refund: Option<f64>, trade: Option<f64>) -> Licence {
        Licence { kind: kind.into(), refundable_until_ms: refund, tradable_from_ms: trade, ..Default::default() }
    }

    #[test]
    fn licence_windows() {
        let now = 30.0 * DAY_MS;
        assert!(refundable(&lic("licence", Some(now + 1.0), None), now));
        assert!(!refundable(&lic("licence", Some(now), None), now));
        assert!(!refundable(&lic("licence", None, None), now));
        assert!(!refundable(&lic("resale", Some(now + 1.0), None), now));
        assert!(tradable_now(&lic("licence", None, Some(now)), now));
        assert!(!tradable_now(&lic("licence", None, Some(now + 1.0)), now));
        assert!(!tradable_now(&lic("licence", None, None), now));
        assert!(within_grace(now - LAUNCH_GRACE_MS, now));
        assert!(!within_grace(now - LAUNCH_GRACE_MS - 1.0, now));
        assert!(!within_grace(0.0, now));
    }

    fn priced(usd: u64, sale: Option<u64>, ends: Option<f64>) -> StoreGame {
        StoreGame { price: Some(StorePrice { usd, sale_usd: sale, sale_ends_ms: ends, accept_sui: true }), ..Default::default() }
    }

    #[test]
    fn uses_the_sale_price_while_the_sale_runs() {
        assert_eq!(price_now(&priced(9_990_000, Some(6_990_000), Some(2000.0)), 1000.0), Some(6_990_000));
        assert_eq!(price_now(&priced(9_990_000, Some(6_990_000), Some(2000.0)), 2000.0), Some(9_990_000));
        assert!(on_sale(&priced(9_990_000, Some(6_990_000), Some(2000.0)), 1000.0));
        assert_eq!(price_now(&StoreGame::default(), 0.0), None);
    }

    #[test]
    fn formats_micro_usd() {
        assert_eq!(usd_text(14_990_000), "$14.99");
        assert_eq!(usd_text(0), "Free");
        assert_eq!(usd_text(1_234_500_000), "$1,234.50");
    }

    #[test]
    fn finds_steam_app_ids() {
        assert_eq!(steam_app_id(Some("https://store.steampowered.com/app/2379780/Balatro/")), Some(2379780));
        assert_eq!(steam_app_id(Some("https://example.com")), None);
    }

    #[test]
    fn sui_quote_adds_one_percent() {
        assert_eq!(sui_quote(10_000_000, 100_000_000), Some((10_000_000_000, 10_100_000_000)));
        assert_eq!(sui_quote(1, 0), None);
    }

    #[test]
    fn words() {
        assert_eq!(time_left(45.0 * 3_600_000.0), "45 h");
        assert_eq!(time_left(60.0), "1 h");
        assert_eq!(time_left(50.0 * 3_600_000.0), "2 days");
        assert_eq!(time_left(30.0 * 3_600_000.0 * 1.6), "2 days");
        assert_eq!(size_text(734_003_200), "734 MB");
        assert_eq!(size_text(1_234_000_000), "1.2 GB");
        assert_eq!(size_text(10), "1 MB");
        assert_eq!(platform_text(&["linux-x86_64".into(), "web".into(), "linux-x86_64".into()]), "Linux, Web");
        assert_eq!(month_day(1_793_731_095_967.0), "Nov 3");
        assert_eq!(plain("rare.market 503"), "Couldn't reach rare.market. Check the connection and try again.");
        assert_eq!(plain("no_build: linux"), "There's no build for this device yet.");
        assert_eq!(plain("bad_thing: The network said no"), "The network said no");
    }
}
