//! The Devices section: pair a phone so rare.market/devices on it controls this device (features/companion.rs),
//! with a QR code to scan, the pairing state, what the phone did lately, and Unpair.

use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::features::companion::{self, View};
use crate::screens::overlay::common::*;
use rare_render::Id;

pub struct DevicesSection;

/// Fixture runs (screenshots): RR_FIXTURE_DEVICES=signedout | code | paired | error, else the start.
pub fn fixture_view() -> View {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let base = View { live: false, signed_in: true, key: true, paired: Some(false), account: Some("0x7a3f0c1e9b2d4c6a8e0f1a3b5c7d9e1f2a4b6c8d0e1f3a5b7c9d1e3f5a7b9c1d".into()), ..Default::default() };
    match std::env::var("RR_FIXTURE_DEVICES").as_deref() {
        Ok("signedout") => View { signed_in: false, key: false, paired: None, account: None, ..base },
        Ok("code") => View { code: Some(("K7QF-M2XP".into(), "https://rare.market/devices/pair?code=K7QFM2XP".into(), now + 540_000)), ..base },
        Ok("error") => View { error: Some("rare.market did not answer. Check the connection and try again.".into()), ..base },
        Ok("paired") => View {
            paired: Some(true),
            recent: vec![
                (now - 40_000, "Typed into Search".into(), true),
                (now - 300_000, "Hollow Knight: Silksong Collector's Edition with a very long name is in the downloads".into(), true),
                (now - 900_000, "Nothing on the device is waiting for text. Open a text field first.".into(), false),
                (now - 3_600_000, "Saved holiday-photo-from-the-beach-2026-10-27.jpg to Share".into(), true),
                (now - 7_200_000, "Paired with your phone".into(), true),
            ],
            ..base
        },
        _ => base,
    }
}

fn short(a: &str) -> String {
    if a.len() > 12 {
        format!("{}...{}", &a[..6], &a[a.len() - 4..])
    } else {
        a.to_string()
    }
}

fn ago(at: i64, now: i64) -> String {
    let s = (now - at).max(0) / 1000;
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86_400),
    }
}

fn small_button(cx: &mut Cx, nav: &mut Nav, parent: Id, label: &str, sid: &str, primary: bool) {
    let b = button(cx, parent, label, if primary { Variant::Primary } else { Variant::Secondary }, Sz::Md, false).sid(sid);
    nav.add(b.group(11));
}

impl DevicesSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, _cfg: &Config, _now: f64) -> DevicesSection {
        let v = companion::view();
        let v = if v.live { v } else { fixture_view() };
        Self::build_with(cx, nav, parent, &v);
        DevicesSection
    }

    pub fn build_with(cx: &mut Cx, nav: &mut Nav, parent: Id, v: &View) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        let p = panel(cx, parent);
        setting_row(cx, p, "Phone control", Some("Use rare.market on your phone to see what this device is doing and to control it: install games, type, open links, send pictures and files, and get notified when downloads finish."), true);
        if !v.signed_in {
            let r = setting_row(cx, p, "Sign in first", Some("Sign in with Rare Wallet here, then sign in to rare.market on your phone with the same account."), false);
            small_button(cx, nav, r.right, "Sign in", "devices-signin", true);
            return;
        }
        match (v.paired, &v.code) {
            (Some(true), _) => {
                let who = v.account.as_deref().map(short).unwrap_or_default();
                let r = setting_row(cx, p, "Paired", Some(&format!("Any phone signed in to {who} on rare.market/devices controls this device.")), false);
                small_button(cx, nav, r.right, "Unpair", "devices-unpair", false);
                let r = setting_row(cx, p, "Add another phone", Some("Sign in on rare.market with the same account. No new code needed."), false);
                let _ = r;
            }
            (_, Some((code, url, exp))) if *exp > now => {
                let left = ((*exp - now) / 60_000).max(1);
                let body = format!("Scan with your phone's camera, or open rare.market/devices/pair and type {code}. Sign in with the same account. The code works for {left} more minute{}.", if left == 1 { "" } else { "s" });
                let r = setting_row(cx, p, &format!("Code {code}"), Some(&body), false);
                super::remote::qr_box(cx, r.right, url, 11.0);
                let r = setting_row(cx, p, "Waiting for your phone", Some("This page updates by itself once the phone confirms."), false);
                small_button(cx, nav, r.right, "New code", "devices-pair", false);
            }
            _ => {
                let body = if v.busy { "Working on it." } else { "Shows a code to scan with your phone. The phone needs the same account signed in at rare.market." };
                let r = setting_row(cx, p, "Pair a phone", Some(body), false);
                small_button(cx, nav, r.right, "Pair a phone", "devices-pair", true);
            }
        }
        if let Some(e) = &v.error {
            setting_row(cx, p, "Didn't work", Some(e), false);
        }
        if v.paired == Some(true) {
            let p = panel(cx, parent);
            setting_row(cx, p, "Recent from your phone", None, true);
            if v.recent.is_empty() {
                setting_row(cx, p, "Nothing yet", Some("Actions from your phone show up here."), false);
            }
            for (at, what, ok) in v.recent.iter().take(5) {
                let body = format!("{}{}", ago(*at, now), if *ok { "" } else { ", didn't work" });
                setting_row(cx, p, what, Some(&body), false);
            }
        }
    }
}

impl SectionView for DevicesSection {
    fn press(&mut self, _cx: &mut Cx, nav: &mut Nav, _cfg: &mut Config, _now: f64) -> SectionOut {
        let Some(f) = nav.current().cloned() else { return SectionOut::None };
        match f.sid.as_str() {
            "devices-pair" => SectionOut::Act("devices:pair".into()),
            "devices-unpair" => SectionOut::Act("devices:unpair".into()),
            "devices-signin" => SectionOut::Act("open:signin".into()),
            _ => SectionOut::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(short("0x7a3f0c1e9b2d4c6a8e0f"), "0x7a3f...8e0f");
        assert_eq!(ago(0, 30_000), "just now");
        assert_eq!(ago(0, 7_200_000), "2 h ago");
    }
}
