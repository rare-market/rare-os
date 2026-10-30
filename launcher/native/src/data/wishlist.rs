//! The wishlist: games added in Rare plus the Steam account's own wishlist (read from Steam's
//! public IWishlistService for the account signed in on this device; it answers when the
//! profile's game details are public), with the lowest current price across stores from
//! rare.market (CheapShark). A price that drops 5 % or more below the last one seen raises a
//! notice. Kept in ~/.local/state/rare-native/wishlist.json; refreshed every 6 hours in front.

use super::knowledge::{self, Price};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Item {
    /// `steam:<appid>` or `t:<title>`.
    pub id: String,
    pub title: String,
    pub steam: Option<u32>,
    /// When it was added (unix secs).
    pub added: i64,
    /// On the Steam account's wishlist.
    pub steam_list: bool,
    /// Added in Rare.
    pub rare: bool,
    /// A Steam wishlist game removed in Rare (kept so the next sync does not bring it back).
    pub hidden: bool,
    /// The price the last notice (or the first look) was about.
    pub seen: Option<f64>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Wishlist {
    pub items: Vec<Item>,
    pub prices: HashMap<String, Price>,
    /// When prices and the Steam list were last read (unix secs).
    pub prices_at: i64,
    pub steam_at: i64,
    /// The Steam account answered with a list (false: private or no account).
    pub steam_ok: bool,
}

impl Wishlist {
    /// What to show: not hidden, on sale first (biggest cut), then newest.
    pub fn shown(&self) -> Vec<&Item> {
        let mut v: Vec<&Item> = self.items.iter().filter(|i| !i.hidden).collect();
        v.sort_by(|a, b| {
            let ca = self.prices.get(&a.id).map_or(0, |p| p.cut);
            let cb = self.prices.get(&b.id).map_or(0, |p| p.cut);
            cb.cmp(&ca).then(b.added.cmp(&a.added))
        });
        v
    }

    pub fn on_sale(&self) -> usize {
        self.shown().iter().filter(|i| self.prices.get(&i.id).is_some_and(|p| p.cut > 0)).count()
    }

    pub fn has(&self, id: &str) -> bool {
        self.items.iter().any(|i| i.id == id && !i.hidden)
    }
}

static WL: Mutex<Option<Wishlist>> = Mutex::new(None);
static VER: AtomicU64 = AtomicU64::new(0);
static AT: Mutex<f64> = Mutex::new(0.0);
static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn path() -> std::path::PathBuf {
    super::userlib::state_dir().join("wishlist.json")
}

pub fn ver() -> u64 {
    VER.load(Ordering::Relaxed)
}

fn bump() {
    VER.fetch_add(1, Ordering::Relaxed);
}

pub fn with<R>(f: impl FnOnce(&Wishlist) -> R) -> R {
    let mut g = WL.lock().unwrap();
    f(g.get_or_insert_with(load))
}

fn edit(f: impl FnOnce(&mut Wishlist)) {
    {
        let mut g = WL.lock().unwrap();
        f(g.get_or_insert_with(load));
        if !knowledge::fixture() {
            super::userlib::write_json(&path(), g.as_ref().unwrap());
        }
    }
    bump();
}

fn load() -> Wishlist {
    if knowledge::fixture() {
        return fixture();
    }
    super::userlib::read_json(&path())
}

/// Adds a game in Rare (its price comes with the next refresh, started at once).
pub fn add(id: &str, title: &str, steam: Option<u32>) {
    let now = super::model::now_secs();
    edit(|w| match w.items.iter_mut().find(|i| i.id == id) {
        Some(i) => {
            i.hidden = false;
            i.rare = true;
        }
        None => w.items.push(Item { id: id.into(), title: title.into(), steam, added: now, rare: true, ..Default::default() }),
    });
    *AT.lock().unwrap() = 0.0;
}

/// Removes a game (a Steam one stays hidden here; Steam's own list is not changed).
pub fn remove(id: &str) {
    edit(|w| {
        if let Some(pos) = w.items.iter().position(|i| i.id == id) {
            if w.items[pos].steam_list {
                w.items[pos].hidden = true;
                w.items[pos].rare = false;
            } else {
                w.items.remove(pos);
            }
        }
    });
}

/// Every update: Steam's list and prices every 6 hours in front (at once after an add).
pub fn poll(fx: &mut crate::page::Fx, now: f64) {
    if knowledge::fixture() || BUSY.load(Ordering::Relaxed) {
        return;
    }
    {
        let mut at = AT.lock().unwrap();
        if !super::keep::due(*at, 6.0 * 3600.0, now) {
            return;
        }
        *at = now;
    }
    BUSY.store(true, Ordering::Relaxed);
    fx.spawn(|_| {
        refresh();
        BUSY.store(false, Ordering::Relaxed);
        Box::new(|_, _| {})
    });
}

/// Blocking: Steam's wishlist, then prices, then the drops worth a notice.
fn refresh() {
    let steam = steam_wishlist();
    let now = super::model::now_secs();
    if let Some(list) = &steam {
        edit(|w| {
            w.steam_at = now;
            w.steam_ok = true;
            for (appid, added, title) in list {
                let id = format!("steam:{appid}");
                match w.items.iter_mut().find(|i| i.id == id) {
                    Some(i) => {
                        i.steam_list = true;
                        if i.title.is_empty() {
                            i.title = title.clone();
                        }
                    }
                    None => w.items.push(Item { id, title: title.clone(), steam: Some(*appid), added: *added, steam_list: true, ..Default::default() }),
                }
            }
            // Gone from Steam's list and not added in Rare: gone here too.
            let ids: Vec<String> = list.iter().map(|(a, _, _)| format!("steam:{a}")).collect();
            w.items.retain(|i| !i.steam_list || i.rare || ids.contains(&i.id));
            for i in w.items.iter_mut().filter(|i| !ids.contains(&i.id)) {
                i.steam_list = false;
            }
        });
    }
    let ids: Vec<String> = with(|w| w.items.iter().filter(|i| !i.hidden).map(|i| i.id.clone()).collect());
    if ids.is_empty() {
        return;
    }
    let prices = match knowledge::prices(&ids) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("wishlist prices: {e}");
            return;
        }
    };
    let mut drops: Vec<(String, Price, f64)> = Vec::new();
    edit(|w| {
        w.prices_at = now;
        for p in prices {
            if let Some(i) = w.items.iter_mut().find(|i| i.id == p.id) {
                if i.title.is_empty() || i.title.starts_with("t:") {
                    i.title = p.title.clone();
                }
                if let Some(new) = p.price {
                    if let Some(old) = i.seen.filter(|o| *o > 0.0 && new < *o * 0.95 && *o - new >= 0.5) {
                        drops.push((i.title.clone(), p.clone(), old));
                    }
                    i.seen = Some(new);
                }
            }
            w.prices.insert(p.id.clone(), p);
        }
    });
    for (title, p, old) in drops {
        let mut n = rare_notify::LocalNotice::new(rare_notify::NoticeKind::Other("price_drop".into()), format!("Price drop: {title}"), rare_notify::types::NoticeAction::Unknown);
        n = n.body(format!("{} at {}, was {}", money(p.price.unwrap_or(0.0)), p.store.clone().unwrap_or_else(|| "a store".into()), money(old)));
        n.image_url = p.steam_app_id.map(|a| format!("https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{a}/header.jpg")).or(p.thumb.clone());
        crate::features::services::post(n);
    }
}

/// "$4.99", "Free".
pub fn money(v: f64) -> String {
    if v <= 0.0 {
        "Free".into()
    } else {
        format!("${v:.2}")
    }
}

// ---------------------------------------------------------------- Steam

/// The signed-in (else most recent) Steam account's id64, from loginusers.vdf.
fn steam_id64() -> Option<u64> {
    let root = rare_launch::steamcfg::primary_root()?;
    let text = std::fs::read_to_string(root.join("config/loginusers.vdf")).ok()?;
    let v = rare_library::vdf::parse(&text);
    let users = v.get("users")?;
    let mut best: Option<(u64, u64, u64)> = None;
    for (id, u) in users.entries() {
        let Ok(id64) = id.parse::<u64>() else { continue };
        let recent = u.str("MostRecent").or(u.str("mostrecent")) == Some("1");
        let ts = u.str("Timestamp").or(u.str("timestamp")).and_then(|t| t.parse::<u64>().ok()).unwrap_or(0);
        let rank = (recent as u64, ts, id64);
        if best.map_or(true, |b| rank > b) {
            best = Some(rank);
        }
    }
    best.map(|b| b.2)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).http_status_as_error(false).build().into()
}

/// (appid, added, name) on the account's Steam wishlist; None when there is no account or Steam
/// did not answer (a private profile answers with an empty list).
fn steam_wishlist() -> Option<Vec<(u32, i64, String)>> {
    let id = steam_id64()?;
    #[derive(Deserialize, Default)]
    struct It {
        appid: u32,
        #[serde(default)]
        date_added: i64,
    }
    #[derive(Deserialize, Default)]
    struct Resp {
        #[serde(default)]
        items: Vec<It>,
    }
    #[derive(Deserialize)]
    struct Top {
        response: Resp,
    }
    let mut r = agent().get(&format!("https://api.steampowered.com/IWishlistService/GetWishlist/v1/?steamid={id}")).call().ok()?;
    if r.status().as_u16() != 200 {
        return None;
    }
    let t: Top = r.body_mut().read_json().ok()?;
    let ids: Vec<u32> = t.response.items.iter().map(|i| i.appid).collect();
    let names = steam_names(&ids);
    Some(t.response.items.into_iter().map(|i| (i.appid, i.date_added, names.get(&i.appid).cloned().unwrap_or_default())).collect())
}

/// Store names for Steam apps (IStoreBrowseService, key-free), 50 at a time.
fn steam_names(ids: &[u32]) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    for chunk in ids.chunks(50) {
        let input = serde_json::json!({ "ids": chunk.iter().map(|a| serde_json::json!({ "appid": a })).collect::<Vec<_>>(), "context": { "language": "english", "country_code": "US", "steam_realm": 1 }, "data_request": {} });
        let url = format!("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json={}", urlenc(&input.to_string()));
        let Ok(mut r) = agent().get(&url).call() else { continue };
        let Ok(v) = r.body_mut().read_json::<serde_json::Value>() else { continue };
        for it in v["response"]["store_items"].as_array().into_iter().flatten() {
            if let (Some(a), Some(n)) = (it["appid"].as_u64(), it["name"].as_str()) {
                out.insert(a as u32, n.to_string());
            }
        }
    }
    out
}

fn urlenc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

// ---------------------------------------------------------------- fixtures

fn fixture() -> Wishlist {
    let now = super::model::now_secs();
    let mk = |a: u32, t: &str, steam_list: bool| Item { id: format!("steam:{a}"), title: t.into(), steam: Some(a), added: now - a as i64 % 1000 * 600, steam_list, rare: !steam_list, ..Default::default() };
    let items = match knowledge::scenario().as_str() {
        "none" => vec![],
        "one" => vec![mk(367520, "Hollow Knight", true)],
        "long" => {
            let mut v = vec![mk(1145360, "The Extraordinarily Long Title of a Game That Never Ends: Definitive Remastered Edition", false)];
            for (i, (a, t)) in [(367520, "Hollow Knight"), (504230, "Celeste"), (413150, "Stardew Valley"), (588650, "Dead Cells"), (1030300, "Hollow Knight: Silksong"), (268910, "Cuphead"), (632470, "Disco Elysium"), (1794680, "Vampire Survivors"), (646570, "Slay the Spire"), (1091500, "Cyberpunk 2077"), (292030, "The Witcher 3: Wild Hunt"), (1086940, "Baldur's Gate 3")].iter().enumerate() {
                v.push(mk(*a, t, i % 2 == 0));
            }
            v
        }
        _ => vec![mk(367520, "Hollow Knight", true), mk(504230, "Celeste", false), mk(413150, "Stardew Valley", true), mk(588650, "Dead Cells", true), mk(1030300, "Hollow Knight: Silksong", false)],
    };
    let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
    let prices = knowledge::prices(&ids).unwrap_or_default().into_iter().map(|p| (p.id.clone(), p)).collect();
    Wishlist { items, prices, prices_at: now, steam_at: now, steam_ok: true }
}
