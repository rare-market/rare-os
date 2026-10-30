//! Capture and streaming: instant replay (on or off, its length, the space clips may take),
//! Discord (what friends see, voice), streaming this device with Sunshine (PIN pairing, the paired
//! devices) and Rare devices on the network. The values come from `features::capture::info()`.

use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::features::capture::{self, Info, CAPS_GB, LENGTHS};
use crate::screens::overlay::common::*;
use rare_discord::Client as Dc;
use rare_render::Id;

pub struct CaptureSection {
    rows: RowSet,
    clients: Vec<String>,
}

fn small_button(cx: &mut Cx, nav: &mut Nav, parent: Id, label: &str, sid: &str, tag: u32) {
    let b = button(cx, parent, label, Variant::Secondary, Sz::Md, false).sid(sid).tag(tag);
    nav.add(b.group(11));
}

/// "1.3 GB", "640 MB".
pub fn size_text(bytes: u64) -> String {
    let gb = bytes as f64 / (1u64 << 30) as f64;
    if gb >= 1.0 {
        format!("{:.1} GB", gb)
    } else {
        format!("{} MB", (bytes >> 20).max(if bytes > 0 { 1 } else { 0 }))
    }
}

fn replay_body(i: &Info) -> String {
    if !i.replay_available {
        return "Needs gpu-screen-recorder, or GStreamer with VA-API. Neither is on this system.".into();
    }
    if !i.replay_on {
        return "Off. Turn it on to save the last moments of a game.".into();
    }
    let mut s = format!("Keeps {} while a game runs, on the GPU's video encoder. Uses a little battery.", capture::last_words(i.secs));
    if i.recording {
        s.push_str(" Recording now.");
    }
    s
}

fn discord_body(i: &Info) -> String {
    let d = &i.discord;
    if !i.presence_on {
        return "Off. Friends on Discord don't see what you play on Rare.".into();
    }
    match d.client {
        None => "Open Discord from Apps. Friends then see the game you play on Rare.".into(),
        Some(Dc::Web) if !d.arrpc_available => "Discord on the web needs arRPC, which comes with the next Rare OS update.".into(),
        Some(Dc::Web) => match &d.showing {
            Some(g) => format!("Friends see: Playing {g}."),
            None => "Discord on the web is open. Friends see every game you play.".into(),
        },
        Some(Dc::App) => match &d.showing {
            Some(g) => format!("Friends see: Playing {g}."),
            None => "The Discord app is open. Friends see games Discord knows.".into(),
        },
    }
}

fn host_body(i: &Info) -> String {
    let h = &i.host;
    if !h.available {
        return "Needs Sunshine, which comes with the next Rare OS update.".into();
    }
    if !i.host_on {
        return format!("Play this {}'s games on a TV, a PC or a phone with Moonlight. Off until you turn it on.", i.name);
    }
    if !h.running {
        return match (&h.error, h.stopped) {
            (Some(e), true) => format!("Sunshine couldn't start ({}). Turn this off and on to try again.", e.trim_start_matches("Fatal: ").trim_end_matches('.')),
            (None, true) => "Sunshine couldn't start. Turn this off and on to try again.".into(),
            _ => "Starting.".into(),
        };
    }
    match &i.ip {
        Some(ip) => format!("On. Moonlight on your network finds it as {}, or add {ip}.", i.name),
        None => format!("On. Moonlight on your network finds it as {}.", i.name),
    }
}

impl CaptureSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, _cfg: &Config, _now: f64) -> CaptureSection {
        Self::build_with(cx, nav, parent, &capture::info())
    }

    pub fn build_with(cx: &mut Cx, nav: &mut Nav, parent: Id, i: &Info) -> CaptureSection {
        let mut rows = RowSet::default();
        let mut clients = Vec::new();

        // Instant replay.
        let p = panel(cx, parent);
        rows.switch(cx, nav, p, "replay-toggle", "Instant replay", &replay_body(i), i.replay_on && i.replay_available, true);
        let labels: Vec<String> = LENGTHS.iter().map(|s| capture::length_label(*s)).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        rows.choice(cx, nav, p, "replay-length", "Length", Some("How much a saved clip keeps."), &labels, LENGTHS.iter().position(|s| *s == i.secs), false);
        let caps: Vec<String> = CAPS_GB.iter().map(|g| format!("{g} GB")).collect();
        let caps: Vec<&str> = caps.iter().map(String::as_str).collect();
        let used = format!("Clips use {} of {} GB. The oldest go first.", size_text(i.used_bytes), i.cap_gb);
        rows.choice(cx, nav, p, "replay-cap", "Space for clips", Some(&used), &caps, CAPS_GB.iter().position(|g| *g == i.cap_gb), false);
        let row_ = setting_row(cx, p, "Save a clip", Some("Rare and View in a game, or Save in Quick settings and the Rare menu. Clips go to Share with the game's name."), false);
        small_button(cx, nav, row_.right, "Open Share", "replay-share", 0);

        // Discord.
        let p = panel(cx, parent);
        rows.switch(cx, nav, p, "discord-presence", "Show what you play on Discord", &discord_body(i), i.presence_on, true);
        let voice = match i.discord.client {
            Some(_) if i.discord.deafened => "Deafened: you hear nothing from Discord and your microphone is muted.",
            Some(_) if i.discord.muted => "Your microphone is muted in Discord.",
            Some(_) => "Discord keeps running while you play. Mute and deafen are here and in the Rare menu.",
            None => "Discord keeps running while you play. Mute and deafen show here and in the Rare menu when it is open.",
        };
        let row_ = setting_row(cx, p, "Voice", Some(voice), false);
        if i.discord.client.is_some() {
            small_button(cx, nav, row_.right, if i.discord.muted { "Unmute" } else { "Mute" }, "discord-mute", 0);
            small_button(cx, nav, row_.right, if i.discord.deafened { "Undeafen" } else { "Deafen" }, "discord-deafen", 0);
        }

        // Stream this device.
        let p = panel(cx, parent);
        let title = format!("Stream this {}", i.name);
        rows.switch(cx, nav, p, "host-toggle", &title, &host_body(i), i.host_on && i.host.available, true);
        if i.host_on && i.host.running {
            let body = if i.host.pin_wanted {
                "A device is waiting for its PIN. Type the four digits it shows.".to_string()
            } else {
                format!("In Moonlight, pick {}. It shows a PIN: type it here.", i.name)
            };
            let row_ = setting_row(cx, p, "Pair a device", Some(&body), false);
            small_button(cx, nav, row_.right, "Enter PIN", "host-pin", 0);
            if i.clients.is_empty() {
                setting_row(cx, p, "No devices paired yet", Some("Paired devices show up here. Remove one to stop it from streaming."), false);
            }
            for (n, c) in i.clients.iter().enumerate() {
                let name = if c.name.trim().is_empty() { "Unnamed device" } else { c.name.as_str() };
                let row_ = setting_row(cx, p, name, Some("Paired. It can stream this device."), false);
                small_button(cx, nav, row_.right, "Remove", "host-unpair", n as u32);
                clients.push(c.uuid.clone());
            }
        }

        // Rare devices nearby.
        let p = panel(cx, parent);
        let head = if i.nearby.is_empty() {
            if i.nearby_done { "No other Rare devices found on this network." } else { "Rare devices on the same network show up here." }
        } else {
            "On the same network. One with streaming on can be played from Stream in Library."
        };
        let row_ = setting_row(cx, p, "Rare devices nearby", Some(head), true);
        small_button(cx, nav, row_.right, "Look again", "nearby-scan", 0);
        for (n, d) in i.nearby.iter().enumerate() {
            let body = format!("{}. {}", d.address, if d.stream { "Streaming is on." } else { "Streaming is off." });
            let row_ = setting_row(cx, p, &d.name, Some(&body), false);
            // Focusable so the list scrolls into view; A does nothing here.
            nav.add(Focusable { id: row_.row, kind: Kind::Row, group: 11, hint: String::new(), tag: n as u32, sid: "nearby-row".into() });
        }
        CaptureSection { rows, clients }
    }
}

impl SectionView for CaptureSection {
    fn press(&mut self, cx: &mut Cx, nav: &mut Nav, _cfg: &mut Config, now: f64) -> SectionOut {
        let Some(f) = nav.current().cloned() else { return SectionOut::None };
        let act = |a: String| SectionOut::Act(a);
        match f.sid.as_str() {
            "replay-toggle" | "discord-presence" | "host-toggle" => {
                let Some((_, v)) = self.rows.press(cx, nav, now) else { return SectionOut::None };
                let on = matches!(v, Val::Bool(true));
                act(match f.sid.as_str() {
                    "replay-toggle" => format!("capture:replay:{on}"),
                    "discord-presence" => format!("discord:presence:{on}"),
                    _ => format!("streamhost:toggle:{on}"),
                })
            }
            "replay-length" | "replay-cap" => {
                let Some((_, Val::Idx(i))) = self.rows.press(cx, nav, now) else { return SectionOut::None };
                if f.sid == "replay-length" {
                    act(format!("capture:length:{}", LENGTHS[i.min(LENGTHS.len() - 1)]))
                } else {
                    act(format!("capture:cap:{}", CAPS_GB[i.min(CAPS_GB.len() - 1)]))
                }
            }
            "replay-share" => act("open:share".into()),
            "discord-mute" => act("discord:mute".into()),
            "discord-deafen" => act("discord:deafen".into()),
            "host-pin" => act("streamhost:ask-pin".into()),
            "host-unpair" => match self.clients.get(f.tag as usize) {
                Some(u) => act(format!("streamhost:unpair:{u}")),
                None => SectionOut::None,
            },
            "nearby-scan" => act("nearby:scan".into()),
            _ => SectionOut::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_lines() {
        assert_eq!(size_text(0), "0 MB");
        assert_eq!(size_text(5 << 20), "5 MB");
        assert_eq!(size_text(1_340_000_000), "1.2 GB");
        let on = capture::fixture("on");
        assert!(replay_body(&on).contains("the last 30 seconds"));
        assert!(host_body(&on).contains("192.168.1.92"));
        assert!(discord_body(&on).contains("Playing Hades II"));
        let off = capture::fixture("off");
        assert!(replay_body(&off).starts_with("Off."));
        assert!(host_body(&off).contains("Off until you turn it on"));
        let na = capture::fixture("unavailable");
        assert!(host_body(&na).contains("next Rare OS update"));
        assert!(discord_body(&na).contains("arRPC"));
        for s in [replay_body(&on), host_body(&on), discord_body(&on), replay_body(&na), host_body(&na), discord_body(&na)] {
            assert!(!s.contains('\u{2014}'), "{s}");
        }
    }
}
