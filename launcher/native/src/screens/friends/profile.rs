//! A friend's messages and profile under their pane, the "Recently played with" pane, and the
//! picker that sends a shared capture to a friend.
//!
//! Messages: the direct messages with this friend (kept 30 days on rare.market), newest at the
//! bottom, in a box of fixed height so nothing under it moves while they load. Profile: what
//! rare.market's /u/<address> shows (games, trophies by game, items held).

use super::bits::*;
use super::model::{FriendProfile, FriendsData};
use super::screen::{hair_top, T_DM_PANEL, T_PROFILE, T_RECENT, T_SEND_CANCEL, T_SEND_PICK};
use crate::kit::art::{art, ArtFit};
use crate::kit::{button, stat_line, Btn, StatItem};
use crate::screens::market::host::*;
use rare_friends::types::{DirectMessage, Friend, PlayedWith};
use rare_render::view::{key, El};

/// The last messages drawn in a conversation (the box shows about six).
pub const SHOWN: usize = 40;
/// Scrolled all the way back, the box ends at the first message (one always fits the box).
pub const TOP_KEEP: usize = 1;

/// An unread count as a badge shows it.
pub fn count_text(n: u32) -> String {
    if n > 99 {
        "99+".into()
    } else {
        n.to_string()
    }
}

/// How long ago, in words ("today", "yesterday", "3 days ago", else the date).
pub fn when(at: i64, now: i64) -> String {
    let days = (now - at).max(0) / 86_400_000;
    match days {
        0 => "today".into(),
        1 => "yesterday".into(),
        2..=13 => format!("{days} days ago"),
        _ => date_short(at),
    }
}

/// One message's words: the text, or what was sent when it is only a capture.
pub fn message_text(m: &DirectMessage) -> String {
    match (&m.attach, m.text.trim()) {
        (Some(a), "") => format!("Sent a {}: {}", if a.kind == "clip" { "clip" } else { "screenshot" }, a.url.trim_start_matches("https://")),
        (Some(a), t) => format!("{t} {}", a.url.trim_start_matches("https://")),
        (None, t) => t.to_string(),
    }
}

/// The conversation with `f`.
/// `back`: how many of the newest messages are scrolled past (d-pad up on the box).
pub fn messages_panel(ui: &mut Ui, d: &FriendsData, f: &Friend, back: usize) -> El {
    let me = d.st.address.as_deref().unwrap_or_default();
    let head = baseline_row(
        ui,
        s().mb(0.6).gap(0.75),
        vec![(T::new(Ty::Body).w(600), "Messages".into()), (T::new(Ty::Meta).c(tok::MUTED), match (back, d.st.dms.get(&f.address).map(|v| v.len()).unwrap_or(0)) {
            (0, n) if n > 5 => "Kept for 30 days. Up for older".into(),
            (0, _) => "Kept for 30 days".into(),
            _ => "Older messages. Down for newer".into(),
        })],
    );
    let msgs = d.st.dms.get(&f.address);
    // The newest at the bottom: the box is bottom-aligned and clipped, so older lines leave at the top.
    let mut list = ui.bx(s().col().gap(0.7).h(12.0).justify_end()).clip();
    let all = msgs.map(|v| v.as_slice()).unwrap_or_default();
    let end = all.len().saturating_sub(back).max(all.len().min(1));
    // Always bottom-aligned, one message per press: every message passes through view in order,
    // and the start line sits just over the first one.
    let at_start = back > 0 && end <= SHOWN && d.st.dm_full.contains(&f.address);
    if at_start && !all.is_empty() {
        list = list.child(ui.text("Start of your messages", T::new(Ty::Label).c(tok::MUTED)));
    }
    for m in &all[end.saturating_sub(SHOWN)..end] {
        let mine = m.from == me;
        let name = if mine { "You".to_string() } else { label(Some(f)) };
        let av = if mine {
            match d.st.person_of(me) {
                Some(p) => friend_avatar(ui, s(), &p.clone(), 1.75),
                None => wallet_avatar(ui, s(), me, 1.75),
            }
        } else {
            friend_avatar(ui, s(), f, 1.75)
        };
        let line = baseline_row(ui, s().gap(0.5), vec![(T::new(Ty::Small).w(600), name), (T::new(Ty::Label).c(tok::MUTED), time_of(m.at))]);
        let words = ui.bx(s().col().shrink().min0()).child(line).child(ui.text(message_text(m), T::new(Ty::Small).c(tok::fg(0.9)).wrap()));
        list = list.child(ui.bx(s().row().items_start().gap(0.7)).child(av).child(words).key(key(&m.id)));
    }
    if all.is_empty() {
        let loading = msgs.is_none() && d.st.convos.iter().any(|c| c.address == f.address);
        let t = if loading { "Loading messages" } else { "No messages yet. Write one, or send a screenshot or clip." };
        list = ui.bx(s().col().h(12.0)).child(ui.text(t, T::new(Ty::Small).c(tok::MUTED)));
    }
    let panel = crate::kit::panel(ui, s().col().px(1.6).py(1.2)).child(head).child(list);
    fill_nav(panel, T_DM_PANEL, Some("Write a message"), Some("Remove or block"), false)
}

/// What rare.market shows of `f`: counts, played together, games, trophies and items.
pub fn profile_panel(ui: &mut Ui, d: &FriendsData, f: &Friend) -> El {
    let el = profile_body(ui, d, f);
    fill_nav(el, T_PROFILE, None, None, false)
}

fn profile_body(ui: &mut Ui, d: &FriendsData, f: &Friend) -> El {
    // ("*": a fixture's profile shown on every friend)
    let prof: Option<&FriendProfile> = d.profile.as_ref().filter(|(a, _)| *a == f.address || a == "*").and_then(|(_, p)| p.as_ref());
    let mut col = crate::kit::panel(ui, s().col().gap(1.1).px(1.6).py(1.3).min_h(14.0)).child(banner(ui, f, prof)).child(ui.text("Profile", T::new(Ty::Body).w(600)));
    if let Some(pw) = d.st.played_with.iter().find(|p| p.address == f.address) {
        col = col.child(ui.text(format!("You played {} together {}", pw.game.title, when(pw.at, d.now_ms)), T::new(Ty::Small).c(tok::fg(0.85))));
    }
    let Some(p) = prof else {
        return col.child(ui.text("Loading their games, trophies and items", T::new(Ty::Small).c(tok::MUTED)));
    };
    let items = p.items.as_deref().unwrap_or_default();
    let stats = vec![
        StatItem { label: "Games".into(), value: p.counts.games.to_string() },
        StatItem { label: "Trophies".into(), value: p.counts.trophies.to_string() },
        StatItem { label: "Points".into(), value: p.counts.trophy_points.to_string() },
        StatItem { label: "Items".into(), value: if items.len() >= 24 { "24+".into() } else { items.len().to_string() } },
    ];
    col = col.maybe(stat_line(ui, s(), &stats, true, false));
    if !p.games.is_empty() {
        let mut g = ui.bx(s().col().gap(0.35).pt(1.0)).child(hair_top(ui)).child(ui.text("Games", T::new(Ty::Label).w(500).c(tok::MUTED)));
        for x in p.games.iter().take(6) {
            let mut runs = vec![(T::new(Ty::Small).w(500).trunc(), x.name.clone())];
            if x.trophies > 0 {
                runs.push((T::new(Ty::Meta).c(tok::MUTED), format!("{} trophies", x.trophies)));
            }
            g = g.child(baseline_row(ui, s().gap(0.6).min0(), runs).key(key(&x.name)));
        }
        col = col.child(g);
    }
    let trophies: Vec<(&str, &str, &str)> = p.trophies.iter().flat_map(|g| g.data.iter().map(move |t| (t.name.as_str(), t.tier.as_str(), g.game_name.as_str()))).take(6).collect();
    if !trophies.is_empty() {
        let mut t = ui.bx(s().col().gap(0.35).pt(1.0)).child(hair_top(ui)).child(ui.text("Rarest trophies", T::new(Ty::Label).w(500).c(tok::MUTED)));
        for (i, (name, tier, game)) in trophies.into_iter().enumerate() {
            let runs = vec![(T::new(Ty::Small).w(500).trunc(), name.to_string()), (T::new(Ty::Meta).c(tok::MUTED).trunc(), format!("{}, {game}", cap(tier)))];
            t = t.child(baseline_row(ui, s().gap(0.6).min0(), runs).key(key(&(i, name))));
        }
        col = col.child(t);
    }
    if !items.is_empty() {
        let size = 4.0;
        let mut row = ui.bx(s().row().wrap().gap(0.6));
        for it in items.iter().take(12) {
            let chain: Vec<String> = it.image_url.iter().cloned().collect();
            let fb = ui
                .bx(s().fill().row().items_center().justify_center())
                .bg(tok::L3)
                .child(ui.text(crate::kit::market::initial(&it.name), T::new(Ty::Small).w(600).c(tok::MUTED.alpha(0.6))));
            let pic = art(ui, s().fill(), &chain, size, size, ArtFit::Cover(0.5), Some(fb));
            row = row.child(ui.bx(s().size(size)).bg(tok::L3).radius(ui.r(0.4)).border(1.0, tok::HAIR).clip().child(pic).key(key(&it.id)));
        }
        col = col.child(ui.bx(s().col().gap(0.5).pt(1.0)).child(hair_top(ui)).child(ui.text("Items", T::new(Ty::Label).w(500).c(tok::MUTED))).child(row));
    }
    if p.games.is_empty() && p.trophies.is_empty() && items.is_empty() {
        col = col.child(ui.text("No games, trophies or items on rare.market yet.", T::new(Ty::Small).c(tok::MUTED)));
    }
    col
}

/// Their banner: the middle band of the 3:1 picture at a fixed height, so nothing moves while it
/// loads (the flat colour of their default meanwhile; the picture only once rare.market answered).
pub fn banner(ui: &mut Ui, f: &Friend, prof: Option<&FriendProfile>) -> El {
    let src = prof.and_then(|p| p.banner.as_ref()).map(|b| b.url.clone()).filter(|u| !u.is_empty());
    let rgb = crate::banners::bg_of(src.as_deref(), &f.address);
    let bg = rare_render::Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    let mut b = ui.bx(s().wp(1.0).h(BANNER_H)).bg(bg).radius(ui.r(0.6)).clip();
    if let Some(u) = src {
        let fb = ui.bx(s().fill()).bg(bg);
        b = b.child(art(ui, s().fill(), &[u], BANNER_H * 3.0, BANNER_H, ArtFit::Cover(0.5), Some(fb)));
    }
    b.key(key("banner"))
}

/// The friend profile banner's height (rem).
pub const BANNER_H: f32 = 7.5;

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Friends from "Recently played with" that are still friends, newest first.
pub fn recent_list(d: &FriendsData) -> Vec<(&PlayedWith, &Friend)> {
    d.st.played_with.iter().filter_map(|p| d.st.friends.iter().find(|f| f.address == p.address).map(|f| (p, f))).collect()
}

pub fn recent_pane(ui: &mut Ui, d: &FriendsData) -> El {
    let mut panel = crate::kit::panel(ui, s().col()).child(ui.text_s(
        s().px(1.4).pb(0.4).pt(1.2),
        "Friends who were in the same game as you, last 90 days.",
        T::new(Ty::Small).c(tok::MUTED).wrap(),
    ));
    for (i, (p, f)) in recent_list(d).into_iter().enumerate() {
        let av = friend_avatar(ui, s(), f, 2.25);
        let words = ui
            .bx(s().col().grow())
            .child(ui.text(label(Some(f)), T::new(Ty::Body).w(500).trunc()))
            .child(ui.text(format!("{}, {}", p.game.title, when(p.at, d.now_ms)), T::new(Ty::Meta).c(tok::MUTED).trunc()));
        let row = ui.bx(s().row().items_center().min_h(4.25).wp(1.0).gap(1.0).px(1.4).py(0.6)).child(hair_top(ui)).child(av).child(words);
        panel = panel.child(fill_nav(row, T_RECENT + i as u32, Some("Open"), None, false).key(key(&f.address)));
    }
    panel
}

/// Pick the friend a shared capture goes to.
pub fn send_picker(ui: &mut Ui, d: &FriendsData, kind: &str) -> El {
    let head = baseline_row(
        ui,
        s().mb(0.9).gap(0.75),
        vec![
            (T::new(Ty::Row).w(600), format!("Send your {} to a friend", if kind == "clip" { "clip" } else { "screenshot" })),
            (T::new(Ty::Small).c(tok::MUTED), "It goes to your messages with them".into()),
        ],
    );
    let mut panel = crate::kit::panel(ui, s().col());
    for (i, f) in d.st.friends.iter().enumerate() {
        let av = friend_avatar(ui, s(), f, 2.25);
        let pres = presence_line(ui, s(), &f.presence, false, d.now_ms, Ty::Meta);
        let words = ui.bx(s().col().grow()).child(ui.text(label(Some(f)), T::new(Ty::Body).w(500).trunc())).child(pres);
        let mut row = ui.bx(s().row().items_center().min_h(4.0).wp(1.0).gap(1.0).px(1.4).py(0.5));
        if i > 0 {
            row = row.child(hair_top(ui));
        }
        panel = panel.child(fill_nav(row.child(av).child(words), T_SEND_PICK + i as u32, Some("Send"), None, false).key(key(&f.address)));
    }
    if d.st.friends.is_empty() {
        panel = panel.child(ui.text_s(s().px(1.4).py(1.2), "No friends yet. Add one first.", T::new(Ty::Small).c(tok::MUTED)));
    }
    let cancel = button(ui, Btn::new("Cancel", T_SEND_CANCEL).ghost());
    ui.bx(s().col()).child(head).child(panel).child(ui.bx(s().row().mt(1.0).gap(0.75)).child(cancel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_friends::types::Attach;

    #[test]
    fn words_for_messages_and_dates() {
        let m = DirectMessage { id: "1".into(), from: "0x1".into(), to: "0x2".into(), text: String::new(), attach: Some(Attach { kind: "clip".into(), url: "https://rare.market/s/abc".into() }), at: 0, read: false };
        assert_eq!(message_text(&m), "Sent a clip: rare.market/s/abc");
        let m = DirectMessage { text: "gg".into(), attach: None, ..m };
        assert_eq!(message_text(&m), "gg");
        let day = 86_400_000;
        assert_eq!(when(10 * day, 10 * day + 5), "today");
        assert_eq!(when(10 * day, 11 * day + 5), "yesterday");
        assert_eq!(when(10 * day, 13 * day), "3 days ago");
    }
}
