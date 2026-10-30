//! The Friends tab on live data: the page of `screens::friends`, fed by the social service
//! (`crate::social`). Without the service (fixtures, headless shots) it shows the sample friends.

use super::page::FriendsPage;
use crate::screens::market::host::*;
use crate::social::Social;
use rare_friends::nav::FriendsTarget;
use rare_render::input::Pad;
use rare_render::Id;
use std::sync::Arc;

pub struct FriendsTab {
    pub page: FriendsPage,
    social: Option<Arc<Social>>,
    seen: u64,
}

impl FriendsTab {
    pub fn new(cx: &mut Cx, parent: Id, social: Option<Arc<Social>>, now: f64) -> FriendsTab {
        let data = match &social {
            Some(s) => s.data(),
            None => super::model::mock(),
        };
        let seen = social.as_ref().map(|s| s.rev()).unwrap_or(0);
        FriendsTab { page: FriendsPage::new(cx, parent, data, now), social, seen }
    }

    /// Opens on a friend or a party (a notice, Home).
    pub fn open(&mut self, cx: &mut Cx, target: FriendsTarget, now: f64) {
        self.page.open(cx, target, now);
        self.follow_sel();
    }

    /// The friend on the right side: their messages and profile load, and their messages are read.
    fn follow_sel(&self) {
        if let Some(s) = &self.social {
            let addr = match (&self.page.v.sel, &self.page.v.picker) {
                (super::screen::Sel::Friend(a), None) => Some(a.as_str()),
                _ => None,
            };
            s.view_friend(addr);
        }
    }
}

impl Drop for FriendsTab {
    fn drop(&mut self) {
        if let Some(s) = &self.social {
            s.view_friend(None);
        }
    }
}

impl Page for FriendsTab {
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.page.page_hints()
    }
}

impl Screen for FriendsTab {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        let hit = self.page.touch_focus(cx, x, y, now);
        self.follow_sel();
        hit
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        let out = self.page.press(cx, pad, now);
        self.follow_sel();
        out
    }

    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if let Some(s) = &self.social {
            let rev = s.rev();
            if rev != self.seen {
                self.seen = rev;
                self.page.set_data(cx, s.data(), now);
            }
            if let Some(t) = s.take_target() {
                self.page.open(cx, t, now);
                self.follow_sel();
            }
        }
        self.page.tick(cx, now);
    }
}
