//! Capture and streaming: instant replay (on or off, its length, the space clips may take),
//! Discord (what friends see, voice), streaming this device with Sunshine (PIN pairing, the paired
//! devices) and Rare devices on the network. The values come from `features::capture::info()`.

use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::features::capture::{self, Info, CAPS_GB, LENGTHS};
use crate::screens::overlay::common::*;
use rare_discord::Client as Dc;
use rare_i18n::{tr, trf};
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
        crate::kit::format::in_gb(format!("{:.1}", gb))
    } else {
        crate::kit::format::in_mb((bytes >> 20).max(if bytes > 0 { 1 } else { 0 }))
    }
}

fn replay_body(i: &Info) -> String {
    let t = &tr().settings;
    if !i.replay_available {
        return t.cap_no_recorder.into();
    }
    if !i.replay_on {
        return t.cap_replay_off.into();
    }
    let mut s = trf!(t.cap_replay_on, last = capture::last_words(i.secs));
    if i.recording {
        s.push(' ');
        s.push_str(t.cap_recording);
    }
    s
}

fn discord_body(i: &Info) -> String {
    let t = &tr().settings;
    let d = &i.discord;
    if !i.presence_on {
        return t.cap_presence_off.into();
    }
    match d.client {
        None => t.cap_discord_closed.into(),
        Some(Dc::Web) if !d.arrpc_available => t.cap_discord_arrpc.into(),
        Some(Dc::Web) => match &d.showing {
            Some(g) => trf!(t.cap_friends_see, game = g),
            None => t.cap_discord_web.into(),
        },
        Some(Dc::App) => match &d.showing {
            Some(g) => trf!(t.cap_friends_see, game = g),
            None => t.cap_discord_app.into(),
        },
    }
}

fn host_body(i: &Info) -> String {
    let t = &tr().settings;
    let h = &i.host;
    if !h.available {
        return t.cap_no_sunshine.into();
    }
    if !i.host_on {
        return trf!(t.cap_host_off, name = i.name);
    }
    if !h.running {
        return match (&h.error, h.stopped) {
            (Some(e), true) => trf!(t.cap_host_failed_why, error = e.trim_start_matches("Fatal: ").trim_end_matches('.')),
            (None, true) => t.cap_host_failed.into(),
            _ => t.cap_host_starting.into(),
        };
    }
    match &i.ip {
        Some(ip) => trf!(t.cap_host_on_ip, name = i.name, ip = ip),
        None => trf!(t.cap_host_on, name = i.name),
    }
}

impl CaptureSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, _cfg: &Config, _now: f64) -> CaptureSection {
        Self::build_with(cx, nav, parent, &capture::info())
    }

    pub fn build_with(cx: &mut Cx, nav: &mut Nav, parent: Id, i: &Info) -> CaptureSection {
        let t = &tr().settings;
        let mut rows = RowSet::default();
        let mut clients = Vec::new();

        // Instant replay.
        let p = panel(cx, parent);
        rows.switch(cx, nav, p, "replay-toggle", t.row_replay, &replay_body(i), i.replay_on && i.replay_available, true);
        let labels: Vec<String> = LENGTHS.iter().map(|s| capture::length_label(*s)).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        rows.choice(cx, nav, p, "replay-length", t.row_length, Some(t.length_body), &labels, LENGTHS.iter().position(|s| *s == i.secs), false);
        let caps: Vec<String> = CAPS_GB.iter().map(|g| format!("{g} GB")).collect();
        let caps: Vec<&str> = caps.iter().map(String::as_str).collect();
        let used = trf!(t.cap_used, used = size_text(i.used_bytes), cap = i.cap_gb);
        rows.choice(cx, nav, p, "replay-cap", t.row_clip_space, Some(&used), &caps, CAPS_GB.iter().position(|g| *g == i.cap_gb), false);
        let row_ = setting_row(cx, p, t.row_save_clip, Some(t.save_clip_body), false);
        small_button(cx, nav, row_.right, t.open_share, "replay-share", 0);

        // Discord.
        let p = panel(cx, parent);
        rows.switch(cx, nav, p, "discord-presence", t.row_presence, &discord_body(i), i.presence_on, true);
        let voice = match i.discord.client {
            Some(_) if i.discord.deafened => t.voice_deafened,
            Some(_) if i.discord.muted => t.voice_muted,
            Some(_) => t.voice_running,
            None => t.voice_closed,
        };
        let row_ = setting_row(cx, p, t.row_voice, Some(voice), false);
        if i.discord.client.is_some() {
            small_button(cx, nav, row_.right, if i.discord.muted { t.unmute } else { t.mute }, "discord-mute", 0);
            small_button(cx, nav, row_.right, if i.discord.deafened { t.undeafen } else { t.deafen }, "discord-deafen", 0);
        }

        // Stream this device.
        let p = panel(cx, parent);
        let title = trf!(t.row_stream, name = i.name);
        rows.switch(cx, nav, p, "host-toggle", &title, &host_body(i), i.host_on && i.host.available, true);
        if i.host_on && i.host.running {
            let body = if i.host.pin_wanted {
                t.host_pin_waiting.to_string()
            } else {
                trf!(t.host_pin_how, name = i.name)
            };
            let row_ = setting_row(cx, p, t.row_pair, Some(&body), false);
            small_button(cx, nav, row_.right, t.enter_pin_btn, "host-pin", 0);
            if i.clients.is_empty() {
                setting_row(cx, p, t.no_paired, Some(t.no_paired_body), false);
            }
            for (n, c) in i.clients.iter().enumerate() {
                let name = if c.name.trim().is_empty() { t.unnamed_device } else { c.name.as_str() };
                let row_ = setting_row(cx, p, name, Some(t.paired_body), false);
                small_button(cx, nav, row_.right, tr().common.remove, "host-unpair", n as u32);
                clients.push(c.uuid.clone());
            }
        }

        // Rare devices nearby.
        let p = panel(cx, parent);
        let head = if i.nearby.is_empty() {
            if i.nearby_done { t.nearby_none } else { t.nearby_wait }
        } else {
            t.nearby_some
        };
        let row_ = setting_row(cx, p, t.row_nearby, Some(head), true);
        small_button(cx, nav, row_.right, t.look_again, "nearby-scan", 0);
        for (n, d) in i.nearby.iter().enumerate() {
            let body = trf!(if d.stream { t.nearby_line_on } else { t.nearby_line_off }, address = d.address);
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
