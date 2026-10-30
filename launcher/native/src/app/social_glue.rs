//! The app side of Friends and notices (`crate::social`): feeding the service the library, showing
//! notice toasts, running Friends actions, and opening what a notice is about.

use super::*;
use crate::screens::notify::toast::{open_newest, ToastLayer};
use crate::screens::notify::ui::ArtSource;
use crate::social::{After, Social};
use rare_notify::actions::{action_target, ActionTarget};
use crate::screens::wallet::home::stack::WalletView;
use rare_notify::types::Notice;
use rare_render::Src;

/// Notice art: the notice's own picture, else the item's.
pub(super) struct LiveArt;

impl ArtSource for LiveArt {
    fn art(&self, n: &Notice) -> Option<Src> {
        let url = n.image_url.as_deref().or_else(|| n.item.as_ref().and_then(|i| i.image_url.as_deref())).filter(|u| !u.trim().is_empty())?;
        crate::data::art::single(url, 128.0)
    }
}

/// Whether the live service runs (not in fixtures and golden shots).
pub(super) fn social_enabled() -> bool {
    std::env::var("RR_SOCIAL").is_ok() || (std::env::var("RR_FIXTURE").is_err() && std::env::var("RR_NO_SOCIAL").is_err() && std::env::var("RR_SHOT").is_err())
}

impl App {
    /// `$XDG_RUNTIME_DIR/rare-playing` exists while a game runs, so the OS updater (os-update)
    /// does not start or go on with an automatic download during a game.
    pub(super) fn playing_marker(&self) {
        use std::sync::atomic::{AtomicU8, Ordering};
        static LAST: AtomicU8 = AtomicU8::new(2);
        if std::env::var_os("RR_FIXTURE").is_some() || std::env::var_os("RR_SHOT").is_some() {
            return;
        }
        let on = !self.st.running.is_empty();
        if LAST.swap(on as u8, Ordering::Relaxed) == on as u8 {
            return;
        }
        let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") else { return };
        let f = std::path::PathBuf::from(dir).join("rare-playing");
        if on {
            let _ = std::fs::write(&f, self.st.running.join("\n"));
        } else {
            let _ = std::fs::remove_file(&f);
        }
    }

    /// Called every update: library to the service, its toasts to the shell, notice toasts drawn.
    pub(super) fn social_update(&mut self, now: f64, wake: &mut Vec<Option<f64>>) {
        let Some(social) = self.social.clone() else { return };
        if social.rev() != self.st.ver.friends {
            self.st.ver.friends = social.rev();
            self.st.friends = Some(std::sync::Arc::new(social.data()));
        }
        let v = (self.st.ver.library, self.st.ver.running);
        if v != self.social_seen.0 {
            self.social_seen.0 = v;
            social.set_library(&self.st.running, self.st.games());
        }
        for n in std::mem::take(&mut self.st.feat.downloads.notices) {
            social.notify_local(n);
        }
        for (text, error) in social.take_toasts() {
            self.st.toast(text, if error { Tone::Error } else { Tone::Info }, now);
        }
        for a in social.take_pending() {
            self.run_after(a);
        }
        if self.st.osk_answer.as_ref().is_some_and(|(k, _)| k.starts_with("social:")) {
            if let Some((k, v)) = self.st.osk_answer.take() {
                if let Some(text) = v {
                    social.answer(&k["social:".len()..], text);
                }
            }
        }
        if self.st.tab == Tab::Settings && crate::screens::settings::live::enabled() && social.rev() != self.social_names {
            self.social_names = social.rev();
            crate::screens::settings::live::set_username(social.data().my_username);
        }
        let expired = social.tick();
        let key = (social.rev(), social.with_store(|s| s.toasts.len()));
        if expired || key != self.social_seen.1 {
            self.social_seen.1 = key;
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            let layer = self.notice_toasts.get_or_insert_with(|| ToastLayer::build(&mut cx));
            social.with_store(|store| layer.sync(&mut cx, store, &[], &LiveArt, now));
        }
        // The app's toasts stack under the notice toasts (same corner).
        let below = self.notice_toasts.as_ref().map(|l| l.height(&self.scene)).filter(|h| *h > 0.0).map(|h| h + 0.5 * self.scene.rem).unwrap_or(0.0);
        let host = self.toasts.view.host();
        if (self.scene.get(host).ty.target() - below).abs() > 0.5 {
            self.scene.n(host).ty.set(below, now, 0.15);
        }
        // The next toast to expire.
        let t_ms = wall_ms() as i64;
        let at = social.with_store(|s| s.toasts.iter().map(|t| t.at + rare_notify::store::TOAST_MS).min());
        wake.push(at.map(|a| now + ((a - t_ms).max(0) as f64) / 1000.0));
    }

    /// Menu (Start) on the newest notice toast opens what it is about. True when one was up.
    pub(super) fn notice_start(&mut self) -> bool {
        let Some(social) = self.social.clone() else { return false };
        let Some(n) = social.with_store(|s| open_newest(s, wall_ms() as i64)) else { return false };
        self.open_notice(&n);
        true
    }

    /// Where a notice leads.
    pub(super) fn open_notice(&mut self, n: &Notice) {
        let now = clock::secs();
        // A transaction notice opens its detail in the wallet's activity.
        let digest = match &n.action {
            rare_notify::types::NoticeAction::Tx { digest } => Some(digest.clone()),
            rare_notify::types::NoticeAction::Unknown => n.digest.clone().filter(|d| !d.is_empty()),
            _ => None,
        };
        if let Some(digest) = digest {
            self.show_wallet(WalletView::Activity { digest });
            return;
        }
        // A wishlist price drop opens the wishlist.
        if n.kind.as_str() == "price_drop" {
            self.close_legacy();
            self.st.close();
            self.st.open(Some(Overlay::Other(crate::features::wishlist_page::KIND)));
            return;
        }
        // A download notice opens the Downloads view.
        if matches!(n.action, rare_notify::types::NoticeAction::Downloads) {
            self.close_legacy();
            self.st.close();
            self.st.set_tab(Tab::Library, false);
            self.st.open(Some(Overlay::Other(crate::features::downloads::KIND)));
            return;
        }
        match action_target(Some(&n.action), n.digest.as_deref(), Some(n)) {
            ActionTarget::Friends { address } => {
                self.show_friends(address.map(|a| rare_friends::nav::FriendsTarget::Friend { address: a }));
            }
            ActionTarget::Party { id } => self.show_friends(Some(rare_friends::nav::FriendsTarget::Party { id })),
            ActionTarget::Library => self.show_tab(Tab::Library),
            ActionTarget::QuickSettings => self.st.open(Some(Overlay::Qam)),
            ActionTarget::AcceptOffer { item_id } => {
                self.close_legacy();
                self.st.close();
                self.st.set_tab(Tab::Wallet, false);
                crate::screens::wallet::page::open_accept(item_id, n.amount.as_ref().map(|a| a.raw.clone()));
            }
            ActionTarget::Trophies { appid } => {
                self.close_legacy();
                self.st.close();
                crate::features::trophies::set_open(appid);
                self.st.open(Some(Overlay::Other("trophies")));
            }
            ActionTarget::MarketItem { id } => self.show_market(format!("item:{id}")),
            ActionTarget::MarketCollection { slug } => self.show_market(format!("collection:{slug}")),
            ActionTarget::TradePerp { market_id } => self.show_market(format!("perp:{market_id}")),
            ActionTarget::TradeToken { coin_type } => self.show_market(format!("token:{coin_type}")),
            ActionTarget::Trader { address } => self.show_market(format!("trader:{address}")),
            ActionTarget::OsRestart => {
                self.show_tab(Tab::Settings);
                self.open_os_sheet(true);
            }
            ActionTarget::EmulatorSettings => self.show_settings(crate::screens::settings::sections::SectionId::Emulators),
            ActionTarget::Web { url } => {
                self.close_legacy();
                self.st.close();
                if let Err(e) = crate::features::webgame::open_external(&url) {
                    log::warn!("open web {url}: {e}");
                    self.st.toast("Couldn't open that page. Try again.", Tone::Error, now);
                }
            }
            ActionTarget::Nothing => {}
        }
    }

    /// Back to the top level on `tab`: no overlay, no sheet.
    pub(super) fn show_tab(&mut self, tab: Tab) {
        self.close_legacy();
        self.st.close();
        self.st.set_tab(tab, false);
    }

    pub(super) fn show_wallet(&mut self, v: WalletView) {
        crate::screens::wallet::page::open_on(v);
        self.show_tab(Tab::Wallet);
    }

    pub(super) fn show_market(&mut self, target: String) {
        crate::screens::market::live::open_on(target);
        self.show_tab(Tab::Market);
    }

    pub(super) fn show_settings(&mut self, id: crate::screens::settings::sections::SectionId) {
        if self.st.tab == Tab::Settings && self.legacy_page.is_some() {
            self.close_legacy();
            self.st.close();
            if let Some((_, screen, _)) = self.legacy_page.as_mut() {
                let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
                screen.command(&mut cx, "goto", id.key(), clock::secs());
            }
        } else {
            crate::screens::settings::open_section_next(id);
            self.show_tab(Tab::Settings);
        }
    }

    pub(super) fn show_friends(&mut self, target: Option<rare_friends::nav::FriendsTarget>) {
        self.close_legacy();
        self.st.close();
        self.st.set_tab(Tab::Friends, false);
        if let Some(s) = &self.social {
            s.open_friends(target);
        }
    }

    /// A press on Home's friends strip.
    pub(super) fn home_friends_act(&mut self, a: crate::screens::friends::home::HomeAct) {
        use crate::screens::friends::home::HomeAct;
        let now = clock::secs();
        let Some(social) = self.social.clone() else {
            // Fixtures: opening works, the rest needs the live service.
            match a {
                HomeAct::Open(t) => self.show_friends(Some(t)),
                _ => self.st.toast("Friends needs the live launcher.", Tone::Info, now),
            }
            return;
        };
        match a {
            HomeAct::Open(t) => self.show_friends(Some(t)),
            HomeAct::JoinParty(id) => self.friends_act(&format!("friends:join-party:{id}")),
            HomeAct::LeaveParty(id) => self.friends_act(&format!("friends:leave-party:{id}")),
            HomeAct::JoinGame(g) => {
                let a = social.join_steam_lobby_or(g);
                self.run_after(a);
            }
        }
    }

    /// A `friends:...` action from the Friends screen.
    pub(super) fn friends_act(&mut self, a: &str) {
        let now = clock::secs();
        let Some(social) = self.social.clone() else {
            self.st.toast("Friends needs the live launcher.", Tone::Info, now);
            return;
        };
        let after = social.act(a);
        self.run_after(after);
    }

    /// Does what a Friends action left for the app.
    pub(super) fn run_after(&mut self, a: After) {
        let now = clock::secs();
        match a {
            After::Nothing => {}
            After::Wallet => self.st.set_tab(Tab::Wallet, false),
            After::WalletView(v) => {
                crate::screens::wallet::page::open_on(v);
                self.st.set_tab(Tab::Wallet, false);
            }
            After::Launch(id) => self.launch(&id),
            After::Toast(t, e) => self.st.toast(t, if e { Tone::Error } else { Tone::Info }, now),
            After::Ask { tag, title, placeholder } => {
                self.st.ask_text(format!("social:{tag}"), title, "", Some(placeholder));
            }
            After::Share(addr) => {
                let label = self.social.as_ref().map(|s| s.label_of(&addr)).unwrap_or_default();
                crate::screens::share::set_send_to(Some((addr, label)));
                self.close_legacy();
                self.st.open(Some(Overlay::Other("share")));
            }
            After::SteamUrl(url) => {
                self.st.handoff_until = Some(now + 3.0);
                self.fx.spawn(move |b| {
                    let r = b.steam_url(&url);
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            log::warn!("steam lobby: {e}");
                            st.handoff_until = None;
                            st.toast("Couldn't open Steam. Try again.", Tone::Error, now);
                        }
                    })
                });
            }
        }
    }
}

pub(super) fn start(waker: Arc<dyn Fn() + Send + Sync>) -> Option<Arc<Social>> {
    social_enabled().then(|| Social::start(waker))
}

// ---- the Rare menu's Friends and Notifications tabs ----

mod menu {
    use crate::screens::overlay::tabs::friends as mf;
    use crate::screens::overlay::tabs::notices as mn;
    use rare_friends::{FriendsState, FriendsStatus, PresenceState};
    use rare_notify::types::{Notice, NoticeAction};

    fn identity(p: &rare_friends::Person) -> mf::Identity {
        mf::Identity { address: p.address.clone(), name: p.name.clone(), username: p.username.clone(), suins: p.suins.clone() }
    }

    fn game(g: &Option<rare_friends::GameRef>) -> Option<mf::GameRef> {
        g.as_ref().map(|g| mf::GameRef { id: g.id.clone(), title: g.title.clone() })
    }

    fn presence(p: &rare_friends::Presence) -> mf::Presence {
        mf::Presence {
            state: match p.state {
                PresenceState::Online => mf::PState::Online,
                PresenceState::Away => mf::PState::Away,
                PresenceState::Offline => mf::PState::Offline,
            },
            game: game(&p.game),
            since: p.since,
            last_seen: p.last_seen,
        }
    }

    pub fn friends_view(st: &FriendsState) -> mf::FriendsView {
        mf::FriendsView {
            status: match st.status {
                FriendsStatus::None => mf::Status::None,
                FriendsStatus::Watch => mf::Status::Watch,
                FriendsStatus::Unlinked => mf::Status::Unlinked,
                FriendsStatus::Linking => mf::Status::Linking,
                FriendsStatus::Loading => mf::Status::Loading,
                FriendsStatus::Ready => mf::Status::Ready,
                FriendsStatus::Error => mf::Status::Error,
            },
            error: st.error.clone(),
            address: st.address.clone().unwrap_or_default(),
            friends: st.friends.iter().map(|f| mf::Friend { who: identity(&f.person), avatar: f.person.avatar_url.clone(), presence: presence(&f.presence) }).collect(),
            received: st.received.iter().map(|p| identity(&p.person)).collect(),
            convos: st
                .convos
                .iter()
                .filter_map(|c| {
                    let f = st.friends.iter().find(|f| f.address == c.address)?;
                    let me = st.address.as_deref().unwrap_or_default();
                    let lines = match st.dms.get(&c.address).filter(|v| !v.is_empty()) {
                        Some(v) => v.iter().rev().take(2).rev().map(|m| (m.from == me, crate::screens::friends::profile::message_text(m))).collect(),
                        None => vec![(c.last.from == me, crate::screens::friends::profile::message_text(&c.last))],
                    };
                    Some(mf::Convo { who: identity(&f.person), avatar: f.person.avatar_url.clone(), lines, unread: c.unread })
                })
                .collect(),
            parties: st
                .parties
                .iter()
                .map(|p| mf::Party {
                    id: p.id.clone(),
                    host: p.host.clone(),
                    game: game(&p.game),
                    members: p.members.iter().map(|m| mf::PartyMember { who: identity(&m.person), state: if m.state == rare_friends::MemberState::Joined { mf::MState::Joined } else { mf::MState::Invited } }).collect(),
                })
                .collect(),
        }
    }

    fn action(n: &Notice) -> mn::Action {
        match &n.action {
            NoticeAction::Item { item_id, .. } => mn::Action::Item { item_id: item_id.clone() },
            NoticeAction::Collection { .. } => mn::Action::Collection,
            NoticeAction::Position { .. } => mn::Action::Position,
            NoticeAction::Coin { .. } => mn::Action::Coin,
            NoticeAction::Tx { .. } | NoticeAction::Unknown => mn::Action::Tx,
            NoticeAction::Trader { .. } => mn::Action::Trader,
            NoticeAction::Friends { .. } => mn::Action::Friends,
            NoticeAction::Party { .. } => mn::Action::Party,
            NoticeAction::OsUpdate => mn::Action::OsUpdate,
            NoticeAction::Downloads => mn::Action::Downloads,
            NoticeAction::Emulators => mn::Action::Emulators,
            NoticeAction::Trophies => mn::Action::Trophies,
            NoticeAction::Battery => mn::Action::Battery,
        }
    }

    pub fn notice_store(s: &rare_notify::NotifyStore) -> mn::NoticeStore {
        mn::NoticeStore {
            notices: s
                .notices
                .iter()
                .map(|n| mn::Notice {
                    id: n.id.clone(),
                    kind: n.kind.as_str().to_string(),
                    title: n.title.clone(),
                    body: n.body.clone(),
                    art_label: n.item.as_ref().map(|i| i.name.clone()).filter(|x| !x.is_empty()).or_else(|| n.amount.as_ref().map(|a| a.symbol.clone()).filter(|x| !x.is_empty())).unwrap_or_else(|| n.title.clone()),
                    action: action(n),
                    item_id: n.item.as_ref().map(|i| i.id.clone()),
                    cp_ts: n.cp_ts,
                    created_at: n.created_at,
                })
                .collect(),
            read: s.read.clone(),
            read_at: s.read_at,
            cleared_at: s.cleared_at,
            prefs: mn::Prefs { off: s.prefs.kinds.iter().filter(|(_, on)| !**on).map(|(k, _)| k.clone()).collect() },
        }
    }
}

impl App {
    /// Puts the live friends and notices where the Rare menu reads them. Returns (unread, an
    /// invite toast is up).
    pub(super) fn menu_feed(&mut self) -> (usize, bool) {
        use crate::screens::overlay::tabs;
        let Some(social) = self.social.clone() else { return (0, false) };
        tabs::set_friends(menu::friends_view(&social.friends().snapshot()));
        let (store, unread, invite) = social.with_store(|s| (menu::notice_store(s), s.unread(), s.toasts.last().is_some_and(|t| t.notice.kind.as_str() == "party_invite")));
        tabs::set_notices(store);
        tabs::set_now_ms(wall_ms() as i64);
        (unread, invite)
    }

    /// What a Rare menu row of Friends or Notifications does. True when it was one of those.
    pub(super) fn menu_act(&mut self, a: &str) -> bool {
        use crate::screens::overlay::tabs::{parse_act, Effect};
        let Some(e) = parse_act(a) else { return false };
        let Some(social) = self.social.clone() else { return false };
        let friends = social.friends().clone();
        match e {
            Effect::OpenNotice(id) => {
                let n = social.with_store(|s| {
                    s.mark_read(&id);
                    s.notices.iter().find(|n| n.id == id).cloned()
                });
                if let Some(n) = n {
                    self.open_notice(&n);
                }
            }
            Effect::MarkAllRead => social.with_store(|s| s.mark_all_read(wall_ms() as i64)),
            Effect::JoinGame(addr) => self.friends_act(&format!("friends:join-game:{addr}")),
            Effect::OpenFriend(addr) => self.show_friends(Some(rare_friends::nav::FriendsTarget::Friend { address: addr })),
            Effect::Reply(addr) => {
                social.mark_read(&addr);
                let name = social.label_of(&addr);
                self.st.ask_text(format!("social:dm:{addr}"), format!("Message to {name}"), "", Some("Up to 280 characters"));
            }
            Effect::OpenRequests => {
                let first = friends.read(|s| s.received.first().map(|p| p.address.clone()));
                self.show_friends(first.map(|address| rare_friends::nav::FriendsTarget::Friend { address }));
            }
            Effect::TurnOn => self.friends_act("friends:turn-on"),
            Effect::JoinParty => {
                let id = friends.read(|s| s.party_invites().first().map(|p| p.id.clone()));
                match id {
                    Some(id) => self.friends_act(&format!("friends:join-party:{id}")),
                    None => self.show_friends(None),
                }
            }
            Effect::LeaveParty => {
                if let Some(id) = friends.read(|s| s.active_party().map(|p| p.id.clone())) {
                    self.friends_act(&format!("friends:leave-party:{id}"));
                }
            }
            Effect::InviteFriends | Effect::StartParty => {
                let id = friends.read(|s| s.active_party().map(|p| p.id.clone()));
                self.show_friends(id.map(|id| rare_friends::nav::FriendsTarget::Party { id }));
            }
            _ => return false,
        }
        true
    }
}
