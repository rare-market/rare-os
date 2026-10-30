//! The Friends page on the Phase 4 host: data + the screen's state + the View/focus host. Pads:
//! the d-pad moves focus (a list row it lands on is selected and the right side follows), A
//! activates by tag (on a row: move to the right side), B closes a picker, else leaves.

use super::model::FriendsData;
use super::screen::{list_rows, press_tag, still_there, view, Picker, Press, Sel, View, T_ROW};
use crate::screens::market::host::*;
use rare_friends::nav::FriendsTarget;
use rare_friends::types::{Privacy, Visibility};
use rare_render::focus::FocusOpts;
use rare_render::input::Pad;
use rare_render::Id;

pub struct FriendsPage {
    pub data: FriendsData,
    pub v: View,
    pub host: Host,
}

/// Is this a list row's tag?
fn is_row(tag: u32) -> bool {
    (T_ROW..T_ROW + 1000).contains(&tag)
}

impl FriendsPage {
    pub fn new(cx: &mut Cx, parent: Id, data: FriendsData, now: f64) -> FriendsPage {
        let mut p = FriendsPage {
            data,
            v: View::default(),
            host: Host::new(cx, parent),
        };
        p.render(cx, now);
        p
    }

    pub fn render(&mut self, cx: &mut Cx, now: f64) {
        let (d, v) = (&self.data, &self.v);
        self.host
            .render(cx, now, |ui| super::bits::trunc_slack(view(ui, d, v)));
    }

    /// New data from rare-friends (a refresh, a stream event). An entry that went away falls back
    /// to the top (or, for an accepted request, to the new friend).
    pub fn set_data(&mut self, cx: &mut Cx, data: FriendsData, now: f64) {
        // Scrolled back in a conversation: new messages at the end keep the same lines on screen.
        if let (Sel::Friend(a), true) = (&self.v.sel, self.v.dm_back > 0) {
            let old_last = self.data.st.dms.get(a).and_then(|v| v.last()).map(|m| m.id.clone());
            if let (Some(old_last), Some(new)) = (old_last, data.st.dms.get(a)) {
                if let Some(i) = new.iter().position(|m| m.id == old_last) {
                    self.v.dm_back += new.len() - 1 - i;
                }
            }
        }
        self.data = data;
        if !still_there(&self.data, &self.v.sel) {
            let next = match &self.v.sel {
                Sel::Request(a) if self.data.st.friends.iter().any(|f| &f.address == a) => {
                    Sel::Friend(a.clone())
                }
                _ => Sel::Add,
            };
            self.v.sel = next;
            self.v.picker = None;
        }
        self.render(cx, now);
    }

    /// Opened at a person or a party (a notice, Home): select it and put focus on its row.
    pub fn open(&mut self, cx: &mut Cx, target: FriendsTarget, now: f64) {
        if let FriendsTarget::Send { url, kind } = target {
            self.select(Sel::Add);
            self.v.picker = Some(super::screen::Picker::Send { kind, url });
            self.render(cx, now);
            self.to_right(cx, now);
            return;
        }
        let s = &self.data.st;
        let next = match target {
            FriendsTarget::Send { .. } => None,
            FriendsTarget::Add => Some(Sel::Add),
            FriendsTarget::Party { id } => s
                .parties
                .iter()
                .any(|p| p.id == id)
                .then_some(Sel::Party(id)),
            FriendsTarget::Friend { address } => {
                if s.friends.iter().any(|f| f.address == address) {
                    Some(Sel::Friend(address))
                } else if s.received.iter().any(|f| f.address == address) {
                    Some(Sel::Request(address))
                } else {
                    None
                }
            }
        };
        let Some(next) = next else { return };
        self.select(next);
        self.render(cx, now);
        if let Some(i) = list_rows(&self.data).iter().position(|r| *r == self.v.sel) {
            self.host.focus_tag(cx, T_ROW + i as u32, now);
        }
    }

    /// Focus list row `i` (selecting it), optionally then moving to the right side as A does.
    pub fn focus_row(&mut self, cx: &mut Cx, i: usize, right: bool, now: f64) {
        let Some(row) = list_rows(&self.data).get(i).cloned() else {
            return;
        };
        self.select(row);
        self.render(cx, now);
        self.host.focus_tag(cx, T_ROW + i as u32, now);
        if right {
            self.to_right(cx, now);
        }
    }

    /// Focus the element tagged `tag` (fixtures and tests).
    pub fn focus(&mut self, cx: &mut Cx, tag: u32, now: f64) -> bool {
        self.host.focus_tag(cx, tag, now)
    }

    fn select(&mut self, sel: Sel) {
        if sel != self.v.sel {
            self.v.sure = None;
            self.v.more = false;
            self.v.dm_back = 0;
            self.v.add = Default::default();
        }
        self.v.picker = None;
        self.v.picked.clear();
        self.v.sel = sel;
    }

    /// The row under focus, if focus is on the list.
    fn focused_row(&self) -> Option<Sel> {
        let t = self.host.tag()?;
        is_row(t)
            .then(|| list_rows(&self.data).get((t - T_ROW) as usize).cloned())
            .flatten()
    }

    /// toRight: focus the right side's default button, else its first focusable.
    fn to_right(&mut self, cx: &mut Cx, now: f64) {
        let items = &self.host.nav.items;
        let right: Vec<&rare_render::focus::Focusable> = items
            .iter()
            .filter(|f| f.tag >= 2000 && !f.disabled)
            .collect();
        let pick = right
            .iter()
            .find(|f| f.default)
            .or_else(|| {
                right.iter().min_by(|a, b| {
                    (a.rect[1], a.rect[0])
                        .partial_cmp(&(b.rect[1], b.rect[0]))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            })
            .map(|f| f.id);
        if let Some(id) = pick {
            self.host.nav.focus(
                id,
                FocusOpts {
                    scroll: true,
                    smooth: false,
                },
                now,
            );
            self.host
                .view
                .set_focus(cx.scene, cx.t, self.host.nav.current(), now);
        }
    }

    /// The hint bar: A as the focused element says, B back.
    pub fn page_hints(&self) -> Vec<(&'static str, String)> {
        let it = self.host.nav.current_item();
        let a = it
            .and_then(|f| f.hint_a.clone())
            .unwrap_or_else(|| "Select".into());
        let mut v = vec![("a", a)];
        if let Some(x) = it.and_then(|f| f.hint_x.clone()) {
            v.push(("x", x));
        }
        v.push(("b", "Back".into()));
        v
    }

    pub fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Out {
        match b {
            Pad::B if self.v.picker.is_some() => {
                self.v.picker = None;
                self.v.picked.clear();
                self.render(cx, now);
                self.to_right(cx, now);
                Out::Moved
            }
            Pad::B => Out::Close,
            // X on a friend's page swaps Remove and Block into the action row (and back).
            Pad::X if matches!(self.v.sel, Sel::Friend(_)) && self.v.picker.is_none() => {
                self.v.more = !self.v.more;
                self.v.sure = None;
                self.render(cx, now);
                self.focus_actions(cx, now);
                Out::Moved
            }
            Pad::Up | Pad::Down if self.host.tag() == Some(super::screen::T_DM_PANEL) => match self.scroll_dms(b == Pad::Up) {
                Some(out) => {
                    self.render(cx, now);
                    out
                }
                None => self.step(cx, b, now),
            },
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => self.step(cx, b, now),
            Pad::Lt | Pad::Rt => {
                if self.host.jump(cx, b == Pad::Rt, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Pad::A => self.activate(cx, now),
            Pad::Y => Out::Act("search".into()),
            _ => Out::None,
        }
    }

    /// Up and Down on the conversation box move through its history, one message a press; past
    /// the oldest loaded page the next one is asked for. None: the box is at that end (focus moves).
    fn scroll_dms(&mut self, up: bool) -> Option<Out> {
        let Sel::Friend(a) = &self.v.sel else { return None };
        let len = self.data.st.dms.get(a).map(|v| v.len()).unwrap_or(0);
        let full = self.data.st.dm_full.contains(a);
        if up {
            let top = len.saturating_sub(super::profile::TOP_KEEP);
            if self.v.dm_back >= top {
                return (!full && len > 0).then(|| Out::Act(format!("friends:dm-older:{a}")));
            }
            self.v.dm_back = (self.v.dm_back + 1).min(top);
            // Ask for the next page a little before the top, so it is there when the box gets there.
            if !full && self.v.dm_back + 20 >= len {
                return Some(Out::Act(format!("friends:dm-older:{a}")));
            }
            Some(Out::Moved)
        } else {
            if self.v.dm_back == 0 {
                return None;
            }
            self.v.dm_back -= 1;
            Some(Out::Moved)
        }
    }

    /// Focus the first button of the friend's action row (after X swapped it).
    fn focus_actions(&mut self, cx: &mut Cx, now: f64) {
        let tags = [super::screen::T_REMOVE, super::screen::T_MESSAGE, super::screen::T_KEEP];
        for t in tags {
            if self.host.focus_tag(cx, t, now) {
                return;
            }
        }
    }

    fn step(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Out {
        match b {
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                let moved = self.host.step(cx, b, now);
                if let Some(row) = self.focused_row() {
                    if row != self.v.sel || self.v.picker.is_some() {
                        self.select(row);
                        self.render(cx, now);
                    }
                }
                if moved {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            _ => Out::None,
        }
    }

    fn activate(&mut self, cx: &mut Cx, now: f64) -> Out {
        let Some(tag) = self.host.tag() else {
            return Out::None;
        };
        let picker_before = self.v.picker.clone();
        match press_tag(&self.data, &mut self.v, tag) {
            None => Out::None,
            Some(Press::Row(sel)) => {
                self.select(sel);
                self.render(cx, now);
                self.to_right(cx, now);
                Out::Moved
            }
            Some(Press::Local) => {
                self.render(cx, now);
                use super::screen::{T_BLOCK, T_KEEP, T_LESS, T_REMOVE, T_YES};
                if [T_REMOVE, T_BLOCK, T_KEEP, T_LESS, T_YES].contains(&tag) {
                    self.focus_actions(cx, now);
                }
                if self.v.picker != picker_before {
                    self.to_right(cx, now);
                }
                Out::Moved
            }
            Some(Press::Act(a)) => {
                self.local_effect(&a);
                self.render(cx, now);
                if self.v.picker != picker_before {
                    self.to_right(cx, now);
                }
                Out::Act(a)
            }
        }
    }

    /// What the screen shows at once for an action (the web store does the same once the API
    /// answers): privacy switches flip, the mock code appears.
    fn local_effect(&mut self, act: &str) {
        let mut parts = act.split(':');
        if parts.next() != Some("friends") {
            return;
        }
        match parts.next() {
            Some("privacy") => {
                let (Some(field), Some(val)) = (parts.next(), parts.next()) else {
                    return;
                };
                let on = val == "true";
                if let Some(me) = self.data.st.me.as_mut() {
                    let p: &mut Privacy = &mut me.settings;
                    match field {
                        "showPlaying" => p.show_playing = on,
                        "appearOffline" => p.appear_offline = on,
                        _ => {
                            p.visibility = if on {
                                Visibility::Everyone
                            } else {
                                Visibility::Friends
                            }
                        }
                    }
                }
            }
            Some("new-code") if self.data.code.is_none() => {
                // The app replaces this with the server's code; the fixture's mock answers at once.
                if self.data.st.address.as_deref() == Some(super::model::MOCK_ME) {
                    self.data.code = Some(super::model::mock_code(self.data.now_ms));
                }
            }
            Some("load-items")
                if self.data.items.is_none() && matches!(self.v.picker, Some(Picker::Item(_))) => {}
            _ => {}
        }
    }
}

impl Page for FriendsPage {
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.page_hints()
    }
}

impl Screen for FriendsPage {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.host.touch_focus(cx, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        self.press(cx, pad, now)
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if self.host.tick(cx, now) {
            self.render(cx, now);
        }
    }
}
