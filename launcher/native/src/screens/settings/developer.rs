//! The Developer section (screens/settings/DeveloperSection.tsx): the Sui network, the testnet
//! endpoints, the developer game.

use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::screens::overlay::common::*;
use rare_render::Id;

pub const DEFAULT_RPC_MAINNET: &str = "https://fullnode.mainnet.sui.io:443";
pub const DEFAULT_RPC_TESTNET: &str = "https://fullnode.testnet.sui.io:443";

/// https anywhere; plain http only on this machine (a local node under test).
pub fn rpc_ok(u: &str) -> bool {
    let (scheme, rest) = match u.split_once("://") {
        Some(x) => x,
        None => return false,
    };
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let auth = &rest[..host_end];
    let auth = auth.rsplit_once('@').map(|(_, h)| h).unwrap_or(auth);
    if auth.is_empty() {
        return false;
    }
    let host = if auth.starts_with('[') { auth.split_once(']').map(|(h, _)| format!("{h}]")).unwrap_or_default() } else { auth.split(':').next().unwrap_or("").to_string() };
    match scheme {
        "https" => !host.is_empty(),
        "http" => ["localhost", "127.0.0.1", "[::1]"].contains(&host.as_str()),
        _ => false,
    }
}

/// A blank answer clears the override (Some(None)); anything else must be https (http only on this
/// machine), else None (refused).
pub fn clean_endpoint(v: &str) -> Option<Option<String>> {
    let t = v.trim().trim_end_matches('/');
    if t.is_empty() {
        return Some(None);
    }
    if rpc_ok(t) {
        Some(Some(t.to_string()))
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct DevGame {
    pub url: String,
    pub slug: String,
    pub collections: usize,
}

pub struct DeveloperSection {
    rows: RowSet,
    keys: Vec<rare_system::ssh::SshKey>,
}

impl DeveloperSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, now: f64) -> DeveloperSection {
        let snap = super::live::get();
        let dev = snap.as_ref().and_then(|s| s.dev.clone());
        let api = snap.and_then(|s| s.local_api);
        DeveloperSection::build_with(cx, nav, parent, cfg, dev.as_ref(), api.as_deref(), now)
    }

    pub fn build_with(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, dev: Option<&DevGame>, api: Option<&str>, _now: f64) -> DeveloperSection {
        let info = super::live::get().map(|s| s.ssh).unwrap_or_else(super::remote::fixture_default);
        Self::build_full(cx, nav, parent, cfg, dev, api, &info)
    }

    pub fn build_full(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, dev: Option<&DevGame>, api: Option<&str>, info: &super::remote::SshInfo) -> DeveloperSection {
        let mut rows = RowSet::default();
        let testnet = cfg.network.as_deref() == Some("testnet");
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let keys = super::remote::build(cx, nav, parent, &mut rows, info, secs);
        super::remote::build_tools(cx, nav, parent, &mut rows, info);
        let p1 = panel(cx, parent);
        let body = if testnet { "Testnet: Sui reads, Rare Wallet and the Rare API use testnet. Nothing here costs real SUI." } else { "Mainnet. Switch to testnet to try a game with test SUI." };
        rows.choice(cx, nav, p1, "dev-network", "Network", Some(body), &["Mainnet", "Testnet"], Some(testnet as usize), true);
        let default_rpc = format!("{DEFAULT_RPC_TESTNET} (default)");
        let eps: [(&str, &str, String, Option<String>, u32); 3] = [
            ("dev-testnet-rpc", "Testnet RPC", default_rpc, cfg.testnet_rpc.clone(), 1),
            ("dev-testnet-api", "Testnet market API", "rare.market with ?network=testnet (default)".into(), cfg.testnet_api.clone(), 2),
            ("dev-testnet-sponsor", "Testnet gas sponsor", "Same as the testnet market API (default)".into(), cfg.testnet_sponsor.clone(), 3),
        ];
        for (sid, title, def, val, tag) in eps {
            let shown = val.clone().unwrap_or(def);
            let row_ = setting_row(cx, p1, title, Some(&shown), false);
            let b = button(cx, row_.right, if val.is_some() { "Change" } else { "Set" }, Variant::Secondary, Sz::Md, false).sid(sid).tag(tag);
            nav.add(b.group(11));
        }
        let p2 = panel(cx, parent);
        let body = match dev {
            Some(d) => format!("{}, {}, {} collection{}", d.url, d.slug, d.collections, if d.collections == 1 { "" } else { "s" }),
            None => "None. Start the launcher with --dev-game <url> to run a local web game with the Local API.".into(),
        };
        let api_body = match api {
            Some(u) => format!("Listening at {u}. Games started from Rare get RARE_API and RARE_TOKEN."),
            None => "Not running.".into(),
        };
        setting_row(cx, p2, "Local API", Some(&api_body), true);
        setting_row(cx, p2, "Developer game", Some(&body), false);
        DeveloperSection { rows, keys }
    }
}

impl SectionView for DeveloperSection {
    fn press(&mut self, cx: &mut Cx, nav: &mut Nav, cfg: &mut Config, now: f64) -> SectionOut {
        let Some(f) = nav.current().cloned() else { return SectionOut::None };
        if let Some(out) = super::remote::press(&mut self.rows, &self.keys, cx, nav, &f, now) {
            return out;
        }
        if f.sid == "dev-network" {
            cfg.network = Some(if f.tag == 1 { "testnet".into() } else { "mainnet".into() });
            return SectionOut::Changed;
        }
        match f.tag {
            1 => SectionOut::Act("developer:endpoint:testnetRpc".into()),
            2 => SectionOut::Act("developer:endpoint:testnetApi".into()),
            3 => SectionOut::Act("developer:endpoint:testnetSponsor".into()),
            _ => SectionOut::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_clear_on_blank_trim_the_slash_refuse_unsafe() {
        assert_eq!(clean_endpoint("  "), Some(None));
        assert_eq!(clean_endpoint("https://t.example.com/"), Some(Some("https://t.example.com".into())));
        assert_eq!(clean_endpoint("http://localhost:3001"), Some(Some("http://localhost:3001".into())));
        assert_eq!(clean_endpoint("http://example.com"), None);
    }

    #[test]
    fn rpc_ok_rules() {
        assert!(rpc_ok("https://fullnode.testnet.sui.io:443"));
        assert!(rpc_ok("http://127.0.0.1:9000/"));
        assert!(rpc_ok("http://[::1]:9000"));
        assert!(!rpc_ok("not a url"));
        assert!(!rpc_ok("ftp://x.y"));
        assert!(!rpc_ok("http://example.com"));
    }
}
