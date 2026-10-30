//! Each tool, run on the UI thread through what the launcher already does: Play, the install and
//! uninstall sheets, Quick settings' backend calls, the Market's buy builder, the Local API's sell
//! sheet, Rare Wallet's coin send, Trade's swap, Friends. Slow parts run on workers (`fx.spawn` or a
//! thread); `reply` is called once with the outcome.
//!
//! Money and items: the builders here hand the transaction to Rare Wallet's review sheet and
//! answer the model as soon as the sheet is up. The player confirms (or not) on the device; the
//! result is the usual toast. Nothing here signs.

use super::{request_ui, UiReq};
use crate::act::Act;
use crate::page::Fx;
use crate::state::{Overlay, State, Tab};
use rare_agent::agent::Outcome;
use rare_agent::Call;
use serde_json::{json, Value};

pub type Reply = Box<dyn FnOnce(Outcome) + Send>;

const MIST: f64 = 1_000_000_000.0;
pub const SUI_TYPE: &str = "0x2::sui::SUI";
pub const USDC_TYPE: &str = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";

fn arg(c: &Call, k: &str) -> String {
    match &c.args[k] {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// A game is in front with the Rare menu over it (sheets need the menu out of the way).
fn over_game(st: &State) -> bool {
    !st.running.is_empty() && crate::features::ingame::on()
}

pub fn run(st: &mut State, fx: &mut Fx, c: &Call, reply: Reply) {
    if std::env::var("RR_FIXTURE").is_ok() {
        return reply(fixture(c));
    }
    match c.name.as_str() {
        "search_games" => reply(search_games(st, &arg(c, "query"))),
        "launch_game" => reply(launch_game(st, fx, &arg(c, "id"))),
        "install_game" => reply(install_game(st, fx, &arg(c, "id"))),
        "uninstall_game" => reply(uninstall_game(st, fx, &arg(c, "id"))),
        "downloads_status" => reply(downloads(st)),
        "storage_suggestions" => reply(storage(st)),
        "get_device_settings" => device_settings(fx, reply),
        "change_setting" => change_setting(st, fx, &arg(c, "setting"), &arg(c, "value"), reply),
        "open_page" => reply(open_page(st, &arg(c, "page"))),
        "wallet_balances" => reply(balances()),
        "wallet_send" => wallet_send(st, &arg(c, "to"), &arg(c, "amount"), &arg(c, "coin"), reply),
        "wallet_swap" => wallet_swap(&arg(c, "from"), &arg(c, "to"), &arg(c, "amount"), reply),
        "my_items" => reply(my_items(&arg(c, "query"))),
        "market_search" => market_search(fx, &arg(c, "query"), reply),
        "market_buy" => market_buy(fx, &arg(c, "item_id"), reply),
        "market_sell" | "market_list" => {
            let list = c.name == "market_list";
            let price = parse_amount(&arg(c, "price_sui"), 9);
            if list && price.is_none_or(|p| p == 0) {
                return reply(Outcome::err("Give the price in SUI, for example \"12.5\"."));
            }
            if over_game(st) {
                request_ui(UiReq::CloseMenu);
            }
            crate::features::localapi::agent_sell(fx, &arg(c, "item_id"), list, price, Box::new(move |r| reply(r.map_or_else(Outcome::err, |s| Outcome::ok(s.clone(), json!({ "status": "review_sheet_open", "note": s }))))));
        }
        "friends_list" => reply(friends_list()),
        "invite_friends" => reply(invite_friends(&c.args["friends"])),
        "message_friend" => reply(message_friend(&arg(c, "friend"), &arg(c, "text"))),
        other => reply(Outcome::err(format!("There is no tool called {other}."))),
    }
}

// ---------------------------------------------------------------- games

fn search_games(st: &State, q: &str) -> Outcome {
    let nq = norm(q);
    let score = |title: &str| -> Option<u8> {
        let t = norm(title);
        if nq.is_empty() {
            Some(2)
        } else if t == nq {
            Some(0)
        } else if t.starts_with(&nq) {
            Some(1)
        } else if t.contains(&nq) {
            Some(2)
        } else {
            None
        }
    };
    let mut games: Vec<(u8, &crate::data::model::Game)> = st.games().iter().filter_map(|g| score(&g.title).map(|s| (s, g))).collect();
    games.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.installed.cmp(&a.1.installed)).then(b.1.last_played.unwrap_or(0).cmp(&a.1.last_played.unwrap_or(0))));
    let lib: Vec<Value> = games
        .iter()
        .take(10)
        .map(|(_, g)| json!({ "id": g.id, "title": g.title, "source": crate::data::model::source_label(&g.source), "installed": g.installed, "running": st.is_running(&g.id), "playtimeMinutes": g.playtime_minutes }))
        .collect();
    let apps: Vec<Value> = if nq.is_empty() {
        vec![]
    } else {
        crate::screens::apps::catalog::apps().iter().filter(|a| score(&a.name).is_some()).take(4).map(|a| json!({ "id": crate::screens::apps::catalog::run_id(&a.id), "title": a.name, "kind": "app" })).collect()
    };
    let store: Vec<Value> = if nq.is_empty() {
        vec![]
    } else {
        crate::screens::sui::catalog::seed()
            .into_iter()
            .filter(|g| score(&g.name).is_some() && !lib.iter().any(|l| norm(l["title"].as_str().unwrap_or("")) == norm(&g.name)))
            .take(4)
            .map(|g| json!({ "id": format!("rare:{}", g.slug), "title": g.name, "kind": "rare_store", "note": "Not in the library: install_game opens its Rare Store page." }))
            .collect()
    };
    let n = lib.len() + apps.len() + store.len();
    let summary = match n {
        0 => format!("Nothing called \"{q}\""),
        1 => "1 match".to_string(),
        n => format!("{n} matches"),
    };
    Outcome::ok(summary, json!({ "library": lib, "apps": apps, "rareStore": store }))
}

fn launch_game(st: &mut State, fx: &mut Fx, id: &str) -> Outcome {
    let leave = over_game(st);
    if let Some(a) = id.strip_prefix("app:").and_then(crate::screens::apps::catalog::app_by_id) {
        let app = a.id.clone();
        if leave {
            request_ui(UiReq::Leave);
        }
        fx.run(Act::OpenApp(app));
        return Outcome::ok(format!("Opened {}", a.name), json!({ "ok": true, "title": a.name }));
    }
    // A small model may pass a name or a made-up id: the one installed game it clearly means.
    let g = st.game(id).cloned().or_else(|| {
        let want = norm(id.rsplit(':').next().unwrap_or(id));
        let hits: Vec<&crate::data::model::Game> = st.games().iter().filter(|g| g.installed && !want.is_empty() && norm(&g.title) == want).collect();
        (hits.len() == 1).then(|| hits[0].clone())
    });
    let Some(g) = g else { return Outcome::err("That id isn't in the library. Use search_games to get the id.") };
    if !g.installed {
        return Outcome::err(format!("{} isn't installed. install_game opens its install sheet.", g.title));
    }
    let again = st.is_running(&g.id);
    if leave {
        request_ui(UiReq::Launch(g.id.clone()));
    } else {
        request_ui(UiReq::CloseMenu);
        fx.run(Act::Launch(g.id.clone()));
    }
    if again {
        return Outcome::ok(format!("Back to {}", g.title), json!({ "ok": true, "title": g.title, "alreadyRunning": true }));
    }
    Outcome::ok(format!("Starting {}", g.title), json!({ "ok": true, "title": g.title }))
}

fn install_game(st: &mut State, fx: &mut Fx, id: &str) -> Outcome {
    if let Some(slug) = id.strip_prefix("rare:") {
        if over_game(st) {
            request_ui(UiReq::Leave);
        }
        st.open(Some(Overlay::Game(format!("rare:{slug}"))));
        return Outcome::ok("Opened its Rare Store page", json!({ "status": "store_page_open" }));
    }
    let Some(g) = st.game(id).cloned() else { return Outcome::err("That id isn't in the library. Use search_games.") };
    if g.installed {
        return Outcome::err(format!("{} is already installed.", g.title));
    }
    if over_game(st) {
        request_ui(UiReq::CloseMenu);
    }
    fx.run(Act::Install(g.id.clone()));
    Outcome::ok(format!("Install sheet for {}", g.title), json!({ "status": "install_sheet_open", "title": g.title, "note": "The player picks the drive and starts the download." }))
}

fn uninstall_game(st: &mut State, fx: &mut Fx, id: &str) -> Outcome {
    let Some(g) = st.game(id).cloned() else { return Outcome::err("That id isn't in the library. Use search_games.") };
    if !g.installed {
        return Outcome::err(format!("{} isn't installed.", g.title));
    }
    if over_game(st) {
        request_ui(UiReq::CloseMenu);
    }
    fx.run(Act::Uninstall(g.id.clone()));
    Outcome::ok(format!("Uninstall sheet for {}", g.title), json!({ "status": "uninstall_sheet_open", "title": g.title, "note": "The player confirms on the sheet." }))
}

fn downloads(st: &State) -> Outcome {
    let snap = &st.feat.downloads.snap;
    let title = |appid: u32, t: &str| if t.is_empty() { st.game(&format!("steam:{appid}")).map(|g| g.title.clone()).unwrap_or_else(|| format!("Steam app {appid}")) } else { t.to_string() };
    let items: Vec<Value> = snap
        .items
        .iter()
        .map(|d| {
            json!({
                "title": title(d.appid, &d.title),
                "percent": if d.total > 0 { (d.done as f64 * 100.0 / d.total as f64).round() } else { 0.0 },
                "doneBytes": d.done, "totalBytes": d.total,
                "state": if d.paused { "paused" } else if d.active { "downloading" } else { "queued" },
                "update": d.update,
                "mbPerSecond": (d.rate as f64 / 1e6 * 10.0).round() / 10.0,
            })
        })
        .collect();
    let others = crate::features::stores::menu_line();
    let n = items.len();
    let summary = match (n, &others) {
        (0, None) => "Nothing is downloading".to_string(),
        (0, Some(o)) => o.clone(),
        (1, _) => "1 download".to_string(),
        (n, _) => format!("{n} downloads"),
    };
    Outcome::ok(summary, json!({ "steam": items, "allPaused": snap.all_paused, "epicGog": others }))
}

fn storage(st: &State) -> Outcome {
    let snap = crate::features::storage::snapshot();
    let gb = |b: u64| (b as f64 / 1e9 * 10.0).round() / 10.0;
    let drives: Vec<Value> = snap.drives.iter().map(|d| json!({ "drive": d.label, "freeGB": gb(d.free), "totalGB": gb(d.total) })).collect();
    let Some(items) = snap.items.as_ref() else {
        crate::features::storage::want_fresh();
        return Outcome::ok("Measuring storage", json!({ "drives": drives, "note": "Rare is still measuring what each game uses. Ask again in a minute, or open Settings > Storage." }));
    };
    let now = crate::data::model::now_secs() as i64;
    let mut v: Vec<&rare_library::storage::Item> = items.iter().filter(|i| i.bytes > 200_000_000).collect();
    // Biggest first, games not played for a long time ahead of recent ones.
    let idle_days = |i: &rare_library::storage::Item| i.game_id.as_deref().and_then(|g| st.game(g)).and_then(|g| g.last_played).map(|t| (now - t).max(0) / 86400);
    v.sort_by(|a, b| {
        let w = |i: &rare_library::storage::Item| i.bytes as f64 * (1.0 + idle_days(i).unwrap_or(365) as f64 / 60.0);
        w(b).partial_cmp(&w(a)).unwrap_or(std::cmp::Ordering::Equal)
    });
    let list: Vec<Value> = v
        .iter()
        .take(8)
        .map(|i| json!({ "title": i.title, "sizeGB": gb(i.bytes), "source": i.source, "id": i.game_id, "lastPlayedDaysAgo": idle_days(i), "kind": format!("{:?}", i.kind) }))
        .collect();
    let free: u64 = snap.drives.iter().map(|d| d.free).sum();
    Outcome::ok(format!("{} GB free", gb(free)), json!({ "drives": drives, "biggest": list, "note": "Suggest; uninstall_game opens the sheet for a game." }))
}

// ---------------------------------------------------------------- settings

fn device_settings(fx: &mut Fx, reply: Reply) {
    fx.spawn(move |b| {
        let (p, l, r) = (b.perf(), b.levels(), b.radios());
        let dnd = crate::social::dnd();
        reply(Outcome::ok(
            "Read settings",
            json!({
                "powerProfile": p.as_ref().map(|p| p.profile.clone()),
                "brightness": l.and_then(|l| l.brightness).map(|v| v.round()),
                "volume": l.and_then(|l| l.volume).map(|v| v.round()),
                "wifi": r.and_then(|r| r.wifi),
                "doNotDisturb": dnd,
            }),
        ));
        Box::new(|_: &mut State, _| {})
    });
}

fn on_off(v: &str) -> Option<bool> {
    match v.to_lowercase().as_str() {
        "on" | "true" | "yes" | "1" | "enable" | "enabled" => Some(true),
        "off" | "false" | "no" | "0" | "disable" | "disabled" => Some(false),
        _ => None,
    }
}

fn change_setting(st: &mut State, fx: &mut Fx, setting: &str, value: &str, reply: Reply) {
    match setting {
        "power_profile" => {
            let id = match value.to_lowercase().as_str() {
                "quiet" | "battery" | "low" | "saver" => "quiet",
                "balanced" | "normal" | "default" => "balanced",
                "performance" | "high" | "max" => "performance",
                _ => return reply(Outcome::err("power_profile is quiet, balanced or performance.")),
            };
            fx.spawn(move |b| {
                let r = b.set_perf(id, None, None);
                let p = b.perf();
                Box::new(move |st: &mut State, _| {
                    st.qs.perf = p;
                    st.game_perf.perf = st.qs.perf.clone();
                    st.ver.qs += 1;
                    reply(match r {
                        Ok(()) => Outcome::ok(format!("Power profile: {id}"), json!({ "ok": true, "powerProfile": id })),
                        Err(e) => Outcome::err(e),
                    })
                })
            });
        }
        "brightness" | "volume" => {
            let Some(n) = value.trim_end_matches('%').trim().parse::<f32>().ok().map(|n| n.clamp(0.0, 100.0)) else {
                return reply(Outcome::err(format!("{setting} is a number from 0 to 100.")));
            };
            let bright = setting == "brightness";
            if let Some(l) = st.qs.levels.as_mut() {
                if bright {
                    l.brightness = Some(n);
                } else {
                    l.volume = Some(n);
                }
            }
            st.ver.qs += 1;
            fx.spawn(move |b| {
                let r = if bright { b.set_levels(Some(n), None) } else { b.set_levels(None, Some(n)) };
                reply(match r {
                    Ok(()) => Outcome::ok(format!("{} {}%", if bright { "Brightness" } else { "Volume" }, n.round()), json!({ "ok": true, "value": n.round() })),
                    Err(e) => Outcome::err(e),
                });
                Box::new(|_: &mut State, _| {})
            });
        }
        "wifi" => {
            let Some(on) = on_off(value) else { return reply(Outcome::err("wifi is on or off.")) };
            fx.spawn(move |b| {
                let r = b.set_radio("wifi", on);
                Box::new(move |st: &mut State, _| match r {
                    Ok(r) => {
                        st.qs.radios = Some(r);
                        st.ver.qs += 1;
                        reply(Outcome::ok(format!("Wi-Fi {}", if on { "on" } else { "off" }), json!({ "ok": true, "wifi": on })));
                    }
                    Err(e) => reply(Outcome::err(e)),
                })
            });
        }
        "do_not_disturb" => {
            let Some(on) = on_off(value) else { return reply(Outcome::err("do_not_disturb is on or off.")) };
            crate::social::set_dnd(on);
            st.qs.dnd = on;
            st.ver.qs += 1;
            reply(Outcome::ok(format!("Do not disturb {}", if on { "on" } else { "off" }), json!({ "ok": true, "doNotDisturb": on })));
        }
        _ => reply(Outcome::err("That setting can't be changed here.")),
    }
}

fn open_page(st: &mut State, page: &str) -> Outcome {
    if over_game(st) {
        request_ui(UiReq::Leave);
    } else {
        request_ui(UiReq::CloseMenu);
    }
    st.close();
    match page {
        "home" => st.set_tab(Tab::Home, false),
        "library" => st.set_tab(Tab::Library, false),
        "store" => st.set_tab(Tab::Games, false),
        "market" => st.set_tab(Tab::Market, false),
        "wallet" => st.set_tab(Tab::Wallet, false),
        "friends" => st.set_tab(Tab::Friends, false),
        "settings" => st.set_tab(Tab::Settings, false),
        "downloads" => st.open(Some(Overlay::Other(crate::features::downloads::KIND))),
        "trophies" => st.open(Some(Overlay::Other("trophies"))),
        _ => return Outcome::err("Unknown page."),
    }
    Outcome::ok(format!("Opened {page}"), json!({ "ok": true }))
}

// ---------------------------------------------------------------- wallet

/// "2.5" in base units with `dp` decimals. None when it isn't a plain positive number.
pub fn parse_amount(s: &str, dp: u32) -> Option<u64> {
    let s = s.trim().trim_end_matches("SUI").trim().replace(',', "");
    let (w, f) = s.split_once('.').unwrap_or((&s, ""));
    if w.is_empty() && f.is_empty() || !w.chars().all(|c| c.is_ascii_digit()) || !f.chars().all(|c| c.is_ascii_digit()) || f.len() > dp as usize {
        return None;
    }
    let mut frac = f.to_string();
    while frac.len() < dp as usize {
        frac.push('0');
    }
    let whole: u128 = if w.is_empty() { 0 } else { w.parse().ok()? };
    let v = whole.checked_mul(10u128.pow(dp))?.checked_add(if frac.is_empty() { 0 } else { frac.parse::<u128>().ok()? })?;
    u64::try_from(v).ok()
}

fn fmt_units(raw: u128, dp: u32) -> String {
    let d = 10u128.pow(dp);
    let (w, f) = (raw / d, raw % d);
    if f == 0 {
        return w.to_string();
    }
    let fs = format!("{f:0width$}", width = dp as usize);
    format!("{w}.{}", fs.trim_end_matches('0').chars().take(6).collect::<String>())
}

fn account() -> Result<crate::sign::Account, String> {
    let a = crate::sign::account().ok_or(crate::sign::NO_SIGNER)?;
    if !a.signer {
        return Err("This Rare Wallet account is watch only.".into());
    }
    Ok(a)
}

/// A coin by symbol or type: (type, symbol, decimals, balance in base units when held).
fn coin_of(s: &str, tokens: &[crate::screens::wallet::types::PortfolioToken]) -> Option<(String, String, u32, Option<u64>)> {
    let s = s.trim();
    let s = if s.is_empty() { "SUI" } else { s };
    let held = tokens.iter().find(|t| t.symbol.eq_ignore_ascii_case(s) || t.coin_type.eq_ignore_ascii_case(s) || (s.contains("::") && t.coin_type.ends_with(s.split_once("::").map(|x| x.1).unwrap_or("~"))));
    if let Some(t) = held {
        return Some((t.coin_type.clone(), t.symbol.clone(), t.decimals, u64::try_from(t.raw.max(0)).ok()));
    }
    match s.to_uppercase().as_str() {
        "SUI" => Some((SUI_TYPE.into(), "SUI".into(), 9, None)),
        "USDC" => Some((USDC_TYPE.into(), "USDC".into(), 6, None)),
        _ => None,
    }
}

fn balances() -> Outcome {
    let Some((addr, label)) = crate::data::account::current() else { return Outcome::err("There is no Rare Wallet account on this device.") };
    let held = crate::data::wallet::get(&addr);
    let Some(pf) = held.pf else {
        crate::data::wallet::refresh(&addr);
        return Outcome::err("The wallet is still loading. Ask again in a moment.");
    };
    let coins: Vec<Value> = pf.tokens.iter().take(20).map(|t| json!({ "symbol": t.symbol, "amount": t.amount, "usd": t.value_usd.map(|v| (v * 100.0).round() / 100.0), "type": t.coin_type })).collect();
    Outcome::ok(format!("{} coins", coins.len()), json!({ "account": label, "totalUsd": pf.total_usd.map(|v| (v * 100.0).round() / 100.0), "coins": coins }))
}

fn wallet_send(st: &State, to: &str, amount: &str, coin: &str, reply: Reply) {
    let a = match account() {
        Ok(a) => a,
        Err(e) => return reply(Outcome::err(e)),
    };
    let tokens = crate::data::wallet::get(&a.address).pf.map(|p| p.tokens).unwrap_or_default();
    let Some((coin_type, symbol, dp, balance)) = coin_of(coin, &tokens) else { return reply(Outcome::err(format!("The wallet doesn't hold {coin}."))) };
    let Some(raw) = parse_amount(amount, dp).filter(|r| *r > 0) else { return reply(Outcome::err("Give the amount as a number, for example \"2.5\".")) };
    if balance.is_some_and(|b| raw > b) {
        return reply(Outcome::err(format!("That is more {symbol} than the wallet holds.")));
    }
    // A friend by name first, then whatever the recipient field takes (0x..., name.sui).
    let friend = find_friend(to).map(|f| f.0);
    let _ = st;
    let to_text = friend.unwrap_or_else(|| to.to_string());
    std::thread::Builder::new()
        .name("ask-send".into())
        .spawn(move || {
            use rare_wallet::recipient::RecipientState as R;
            let Some(w) = crate::screens::wallet::service::wallet() else { return reply(Outcome::err("Rare Wallet isn't available.")) };
            let addr = match w.rt.block_on(rare_wallet::recipient::resolve_recipient(&w.names(), &to_text)) {
                R::Ready { address, .. } => address,
                R::Missing { msg, .. } | R::Error { msg, .. } | R::Invalid { msg } => return reply(Outcome::err(msg)),
                _ => return reply(Outcome::err("That recipient isn't a 0x address, a name.sui or a friend.")),
            };
            let p = rare_sui::tx::CoinSend { sender: a.address.clone(), to: addr.clone(), coin_type: coin_type.clone(), amount: raw, max: false, balance };
            let built = w.rt.block_on(rare_sui::tx::build_coin_send(&w.sui, &p));
            let (tx, opts) = match built {
                Ok(t) => t,
                Err(e) => return reply(Outcome::err(e.to_string())),
            };
            let human = fmt_units(raw as u128, dp);
            let title = format!("Send {human} {symbol}");
            let short = format!("{}...{}", &addr[..6.min(addr.len())], &addr[addr.len().saturating_sub(4)..]);
            reply(Outcome::ok(format!("Review sheet: {human} {symbol} to {short}"), json!({ "status": "review_sheet_open", "amount": human, "coin": symbol, "to": addr, "note": "The player confirms on the device." })));
            match crate::screens::wallet::service::send(rare_wallet::sign::TxInput::PtbGas(tx, opts), &a.address, &title) {
                Ok(r) if r.status.as_deref() != Some("failure") => crate::features::bus::note(format!("Sent {human} {symbol}"), false),
                Ok(_) => crate::features::bus::note("The network rejected this transaction.", true),
                Err(rare_wallet::sign::SignError::Rejected(_)) => {}
                Err(e) => crate::features::bus::note(e.to_string(), true),
            }
        })
        .ok();
}

fn wallet_swap(from: &str, to: &str, amount: &str, reply: Reply) {
    let a = match account() {
        Ok(a) => a,
        Err(e) => return reply(Outcome::err(e)),
    };
    let tokens = crate::data::wallet::get(&a.address).pf.map(|p| p.tokens).unwrap_or_default();
    let Some((cin, sin, din, bal)) = coin_of(from, &tokens) else { return reply(Outcome::err(format!("The wallet doesn't hold {from}."))) };
    let out = coin_of(to, &tokens).or_else(|| to.contains("::").then(|| (to.to_string(), to.rsplit("::").next().unwrap_or(to).to_string(), 0, None)));
    let Some((cout, sout, dout, _)) = out else { return reply(Outcome::err(format!("I don't know the coin {to}. Give its coin type (0x...::module::NAME)."))) };
    if cin == cout {
        return reply(Outcome::err("Pick two different coins."));
    }
    let Some(raw) = parse_amount(amount, din).filter(|r| *r > 0) else { return reply(Outcome::err("Give the amount as a number, for example \"2.5\".")) };
    if bal.is_some_and(|b| raw > b) {
        return reply(Outcome::err(format!("That is more {sin} than the wallet holds.")));
    }
    std::thread::Builder::new()
        .name("ask-swap".into())
        .spawn(move || {
            use rare_trade::swap::QuoteParams;
            let sw = crate::screens::trade::live::agent_swap();
            let q = match sw.quote(&QuoteParams { coin_in: cin.clone(), coin_out: cout.clone(), amount_in: raw as u128, owner: Some(a.address.clone()) }) {
                Ok(q) => q,
                Err(e) => return reply(Outcome::err(e.to_string())),
            };
            let b = match sw.build_default(&q, &a.address, false) {
                Ok(b) => b,
                Err(e) => return reply(Outcome::err(e.to_string())),
            };
            let human_in = fmt_units(raw as u128, din);
            let human_out = if dout > 0 { fmt_units(q.amount_out, dout) } else { q.amount_out.to_string() };
            let title = format!("Swap {human_in} {sin} for {sout}");
            reply(Outcome::ok(format!("Review sheet: {human_in} {sin} for about {human_out} {sout}"), json!({ "status": "review_sheet_open", "pay": format!("{human_in} {sin}"), "getAbout": format!("{human_out} {sout}"), "note": "The player confirms on the device." })));
            let req = rare_trade::wallet::SignRequest { tx: b.tx, account: a.address.clone(), title, origin_name: rare_trade::wallet::ORIGIN_NAME, origin_kind: rare_trade::wallet::ORIGIN_KIND, sponsorship: None };
            let o = crate::sign::sign_trade(&req, None);
            crate::screens::trade::live::note_outcome(&o, &format!("Swapped {human_in} {sin} for {sout}"));
        })
        .ok();
}

// ---------------------------------------------------------------- market

fn my_items(q: &str) -> Outcome {
    let Some((addr, _)) = crate::data::account::current() else { return Outcome::err("There is no Rare Wallet account on this device.") };
    let held = crate::data::wallet::get(&addr);
    let Some(inv) = held.inv else {
        crate::data::wallet::refresh(&addr);
        return Outcome::err("Your items are still loading. Ask again in a moment.");
    };
    let nq = norm(q);
    let sui = |m: &Option<String>| m.as_deref().and_then(|s| s.parse::<f64>().ok()).map(|v| (v / MIST * 1000.0).round() / 1000.0);
    let mut out = vec![];
    for g in &inv.groups {
        for it in &g.items {
            let coll = it.collection.as_ref().map(|c| c.name.clone()).unwrap_or_default();
            let game = g.game.as_ref().map(|x| x.name.clone());
            if !nq.is_empty() && !norm(&it.name).contains(&nq) && !norm(&coll).contains(&nq) && !game.as_deref().is_some_and(|x| norm(x).contains(&nq)) {
                continue;
            }
            out.push(json!({ "id": it.id, "name": it.name, "collection": coll, "game": game, "floorSui": sui(&it.floor_mist), "topOfferSui": sui(&it.top_offer_mist), "listedSui": it.listing.as_ref().and_then(|l| l.price_mist.parse::<f64>().ok()).map(|v| v / MIST), "staked": it.staked.is_some() }));
            if out.len() >= 25 {
                break;
            }
        }
    }
    Outcome::ok(format!("{} items", out.len()), json!({ "items": out, "total": inv.total }))
}

fn market_search(fx: &mut Fx, q: &str, reply: Reply) {
    let q = q.to_string();
    fx.spawn(move |_| {
        use rare_market::api::{self, paths, ItemCard, Page, SearchResult, Sort};
        let net = crate::screens::wallet::live::market_net();
        let sui = |m: Option<&str>| m.and_then(|s| s.parse::<f64>().ok()).map(|v| (v / MIST * 1000.0).round() / 1000.0);
        let r = match api::get::<SearchResult>(net, &paths::search(&q)) {
            Ok(r) => r,
            Err(_) => {
                reply(Outcome::err("Couldn't reach Rare's market."));
                return Box::new(|_: &mut State, _| {});
            }
        };
        let colls: Vec<Value> = r.collections.iter().take(5).map(|c| json!({ "slug": c.r#ref.slug, "name": c.r#ref.name, "verified": c.r#ref.verified, "floorSui": sui(c.floor_mist.as_deref()) })).collect();
        let mut cheapest = vec![];
        if let Some(c) = r.collections.first() {
            if let Ok(p) = api::get::<Page<ItemCard>>(net, &paths::items(&c.r#ref.slug, Sort::PriceAsc, "")) {
                for it in p.data.iter().filter(|i| i.listing.is_some()).take(5) {
                    let l = it.listing.as_ref().unwrap();
                    cheapest.push(json!({ "id": it.id, "name": it.name, "priceSui": sui(Some(l.all_in_mist.as_deref().unwrap_or(&l.price_mist))) }));
                }
            }
        }
        reply(Outcome::ok(format!("{} collections", colls.len()), json!({ "collections": colls, "cheapestListedInFirst": cheapest })));
        Box::new(|_: &mut State, _| {})
    });
}

fn market_buy(fx: &mut Fx, id: &str, reply: Reply) {
    let a = match account() {
        Ok(a) => a,
        Err(e) => return reply(Outcome::err(e)),
    };
    let id = id.to_string();
    fx.spawn(move |_| {
        use rare_market::api::{self, ItemDetail};
        let net = crate::screens::wallet::live::market_net();
        let r: Result<(rare_sui::ptb::Tx, String, f64), String> = (|| {
            let d = api::get::<ItemDetail>(net, &api::paths::item(&id)).map_err(|_| "Couldn't read that item.".to_string())?;
            let listing = d.card.listing.clone().ok_or_else(|| "This item is not listed.".to_string())?;
            let price = listing.all_in_mist.as_deref().unwrap_or(&listing.price_mist).parse::<f64>().unwrap_or(0.0) / MIST;
            let tx = rare_market::buy::build_buy_transaction(net, &Default::default(), &d.card, &listing, &a.address, None, None).map_err(|e| if e.0.is_empty() { "This item can't be bought here.".to_string() } else { e.0 })?;
            Ok((tx, d.card.name, price))
        })();
        match r {
            Err(e) => reply(Outcome::err(e)),
            Ok((tx, name, price)) => {
                reply(Outcome::ok(format!("Review sheet: {name} for {price} SUI"), json!({ "status": "review_sheet_open", "item": name, "priceSui": price, "note": "The player confirms on the device." })));
                let addr = a.address.clone();
                std::thread::spawn(move || match crate::sign::sign_and_execute(tx, &addr, &format!("Buy {name}")) {
                    Ok(_) => crate::features::bus::note(format!("Bought {name}."), false),
                    Err(e) if e == rare_market::inventory::notice::SIGN_REJECTED => {}
                    Err(e) => crate::features::bus::note(e, true),
                });
            }
        }
        Box::new(|_: &mut State, _| {})
    });
}

// ---------------------------------------------------------------- friends

/// A friend by what the player called them: (address, the name shown).
fn find_friend(name: &str) -> Option<(String, String)> {
    let social = crate::social::live()?;
    let d = social.data();
    let n = norm(name.trim_start_matches('@'));
    if n.is_empty() {
        return None;
    }
    let all: Vec<(String, String, Vec<String>)> = d
        .st
        .friends
        .iter()
        .map(|f| {
            let p = &f.person;
            let label = d.st.label_of(&p.address);
            (p.address.clone(), label.clone(), [Some(label), p.username.clone(), p.name.clone(), p.suins.clone()].into_iter().flatten().map(|x| norm(&x)).collect())
        })
        .collect();
    all.iter()
        .find(|(_, _, names)| names.iter().any(|x| *x == n))
        .or_else(|| all.iter().find(|(_, _, names)| names.iter().any(|x| x.starts_with(&n))))
        .or_else(|| all.iter().find(|(a, _, _)| a.eq_ignore_ascii_case(name.trim())))
        .map(|(a, l, _)| (a.clone(), l.clone()))
}

fn friends_list() -> Outcome {
    let Some(social) = crate::social::live() else { return Outcome::err("Friends isn't running.") };
    let d = social.data();
    let list: Vec<Value> = d
        .st
        .friends
        .iter()
        .take(50)
        .map(|f| json!({ "name": d.st.label_of(&f.person.address), "username": f.person.username, "status": f.presence.state.as_str(), "playing": f.presence.game.as_ref().map(|g| g.title.clone()) }))
        .collect();
    let online = d.st.friends.iter().filter(|f| f.presence.state.as_str() != "offline").count();
    Outcome::ok(format!("{} friends, {online} online", list.len()), json!({ "friends": list, "inParty": d.st.active_party().is_some() }))
}

fn invite_friends(names: &Value) -> Outcome {
    let Some(social) = crate::social::live() else { return Outcome::err("Friends isn't running.") };
    let names: Vec<String> = names.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let mut found = vec![];
    let mut missing = vec![];
    for n in &names {
        match find_friend(n) {
            Some(f) => found.push(f),
            None => missing.push(n.clone()),
        }
    }
    if found.is_empty() {
        return Outcome::err(format!("None of those are friends: {}.", missing.join(", ")));
    }
    let addrs = found.iter().map(|f| f.0.clone()).collect::<Vec<_>>().join(",");
    let party = social.data().st.active_party().map(|p| p.id.clone());
    match party {
        Some(id) => social.act(&format!("friends:invite:{id}:{addrs}")),
        None => social.act(&format!("friends:start-party:{addrs}")),
    };
    let who = found.iter().map(|f| f.1.clone()).collect::<Vec<_>>().join(", ");
    Outcome::ok(format!("Invited {who}"), json!({ "invited": who, "notFriends": missing }))
}

fn message_friend(name: &str, text: &str) -> Outcome {
    let Some(social) = crate::social::live() else { return Outcome::err("Friends isn't running.") };
    let Some((addr, label)) = find_friend(name) else { return Outcome::err(format!("{name} isn't one of your friends.")) };
    let text: String = text.trim().chars().take(280).collect();
    if text.is_empty() {
        return Outcome::err("The message is empty.");
    }
    social.answer(&format!("dm:{addr}"), text.clone());
    Outcome::ok(format!("Message to {label}"), json!({ "sent": true, "to": label }))
}

// ---------------------------------------------------------------- fixtures (screenshots, no device)

fn fixture(c: &Call) -> Outcome {
    match c.name.as_str() {
        "search_games" => Outcome::ok("1 match", json!({ "library": [{ "id": "steam:1145360", "title": "Hades", "installed": true }] })),
        "launch_game" => Outcome::ok("Starting Hades", json!({ "ok": true })),
        "wallet_send" => Outcome::ok("Review sheet: 2.5 SUI to 0x7ce7...61cb", json!({ "status": "review_sheet_open" })),
        "change_setting" => Outcome::ok("Brightness 40%", json!({ "ok": true })),
        _ => Outcome::ok("Done", json!({ "ok": true })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts() {
        assert_eq!(parse_amount("2.5", 9), Some(2_500_000_000));
        assert_eq!(parse_amount("1,000", 6), Some(1_000_000_000));
        assert_eq!(parse_amount(".5", 9), Some(500_000_000));
        assert_eq!(parse_amount("0.0000000001", 9), None);
        assert_eq!(parse_amount("-1", 9), None);
        assert_eq!(parse_amount("abc", 9), None);
        assert_eq!(parse_amount("3 SUI", 9), Some(3_000_000_000));
        assert_eq!(fmt_units(2_500_000_000, 9), "2.5");
        assert_eq!(fmt_units(3_000_000, 6), "3");
    }

    #[test]
    fn coins_by_symbol() {
        assert_eq!(coin_of("", &[]).unwrap().0, SUI_TYPE);
        assert_eq!(coin_of("usdc", &[]).unwrap().2, 6);
        assert!(coin_of("DOGE", &[]).is_none());
    }

    #[test]
    fn switches_read_like_people_say_them() {
        assert_eq!(on_off("On"), Some(true));
        assert_eq!(on_off("disable"), Some(false));
        assert_eq!(on_off("maybe"), None);
    }
}
