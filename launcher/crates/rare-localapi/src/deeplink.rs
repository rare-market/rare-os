//! `rare://` deep links and their https://rare.market twins.
//!
//! | Link                                        | Web twin                                  |
//! |---------------------------------------------|-------------------------------------------|
//! | rare://item/<objectId>[?sheet=sell|buy]     | https://rare.market/item/<objectId>       |
//! | rare://collection/<slug or type>            | https://rare.market/collection/<slug>     |
//! | rare://game/<slug>                          | https://rare.market/games/<slug>          |
//! | rare://store/<slug>                         | https://rare.market/store/<slug>          |
//! | rare://trophies/<slug>                      | https://rare.market/games/<slug>#trophies |
//! | rare://u/<address or username>              | https://rare.market/u/<address>           |
//! | rare://overlay[?tab=items|market|trophies|notices] | none                               |
//! | rare://claim?v=<base64 voucher>             | https://rare.market/claim?v=              |
//!
//! `rare://profile/<x>` is accepted as an alias of `rare://u/<x>`. Everything a link carries is
//! checked here, so the UI only ever sees well-formed ids. Links are fire-and-forget: nothing is
//! signed from a link, every sheet it opens still needs the player.

use serde::Serialize;

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Sheet {
    Sell,
    Buy,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OverlayTab {
    Items,
    Market,
    Trophies,
    Notices,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Route {
    Item { id: String, sheet: Option<Sheet> },
    /// A collection slug, or a Move type (`0x..::module::Name`).
    Collection { id: String },
    Game { slug: String },
    Store { slug: String },
    Trophies { slug: String },
    /// An address (0x...) or a Rare username.
    Profile { who: String },
    Overlay { tab: Option<OverlayTab> },
    Claim { voucher: String },
}

/// Longest voucher a link may carry (base64 text).
pub const MAX_VOUCHER: usize = 8192;

/// Parses a `rare://` link or an https://rare.market link. Err says what is wrong with it.
pub fn parse(link: &str) -> Result<Route, String> {
    let link = link.trim();
    if link.len() > MAX_VOUCHER + 256 {
        return Err("link too long".into());
    }
    let u: url::Url = link.parse().map_err(|_| "not a link".to_string())?;
    let query = |k: &str| u.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned());
    // (head, rest) where head is the first part of the route ("item") and rest the path segments after it.
    let (head, rest): (String, Vec<String>) = match u.scheme() {
        "rare" => {
            let head = u.host_str().unwrap_or("").to_ascii_lowercase();
            (head, segments(&u))
        }
        "https" => {
            let host = u.host_str().unwrap_or("").to_ascii_lowercase();
            if host != "rare.market" && host != "www.rare.market" {
                return Err("not a rare.market link".into());
            }
            if u.port().is_some() {
                return Err("not a rare.market link".into());
            }
            let mut segs = segments(&u);
            if segs.is_empty() {
                return Err("no page in the link".into());
            }
            let head = segs.remove(0).to_ascii_lowercase();
            // The web's names for the same pages.
            let head = match head.as_str() {
                "games" if u.fragment() == Some("trophies") => "trophies".to_string(),
                "games" => "game".to_string(),
                "collections" => "collection".to_string(),
                "overlay" => return Err("the overlay has no web page".into()),
                _ => head,
            };
            (head, segs)
        }
        _ => return Err("not a rare link".into()),
    };
    let one = |what: &str| -> Result<String, String> {
        match rest.as_slice() {
            [x] if !x.is_empty() => Ok(x.clone()),
            [] => Err(format!("no {what} in the link")),
            _ => Err("too many parts in the link".into()),
        }
    };
    match head.as_str() {
        "item" => {
            let id = object_id(&one("item id")?).ok_or("not an object id")?;
            let sheet = match query("sheet").as_deref() {
                None | Some("") => None,
                Some("sell") => Some(Sheet::Sell),
                Some("buy") => Some(Sheet::Buy),
                Some(_) => return Err("sheet must be sell or buy".into()),
            };
            Ok(Route::Item { id, sheet })
        }
        "collection" => {
            let id = one("collection")?;
            if slug(&id) || move_type(&id) {
                Ok(Route::Collection { id })
            } else {
                Err("not a collection slug or type".into())
            }
        }
        "game" | "store" | "trophies" => {
            let s = one("game")?;
            if !slug(&s) {
                return Err("not a game slug".into());
            }
            Ok(match head.as_str() {
                "game" => Route::Game { slug: s },
                "store" => Route::Store { slug: s },
                _ => Route::Trophies { slug: s },
            })
        }
        "u" | "profile" => {
            let who = one("address or username")?;
            if let Some(a) = object_id(&who) {
                return Ok(Route::Profile { who: a });
            }
            if username(&who) {
                return Ok(Route::Profile { who: who.to_ascii_lowercase() });
            }
            Err("not an address or username".into())
        }
        "overlay" => {
            if !rest.is_empty() {
                return Err("too many parts in the link".into());
            }
            let tab = match query("tab").as_deref() {
                None | Some("") => None,
                Some("items") => Some(OverlayTab::Items),
                Some("market") => Some(OverlayTab::Market),
                Some("trophies") => Some(OverlayTab::Trophies),
                Some("notices") => Some(OverlayTab::Notices),
                Some(_) => return Err("tab must be items, market, trophies or notices".into()),
            };
            Ok(Route::Overlay { tab })
        }
        "claim" => {
            if !rest.is_empty() {
                return Err("too many parts in the link".into());
            }
            let v = query("v").unwrap_or_default();
            if !voucher(&v) {
                return Err("no readable voucher in the link".into());
            }
            Ok(Route::Claim { voucher: v })
        }
        _ => Err("unknown Rare page".into()),
    }
}

/// True for links this module handles (so a web game's navigation to them is routed, not followed).
pub fn is_rare_link(u: &url::Url) -> bool {
    match u.scheme() {
        "rare" => true,
        "https" => matches!(u.host_str(), Some("rare.market") | Some("www.rare.market")) && parse(u.as_str()).is_ok(),
        _ => false,
    }
}

/// Path segments, percent-decoded, without empty ones (a trailing slash).
fn segments(u: &url::Url) -> Vec<String> {
    u.path_segments().map(|s| s.filter(|x| !x.is_empty()).map(pct_decode).collect()).unwrap_or_default()
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `0x` and 1 to 64 hex digits, returned in the canonical form (lowercase, 64 digits).
pub fn object_id(s: &str) -> Option<String> {
    let h = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    if h.is_empty() || h.len() > 64 || !h.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("0x{:0>64}", h.to_ascii_lowercase()))
}

pub fn slug(s: &str) -> bool {
    !s.is_empty() && s.len() <= 96 && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.')) && !s.starts_with('.')
}

fn username(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.')) && !s.starts_with('.')
}

/// A Move type: `0x<hex>::module::Name`, optionally with type arguments.
pub fn move_type(s: &str) -> bool {
    let Some((addr, rest)) = s.split_once("::") else { return false };
    if object_id(addr).is_none() || s.len() > 512 || !rest.contains("::") {
        return false;
    }
    s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b':' | b'<' | b'>' | b',' | b' '))
}

/// base64 or base64url text, 1 to MAX_VOUCHER characters.
pub fn voucher(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_VOUCHER && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'-' | b'_' | b'='))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0x00000000000000000000000000000000000000000000000000000000000abc12";

    fn ok(l: &str) -> Route {
        parse(l).unwrap_or_else(|e| panic!("{l}: {e}"))
    }

    #[test]
    fn item_links_and_their_web_twin() {
        assert_eq!(ok("rare://item/0xabc12"), Route::Item { id: ID.into(), sheet: None });
        assert_eq!(ok("rare://item/0xABC12?sheet=sell"), Route::Item { id: ID.into(), sheet: Some(Sheet::Sell) });
        assert_eq!(ok("rare://item/0xabc12/?sheet=buy"), Route::Item { id: ID.into(), sheet: Some(Sheet::Buy) });
        assert_eq!(ok("https://rare.market/item/0xabc12"), Route::Item { id: ID.into(), sheet: None });
        assert_eq!(ok("https://www.rare.market/item/0xabc12?sheet=sell"), Route::Item { id: ID.into(), sheet: Some(Sheet::Sell) });
        assert!(parse("rare://item/abc12").is_err());
        assert!(parse("rare://item/0x").is_err());
        assert!(parse(&format!("rare://item/0x{}", "a".repeat(65))).is_err());
        assert!(parse("rare://item/0xabc?sheet=transfer").is_err());
        assert!(parse("rare://item/0xabc/extra").is_err());
        assert!(parse("rare://item").is_err());
    }

    #[test]
    fn collections_by_slug_or_type() {
        assert_eq!(ok("rare://collection/pawtato-land"), Route::Collection { id: "pawtato-land".into() });
        assert_eq!(ok("https://rare.market/collection/popkins"), Route::Collection { id: "popkins".into() });
        assert_eq!(ok("https://rare.market/collections/popkins"), Route::Collection { id: "popkins".into() });
        assert_eq!(ok("rare://collection/0x2%3A%3Apopkins%3A%3APopkin"), Route::Collection { id: "0x2::popkins::Popkin".into() });
        assert!(parse("rare://collection/<script>").is_err());
        assert!(parse("rare://collection/0x2::nomodule").is_err());
    }

    #[test]
    fn games_store_trophies() {
        assert_eq!(ok("rare://game/act"), Route::Game { slug: "act".into() });
        assert_eq!(ok("https://rare.market/games/act"), Route::Game { slug: "act".into() });
        assert_eq!(ok("rare://store/my-game"), Route::Store { slug: "my-game".into() });
        assert_eq!(ok("https://rare.market/store/my-game"), Route::Store { slug: "my-game".into() });
        assert_eq!(ok("rare://trophies/act"), Route::Trophies { slug: "act".into() });
        assert_eq!(ok("https://rare.market/games/act#trophies"), Route::Trophies { slug: "act".into() });
        assert!(parse("rare://game/a/b").is_err());
        assert!(parse("rare://game/a%2Fb").is_err());
    }

    #[test]
    fn profiles() {
        assert_eq!(ok("rare://u/0xabc12"), Route::Profile { who: ID.into() });
        assert_eq!(ok("rare://u/Ghost_7"), Route::Profile { who: "ghost_7".into() });
        assert_eq!(ok("rare://profile/ghost.sui"), Route::Profile { who: "ghost.sui".into() });
        assert_eq!(ok("https://rare.market/u/0xabc12"), Route::Profile { who: ID.into() });
        assert!(parse("rare://u/a b").is_err());
    }

    #[test]
    fn overlay_tabs() {
        assert_eq!(ok("rare://overlay"), Route::Overlay { tab: None });
        assert_eq!(ok("rare://overlay?tab=market"), Route::Overlay { tab: Some(OverlayTab::Market) });
        assert_eq!(ok("rare://overlay?tab=notices"), Route::Overlay { tab: Some(OverlayTab::Notices) });
        assert!(parse("rare://overlay?tab=settings").is_err());
        assert!(parse("https://rare.market/overlay").is_err());
    }

    #[test]
    fn claim_vouchers() {
        assert_eq!(ok("rare://claim?v=eyJ2IjoxfQ"), Route::Claim { voucher: "eyJ2IjoxfQ".into() });
        assert_eq!(ok("https://rare.market/claim?v=ab%2Bc%2F%3D"), Route::Claim { voucher: "ab+c/=".into() });
        assert!(parse("rare://claim").is_err());
        assert!(parse("rare://claim?v=not%20base64").is_err());
        assert!(parse(&format!("rare://claim?v={}", "a".repeat(MAX_VOUCHER + 1))).is_err());
    }

    #[test]
    fn foreign_links_are_refused() {
        assert!(parse("https://evil.example/item/0xabc").is_err());
        assert!(parse("https://rare.market.evil.example/item/0xabc").is_err());
        assert!(parse("https://rare.market:8443/item/0xabc").is_err());
        assert!(parse("http://rare.market/item/0xabc").is_err());
        assert!(parse("rare://settings").is_err());
        assert!(parse("file:///etc/passwd").is_err());
        assert!(parse("https://rare.market/").is_err());
        assert!(parse("not a link").is_err());
    }

    #[test]
    fn rare_link_detection_for_web_games() {
        assert!(is_rare_link(&"rare://item/0x1".parse().unwrap()));
        assert!(is_rare_link(&"https://rare.market/item/0x1".parse().unwrap()));
        // Other rare.market pages (the market itself, sign-in) keep loading normally.
        assert!(!is_rare_link(&"https://rare.market/developers".parse().unwrap()));
        assert!(!is_rare_link(&"https://example.com/item/0x1".parse().unwrap()));
    }
}
