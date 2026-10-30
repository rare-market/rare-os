//! Avatars asked for when their data arrives: the friends lists (every friend and request, at the
//! sizes the Friends screens draw them) and the player's own picture on the profile pages, so
//! opening those screens shows them at once instead of starting the fetches then.

use super::App;
use crate::kit::art::Arts;
use crate::state::State;
use rare_render::ImageLoader;

/// Friend rows (friends/screen.rs) and the Friends home pane (friends/home.rs).
const FRIEND_SIZES: [f32; 2] = [2.25, 1.75];
/// MImg at 160 web px: the profile header and the Home shelf (sui/profile.rs, sui/shelf.rs).
const PROFILE_REM: f32 = 10.0;

pub(super) fn prefetch(arts: &mut Arts, img: &mut ImageLoader, rem: f32, st: &State) {
    let stamp = rare_render::view::key(&(st.ver.friends, st.ver.live, st.ver.account));
    arts.prefetch(img, rem, stamp, || {
        let mut v: Vec<(String, f32)> = Vec::new();
        if let Some(f) = &st.friends {
            let people = f.st.friends.iter().map(|x| &x.person).chain(f.st.received.iter().map(|x| &x.person)).chain(f.st.sent.iter().map(|x| &x.person));
            for p in people {
                if let Some(u) = p.avatar_url.as_deref().filter(|u| !u.is_empty()) {
                    v.extend(FRIEND_SIZES.iter().map(|s| (u.to_string(), *s)));
                }
            }
        }
        if let Some(a) = st.live.profile.as_ref().and_then(|p| p.avatar.clone()) {
            v.push((a, PROFILE_REM));
        }
        v
    });
}

impl App {
    pub(super) fn prefetch_art(&mut self) {
        prefetch(&mut self.arts, &mut self.images, self.scene.rem, &self.st);
    }
}
