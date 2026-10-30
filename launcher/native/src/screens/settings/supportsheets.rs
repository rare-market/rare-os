//! The Support sheets (Settings > System), controller only: the time zone picker (regions, then
//! the cities of one), the factory reset (what goes, then type RESET), and the logs' link with a
//! QR code. A picks, B goes back or closes.

use super::support;
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::input::Pad;
use rare_render::theme::tok;
use rare_render::{Id, Node};
use rare_system::support as sup;

fn para(cx: &mut Cx, parent: Id, t: &str, small: bool) {
    let r = cx.r();
    let (sz, lh, c) = if small { (ty::SMALL.0, ty::SMALL.1, tok::MUTED) } else { (ty::BODY.0, ty::BODY.1, tok::FG.alpha(0.8)) };
    cx.scene.add_text(cx.t, parent, s().done(), t, ts(r, sz, lh, 400).wrap(), c);
}

fn step(nav: &mut Nav, cx: &mut Cx, pad: Pad, now: f64) -> Out {
    if nav.step(cx.scene, pad, now) {
        Out::Moved
    } else {
        Out::None
    }
}

// ---------------------------------------------------------------------------- time zone

pub struct TzSheet {
    nav: Nav,
    body: Id,
    zones: Vec<String>,
    current: Option<String>,
    /// None: the region list. Some: that region's cities.
    region: Option<String>,
    regions: Vec<(String, usize)>,
}

impl TzSheet {
    pub fn new(cx: &mut Cx, zones: Vec<String>, current: Option<String>, now: f64) -> TzSheet {
        let root = cx.scene.root;
        let sh = sheet(cx, root, "Time zone", Some("Pick your region, then the city nearest you."), false, false, now);
        let regions = sup::regions(&zones);
        let mut me = TzSheet { nav: Nav::default(), body: sh.body, zones, current, region: None, regions };
        me.show(cx, now);
        me
    }

    fn list_row(&mut self, cx: &mut Cx, label: &str, right: Option<&str>, tag: u32, sid: String) -> Id {
        let r = cx.r();
        let row_ = cx.scene.add(self.body, s().row().items_center().gap(1.0 * r).min_h(3.25 * r).wp(1.0).px(1.0 * r).py(0.6 * r).done(), Node::default().radius(0.5 * r));
        cx.scene.add_text(cx.t, row_, s().grow().done(), label, ts(r, ty::BODY.0, ty::BODY.1, 500).truncate(), tok::FG);
        if let Some(t) = right {
            text(cx, row_, ty::SMALL, 400, tok::MUTED, t);
        }
        self.nav.add(Focusable { id: row_, kind: Kind::Row, group: 1, hint: "Choose".into(), tag, sid })
    }

    fn show(&mut self, cx: &mut Cx, now: f64) {
        cx.scene.clear_children(self.body);
        cx.scene.n(self.body).scroll_y.snap(0.0);
        self.nav.clear();
        let cur_region = self.current.as_deref().map(|z| sup::region_of(z).to_string());
        let mut focus = None;
        match self.region.clone() {
            None => {
                if self.regions.is_empty() {
                    para(cx, self.body, "Couldn't read the list of time zones.", false);
                }
                // The common zones first; still on UTC, focus starts there.
                let common = sup::common(&self.zones);
                if !common.is_empty() {
                    let id = self.list_row(cx, "Common time zones", Some(&format!("{}", common.len())), 1000, "tz-region:Common".into());
                    if matches!(self.current.as_deref(), None | Some("UTC") | Some("Etc/UTC")) {
                        focus = Some(id);
                    }
                }
                for (i, (name, n)) in self.regions.clone().iter().enumerate() {
                    let here = cur_region.as_deref() == Some(name.as_str());
                    let right = if here { "Current".to_string() } else if name == "UTC" { String::new() } else { format!("{n}") };
                    let id = self.list_row(cx, name, Some(&right).filter(|r| !r.is_empty()).map(|r| r.as_str()), i as u32, format!("tz-region:{name}"));
                    if here && focus.is_none() {
                        focus = Some(id);
                    }
                }
            }
            Some(region) => {
                let r = cx.r();
                let common = region == "Common";
                let lab = cx.scene.add(self.body, s().pad(0.0, 0.0, 0.5 * r, 1.0 * r).done(), Node::default());
                text(cx, lab, ty::LABEL, 500, tok::MUTED, if common { "Common time zones" } else { &region });
                if !common {
                    text(cx, lab, ty::SMALL, 400, tok::MUTED, "LB and RB jump a letter");
                }
                let list = if common { sup::common(&self.zones) } else { sup::cities(&self.zones, &region) };
                for (i, z) in list.iter().enumerate() {
                    let here = self.current.as_deref() == Some(z.as_str());
                    let right = if here { Some("Current") } else if common { Some(sup::region_of(z)) } else { None };
                    let id = self.list_row(cx, &sup::city_name(z), right, i as u32, format!("tz:{z}"));
                    if here {
                        focus = Some(id);
                    }
                }
            }
        }
        if let Some(f) = focus.or_else(|| self.nav.items.first().map(|f| f.id)) {
            self.nav.focus(cx.scene, f, now, false);
            self.nav.reveal(cx.scene, f, now);
        }
    }
}

impl Screen for TzSheet {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::B if self.region.is_some() => {
                let back = self.region.take();
                self.show(cx, now);
                // Land on the region just left.
                if let Some(id) = back.and_then(|b| self.nav.items.iter().find(|f| f.sid == format!("tz-region:{b}")).map(|f| f.id)) {
                    self.nav.focus(cx.scene, id, now, false);
                }
                Out::Moved
            }
            Pad::B => Out::Close,
            Pad::A => {
                let Some(sid) = self.nav.current().map(|f| f.sid.clone()) else { return Out::None };
                if let Some(z) = sid.strip_prefix("tz:") {
                    return Out::Act(format!("time:zone:{z}"));
                }
                match sid.strip_prefix("tz-region:") {
                    Some("UTC") => Out::Act("time:zone:UTC".into()),
                    Some(region) => {
                        self.region = Some(region.to_string());
                        self.show(cx, now);
                        Out::Moved
                    }
                    None => Out::None,
                }
            }
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => step(&mut self.nav, cx, pad, now),
            // A long region: the first city of the next (or previous) letter.
            Pad::Lb | Pad::Rb if self.region.is_some() => {
                let names: Vec<(Id, char)> = self.nav.items.iter().map(|f| (f.id, f.sid.strip_prefix("tz:").map(sup::city_name).and_then(|n| n.chars().next()).unwrap_or(' '))).collect();
                let Some(at) = self.nav.cur.and_then(|c| names.iter().position(|(id, _)| *id == c)) else { return Out::None };
                let letter = names[at].1;
                let to = if pad == Pad::Rb {
                    names.iter().skip(at).position(|(_, l)| *l != letter).map(|k| at + k)
                } else {
                    // the start of this letter, or of the one before when already there
                    let start = names[..at].iter().rposition(|(_, l)| *l != letter).map_or(0, |k| k + 1);
                    if start < at { Some(start) } else { start.checked_sub(1).map(|p| names[..=p].iter().rposition(|(_, l)| *l != names[p].1).map_or(0, |k| k + 1)) }
                };
                match to {
                    Some(k) => {
                        let id = names[k].0;
                        self.nav.focus(cx.scene, id, now, false);
                        self.nav.reveal(cx.scene, id, now);
                        Out::Moved
                    }
                    None => Out::None,
                }
            }
            _ => Out::None,
        }
    }

    fn focus_id(&self) -> Option<Id> {
        self.nav.cur
    }
}

// ---------------------------------------------------------------------------- factory reset

const KEEP_GAMES: &str = "Signs out and clears Rare on this device: settings, accounts, wallets, friends, notices, Wi-Fi networks and Bluetooth pairings. Installed games and saves stay.";
const ERASE_ALL: &str = "Removes everything you added: games, saves, screenshots, files, accounts, wallets and Wi-Fi networks. Rare OS stays installed.";
const WALLETS: &str = "Wallets on this device are removed. Have the recovery phrase for each one before you go on, or they can't be brought back.";

pub struct ResetSheet {
    nav: Nav,
    body: Id,
    footer: Id,
    mode: Option<sup::ResetMode>,
    done: bool,
    message: Option<String>,
}

impl ResetSheet {
    pub fn new(cx: &mut Cx, now: f64) -> ResetSheet {
        let root = cx.scene.root;
        let sh = sheet(cx, root, "Factory reset", Some("Pick what to keep. The device restarts to reset."), true, true, now);
        let mut me = ResetSheet { nav: Nav::default(), body: sh.body, footer: sh.footer.unwrap(), mode: None, done: false, message: None };
        me.show(cx, now);
        me
    }

    fn show(&mut self, cx: &mut Cx, now: f64) {
        let r = cx.r();
        cx.scene.clear_children(self.body);
        cx.scene.clear_children(self.footer);
        self.nav.clear();
        let col = cx.scene.add(self.body, s().col().gap(0.75 * r).done(), Node::default());
        let f = self.footer;
        match self.mode {
            None => {
                text(cx, col, ty::BODY, 600, tok::FG, "Keep games");
                para(cx, col, KEEP_GAMES, false);
                text(cx, col, ty::BODY, 600, tok::FG, "Erase everything");
                para(cx, col, ERASE_ALL, false);
                para(cx, col, WALLETS, true);
                self.nav.add(button(cx, f, "Keep games", Variant::Secondary, Sz::Md, false).hint("Keep games").tag(1).group(1));
                self.nav.add(button(cx, f, "Erase everything", Variant::Secondary, Sz::Md, false).hint("Erase everything").tag(2).group(1));
                let cancel = self.nav.add(button(cx, f, "Cancel", Variant::Secondary, Sz::Md, false).hint("Cancel").tag(9).group(1));
                self.nav.focus(cx.scene, cancel, now, false);
            }
            Some(m) => {
                let (title, what) = match m {
                    sup::ResetMode::KeepGames => ("Keep games", KEEP_GAMES),
                    sup::ResetMode::EraseAll => ("Erase everything", ERASE_ALL),
                };
                text(cx, col, ty::BODY, 600, tok::FG, title);
                para(cx, col, what, false);
                para(cx, col, WALLETS, false);
                para(cx, col, "The device restarts and resets before Rare opens again. This can't be undone. To go on, type RESET.", true);
                self.nav.add(button(cx, f, "Type RESET", Variant::Primary, Sz::Md, false).hint("Type RESET").tag(4).group(1));
                let back = self.nav.add(button(cx, f, "Back", Variant::Secondary, Sz::Md, false).hint("Back").tag(5).group(1));
                self.nav.focus(cx.scene, back, now, false);
            }
        }
    }
}

impl Screen for ResetSheet {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::B if self.mode.is_some() => {
                self.mode = None;
                self.show(cx, now);
                Out::Moved
            }
            Pad::B => Out::Close,
            Pad::A => match self.nav.current().map(|f| f.tag) {
                Some(1) | Some(2) => {
                    self.mode = Some(if self.nav.current().map(|f| f.tag) == Some(1) { sup::ResetMode::KeepGames } else { sup::ResetMode::EraseAll });
                    self.show(cx, now);
                    Out::Moved
                }
                Some(4) => Out::Act("support:reset-type".into()),
                Some(5) => {
                    self.mode = None;
                    self.show(cx, now);
                    Out::Moved
                }
                Some(9) => Out::Close,
                _ => Out::None,
            },
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => step(&mut self.nav, cx, pad, now),
            _ => Out::None,
        }
    }

    fn answer(&mut self, _cx: &mut Cx, key: &str, value: Option<String>, _now: f64) -> Out {
        if key != "support:reset-type" {
            return Out::None;
        }
        let (Some(v), Some(m)) = (value, self.mode) else { return Out::None };
        if v.trim().eq_ignore_ascii_case("reset") {
            support::request(format!("reset:{}", m.arg()));
            self.done = true;
        } else if !v.trim().is_empty() {
            self.message = Some("That didn't match. Type RESET to go on.".into());
        }
        Out::None
    }

    fn finished(&self) -> bool {
        self.done
    }

    fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    fn focus_id(&self) -> Option<Id> {
        self.nav.cur
    }
}

// ---------------------------------------------------------------------------- the logs' link

pub struct LinkSheet {
    nav: Nav,
    link: String,
    copy_text: Option<Id>,
}

impl LinkSheet {
    pub fn new(cx: &mut Cx, link: &str, now: f64) -> LinkSheet {
        let r = cx.r();
        let root = cx.scene.root;
        let sh = sheet(cx, root, "Link to your logs", Some("Anyone with the link can download them for a month. It is copied already."), true, false, now);
        let row_ = cx.scene.add(sh.body, s().row().items_center().gap(1.25 * r).done(), Node::default());
        super::remote::qr_box(cx, row_, link, 9.0);
        let col = cx.scene.add(row_, s().col().grow().gap(0.4 * r).done(), Node::default());
        para(cx, col, "Scan the code with a phone, or paste the link into your bug report.", false);
        cx.scene.add_text(cx.t, col, s().done(), link, ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
        let f = sh.footer.unwrap();
        let mut nav = Nav::default();
        let copy = button(cx, f, "Copy link", Variant::Primary, Sz::Md, false).hint("Copy link").tag(1).group(1);
        let copy_text = match copy.kind {
            Kind::Btn { text, .. } => Some(text),
            _ => None,
        };
        nav.add(copy);
        let close = nav.add(button(cx, f, "Close", Variant::Secondary, Sz::Md, false).hint("Close").tag(2).group(1));
        nav.focus(cx.scene, close, now, false);
        LinkSheet { nav, link: link.to_string(), copy_text }
    }
}

impl Screen for LinkSheet {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::B => Out::Close,
            Pad::A => match self.nav.current().map(|f| f.tag) {
                Some(1) => {
                    if crate::app::share_up::copy_link(&self.link) {
                        if let Some(t) = self.copy_text {
                            cx.scene.set_text(cx.t, t, "Copied");
                        }
                    }
                    Out::None
                }
                Some(2) => Out::Close,
                _ => Out::None,
            },
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => step(&mut self.nav, cx, pad, now),
            _ => Out::None,
        }
    }

    fn focus_id(&self) -> Option<Id> {
        self.nav.cur
    }
}

/// Fixture screens: `support-tz`, `support-tz-america`, `support-reset`, `support-link`.
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    let zones: Vec<String> = ["America/New_York", "America/Chicago", "America/Los_Angeles", "America/Argentina/Buenos_Aires", "Europe/Berlin", "Europe/London", "Asia/Tokyo", "Australia/Sydney", "Africa/Lagos", "Pacific/Auckland", "UTC"].iter().map(|z| z.to_string()).collect();
    Some(match name {
        "support-tz" => Box::new(TzSheet::new(cx, zones, Some("America/New_York".into()), now)),
        "support-tz-america" => {
            let mut t = TzSheet::new(cx, zones, Some("America/New_York".into()), now);
            t.region = Some("America".into());
            t.show(cx, now);
            Box::new(t)
        }
        "support-reset" => Box::new(ResetSheet::new(cx, now)),
        "support-link" => Box::new(LinkSheet::new(cx, "https://aggregator.walrus-mainnet.walrus.space/v1/blobs/by-quilt-id/abc/rare-logs-2026-10-27-1532.tar.gz", now)),
        _ => return None,
    })
}
