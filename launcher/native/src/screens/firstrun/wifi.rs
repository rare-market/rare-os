//! First run: Connect to Wi-Fi (launcher/src/onboarding/steps/Wifi.tsx). Coming online here moves on
//! by itself after 1.4 s (not when B brought you back to a connected screen).

use super::kit::{done_line, step, working};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::{button_el, empty, BSize, ButtonSpec, BVariant};

/// lib/types.ts `NetStatus`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Net {
    pub online: bool,
    pub wifi: Option<String>,
    /// NetworkManager lets the launcher change networks
    pub managed: bool,
}

/// `WifiNet`.
#[derive(Clone, Debug, PartialEq)]
pub struct WifiNet {
    pub ssid: String,
    pub signal: u8,
    pub secure: bool,
    pub active: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub net: Option<Net>,
    pub nets: Option<Vec<WifiNet>>,
    pub scanning: bool,
    pub joining: Option<String>,
    pub error: Option<String>,
    pub was_offline: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Continue,
    Scan,
    Join(String),
    Back,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Next,
    Back,
    Scan,
    /// keyboard "Password for <ssid>" (secret), then connect
    AskPassword(String),
    Connect { ssid: String, password: Option<String> },
    /// move on after 1.4 s
    NextSoon,
}

/// `Bars`: four bars for the signal.
pub fn bars<M>(signal: u8) -> El<M> {
    let n = if signal >= 75 {
        4
    } else if signal >= 50 {
        3
    } else if signal >= 25 {
        2
    } else {
        1
    };
    span("flex h-[0.95rem] items-end gap-[2px]").label(&format!("Signal {signal}%")).kids((1..=4).map(|i| span(cn(&["w-[3px] rounded-[1px]", if i <= n { "bg-fg/85" } else { "bg-fg/20" }])).attr("style", format!("height: {}%", i * 25)).key(i.to_string())))
}

pub fn view(st: &State) -> El<Msg> {
    let online = st.net.as_ref().is_some_and(|n| n.online);
    let managed = st.net.as_ref().is_some_and(|n| n.managed);
    let mut actions = vec![button_el(ButtonSpec::new(if online { "Continue" } else { "Continue offline" }).v(if online { BVariant::Primary } else { BVariant::Secondary }).size(BSize::Lg).def(online), Some(Msg::Continue))];
    if managed {
        actions.push(button_el(ButtonSpec::new("Scan again").v(BVariant::Ghost).size(BSize::Lg).disabled(st.scanning), Some(Msg::Scan)));
    }
    let mut c = div("flex flex-col gap-[1rem]");
    match &st.net {
        Some(n) if n.online => {
            c = c.kid(done_line(match &n.wifi {
                Some(w) => vec![text("Connected to "), span("font-semibold").text(w)],
                None => vec![text("Connected to the internet")],
            }))
        }
        Some(_) => c = c.kid(div("text-body text-muted").text("Not connected yet.")),
        None => {}
    }
    if st.scanning || st.joining.is_some() {
        let l = st.joining.as_ref().map(|j| format!("Joining {j}")).unwrap_or_else(|| "Looking for networks".into());
        c = c.kid(working(true, Some(&l)));
    }
    if let Some(e) = &st.error {
        c = c.kid(div("text-small text-neg").text(e));
    }
    if st.net.as_ref().is_some_and(|n| !n.managed) {
        c = c.kid(empty("Wi-Fi is managed outside Rare", Some(text("This system doesn't let the launcher change networks. Connect in desktop mode, then come back. If the device is online already, just continue.")), vec![], ""));
    } else if st.nets.as_ref().is_some_and(|n| n.is_empty()) && !st.scanning {
        c = c.kid(empty("No networks found", Some(text("Move closer to your router, or check that Wi-Fi is turned on, then scan again.")), vec![], ""));
    } else {
        c = c.kid(div("flex flex-col gap-[0.4rem]").attr("data-nav-group", "wifi-list").kids(st.nets.as_deref().unwrap_or(&[]).iter().enumerate().map(|(i, n)| {
            let tag = if n.active {
                span("text-small text-pos").text("Connected")
            } else if n.secure {
                span("text-small text-muted").text("Password")
            } else {
                span("text-small text-muted").text("Open")
            };
            button("flex items-center gap-[1rem] rounded-card border border-hair bg-l1 px-[1.2rem] py-[0.85rem] text-left", Some(Msg::Join(n.ssid.clone())))
                .def(!online && i == 0)
                .hint_a(if n.active { "Connected" } else { "Join" })
                .key(&n.ssid)
                .kid(icon("wifi", "shrink-0 text-icon").attr("size", "1.15rem"))
                .kid(span("min-w-0 flex-1 truncate text-body font-medium").text(&n.ssid))
                .kid(tag)
                .kid(bars(n.signal))
        })));
    }
    step(text("Connect to Wi-Fi"), Some(text("Games, updates, Rare Marketplace and your wallet need the internet. Pick your network.")), vec![c], actions, false).on_pad("b", Msg::Back)
}

pub fn update(st: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Continue => Some(Effect::Next),
        Msg::Back => Some(Effect::Back),
        Msg::Scan => {
            st.scanning = true;
            st.error = None;
            Some(Effect::Scan)
        }
        Msg::Join(ssid) => {
            let n = st.nets.as_ref()?.iter().find(|n| n.ssid == ssid)?.clone();
            if n.active {
                return None;
            }
            if n.secure {
                return Some(Effect::AskPassword(ssid));
            }
            Some(join(st, ssid, None))
        }
    }
}

/// The password from the keyboard (None cancelled).
pub fn password(st: &mut State, ssid: String, pw: Option<String>) -> Option<Effect> {
    pw.map(|p| join(st, ssid, Some(p)))
}

fn join(st: &mut State, ssid: String, password: Option<String>) -> Effect {
    st.joining = Some(ssid.clone());
    st.error = None;
    st.was_offline = true;
    Effect::Connect { ssid, password }
}

/// `wifiConnect`'s answer: a password problem says so plainly.
pub fn joined(st: &mut State, ssid: &str, res: Result<(), String>) {
    st.joining = None;
    if let Err(m) = res {
        let lower = m.to_lowercase();
        st.error = Some(if lower.contains("secret") || lower.contains("password") || lower.contains("802-11-wireless-security") {
            format!("Couldn't join {ssid}. Check the password and try again.")
        } else {
            crate::screens::wallet::fmt::failed(&format!("Couldn't join {ssid}."))
        });
    }
}

/// A status poll (every 3 s). Coming online after being offline here moves on.
pub fn status(st: &mut State, n: Net) -> Option<Effect> {
    let first = st.net.is_none();
    if first && !n.online {
        st.was_offline = true;
    }
    let came_online = n.online && st.was_offline;
    let scan = first && n.managed;
    st.net = Some(n);
    if came_online {
        st.was_offline = false;
        return Some(Effect::NextSoon);
    }
    if scan {
        st.scanning = true;
        return Some(Effect::Scan);
    }
    None
}

pub fn scanned(st: &mut State, res: Result<Vec<WifiNet>, String>) {
    st.scanning = false;
    match res {
        Ok(n) => st.nets = Some(n),
        Err(m) => {
            st.nets = Some(Vec::new());
            log::warn!("wifi scan: {m}");
            st.error = Some(crate::screens::wallet::fmt::failed("Couldn't look for networks."));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wifi() {
        let mut st = State::default();
        assert_eq!(status(&mut st, Net { online: false, wifi: None, managed: true }), Some(Effect::Scan));
        scanned(&mut st, Ok(vec![WifiNet { ssid: "Home".into(), signal: 80, secure: true, active: false }, WifiNet { ssid: "Cafe".into(), signal: 30, secure: false, active: false }]));
        let v = view(&st);
        assert_eq!(v.texts(), ["Connect to Wi-Fi", "Games, updates, Rare Marketplace and your wallet need the internet. Pick your network.", "Not connected yet.", "Home", "Password", "Cafe", "Open", "Continue offline", "Scan again"]);
        assert_eq!(v.landing().as_deref(), Some("HomePassword"));
        assert_eq!(update(&mut st, Msg::Join("Home".into())), Some(Effect::AskPassword("Home".into())));
        assert_eq!(password(&mut st, "Home".into(), Some("pw".into())), Some(Effect::Connect { ssid: "Home".into(), password: Some("pw".into()) }));
        assert!(view(&st).contains_text("Joining Home"));
        joined(&mut st, "Home", Err("Secrets were required".into()));
        assert!(view(&st).contains_text("Couldn't join Home. Check the password and try again."));
        assert_eq!(status(&mut st, Net { online: true, wifi: Some("Home".into()), managed: true }), Some(Effect::NextSoon));
        let v = view(&st);
        assert!(v.contains_text("Home"));
        assert_eq!(v.landing().as_deref(), Some("Continue"));
        let mut un = State::default();
        status(&mut un, Net { online: true, wifi: None, managed: false });
        assert!(view(&un).contains_text("Wi-Fi is managed outside Rare"));
    }
}
