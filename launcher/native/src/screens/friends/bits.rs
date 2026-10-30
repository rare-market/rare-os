//! Small friends pieces (friends/ui/bits.tsx): a player's picture (their NFT, else the wallet gem
//! pattern), the presence line (a Dot and muted text, never a pill), a friend's game art, plus the
//! dates and the QR code the panes print.

use super::model::FriendsData;
use crate::kit::art::{art, ArtFit};
use crate::kit::icon::{svg_box, svg_tinted};
use crate::kit::{dot, tok, DotTone};
use crate::screens::market::host::{s, Ty, Ui, S, T};
use rare_friends::format::presence_text;
use rare_friends::types::{GameKind, GameRef, Person, Presence, PresenceState};
use rare_render::view::El;

/// `FriendAvatar`: the player's picture (a Rare default, drawn here, or their NFT through
/// rare.market's image chain, MImg 120 px), else their default. `size` rem.
pub fn friend_avatar(ui: &mut Ui, st: S, p: &Person, size: f32) -> El {
    let r = ui.r(size / 2.0);
    // No picture from rare.market (an older answer): the account's default, as rare.market picks it.
    let starter = crate::avatars::url(crate::avatars::starter(&p.address));
    let url = p.avatar_url.as_deref().filter(|u| !u.is_empty()).unwrap_or(&starter);
    // Resolved to rare.market's chain at the drawn size by kit::art.
    let chain = vec![url.to_string()];
    let label = p.name.clone().unwrap_or_else(|| p.address.clone());
    // MImg's last resort: the initial, text-[1.6rem] semibold secondary/60 on layer 03.
    let fb = ui
        .bx(s().fill().row().items_center().justify_center())
        .bg(tok::L3)
        .child(ui.text(
            crate::kit::market::initial(&label),
            T::new(Ty::px(1.6)).w(600).c(tok::MUTED.alpha(0.6)),
        ));
    let img = art(
        ui,
        s().fill(),
        &chain,
        size,
        size,
        ArtFit::Cover(0.5),
        Some(fb),
    );
    ui.bx(st.size(size)).bg(tok::L3).radius(r).clip().child(img)
}

/// `WalletAvatar`: the generated gem pattern for an address, round.
pub fn wallet_avatar(ui: &mut Ui, st: S, address: &str, size: f32) -> El {
    let svg = rare_market::identicon::avatar_svg(address);
    let name = format!("gem-{}", &address[..address.len().min(18)]);
    let gem = svg_box(
        ui,
        &name,
        svg,
        size,
        size,
        s().abs(Some(0.0), None, None, Some(0.0)),
    );
    ui.bx(st.size(size))
        .radius(ui.r(size / 2.0))
        .clip()
        .child(gem)
}

/// `PresenceLine`: a dot (white online, grey away, none offline) and the words, `ty` sized.
pub fn presence_line(ui: &mut Ui, st: S, p: &Presence, long: bool, now: i64, ty: Ty) -> El {
    let mut row = ui.bx(st.row().items_center().gap(0.45).min0());
    if p.state != PresenceState::Offline {
        row = row.child(dot(
            ui,
            s(),
            if p.state == PresenceState::Online {
                DotTone::Primary
            } else {
                DotTone::Muted
            },
            false,
        ));
    }
    let bright = p.state == PresenceState::Online && p.game.is_some();
    row.child(
        ui.text(
            presence_text(p, long, now),
            T::new(ty)
                .trunc()
                .c(if bright { tok::fg(0.85) } else { tok::MUTED }),
        ),
    )
}

/// Cover art for a friend's game: this device's own art when it has the game, Steam's for Steam
/// games, the bundled art for built-in web games (bits.tsx `gameArt`).
pub fn game_art_sources(d: &FriendsData, g: &GameRef, header: bool) -> Vec<String> {
    if let Some(own) = d.library.iter().find(|x| x.id == g.id) {
        return if !header {
            own.cover.clone()
        } else if !own.header.is_empty() {
            own.header.clone()
        } else {
            own.hero.clone()
        };
    }
    if let (GameKind::Steam, Some(appid)) = (g.kind, g.appid) {
        let base = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}");
        return if header {
            vec![format!("{base}/header.jpg")]
        } else {
            vec![
                format!("{base}/library_600x900.jpg"),
                format!("{base}/library_600x900_2x.jpg"),
            ]
        };
    }
    // lib/builtins.ts BUILTIN_WEB_GAMES
    let builtin = match g.id.as_str() {
        "web:act" => Some(("games/act/cover.jpg", "games/act/hero.jpg")),
        _ => None,
    };
    match builtin {
        Some((cover, hero)) => vec![if header { hero } else { cover }.to_string()],
        None => Vec::new(),
    }
}

/// `GameArt`: the art filling `st` (sized `w` x `h` rem), else the title on layer 03 (`Generated`).
pub fn game_art(
    ui: &mut Ui,
    st: S,
    d: &FriendsData,
    g: &GameRef,
    header: bool,
    w: f32,
    h: f32,
) -> El {
    let sources = game_art_sources(d, g, header);
    let fb = crate::kit::title_cover(ui, &g.title, w, h, if header { 0.95 } else { 1.05 });
    art(ui, st, &sources, w, h, ArtFit::Cover(0.5), Some(fb))
}

/// `shortAddress`: 0xa049...887e
pub fn short_address(a: &str) -> String {
    rare_friends::display::short_addr(a)
}

/// The store's `label`: username first, else SuiNS name, else a short address ("Someone").
pub fn label(p: Option<&Person>) -> String {
    rare_friends::display::label(p)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// (year, month 1-12, day) of a UTC time in ms (the launcher shows dates in the device's zone; the
/// goldens run in UTC).
pub fn ymd(ms: i64) -> (i64, u32, u32) {
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// toLocaleDateString({ month: "short", day: "numeric", year: "numeric" }): "Oct 7, 2026".
pub fn date_long(ms: i64) -> String {
    let (y, m, d) = ymd(ms);
    format!("{} {d}, {y}", MONTHS[m as usize - 1])
}

/// toLocaleDateString({ month: "short", day: "numeric" }): "Oct 25".
pub fn date_short(ms: i64) -> String {
    let (_, m, d) = ymd(ms);
    format!("{} {d}", MONTHS[m as usize - 1])
}

/// toLocaleTimeString({ hour: "numeric", minute: "2-digit" }): "6:41 PM".
pub fn time_of(ms: i64) -> String {
    let mins = ms.div_euclid(60_000).rem_euclid(24 * 60);
    let (h, m) = (mins / 60, mins % 60);
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    format!("{h12}:{m:02} {}", if h < 12 { "AM" } else { "PM" })
}

/// BuySheet's `Qr`: margin 1, error correction M, #100C18 on white, in a white rounded-md box with
/// 0.5rem padding, `size` rem square.
pub fn qr(ui: &mut Ui, st: S, text: &str, size: f32) -> El {
    let Ok(code) = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M)
    else {
        return ui.bx(st.size(size)).bg(tok::L3).radius(ui.r(0.375));
    };
    let n = code.width();
    let colors = code.to_colors();
    let dim = n + 2;
    let mut path = String::new();
    for y in 0..n {
        for x in 0..n {
            if colors[y * n + x] == qrcode::Color::Dark {
                path.push_str(&format!("M{} {}h1v1h-1z", x + 1, y + 1));
            }
        }
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {dim} {dim}" shape-rendering="crispEdges"><path fill="#ffffff" d="M0 0h{dim}v{dim}H0z"/><path fill="#100C18" d="{path}"/></svg>"##
    );
    let inner = size - 1.0;
    let img = svg_box(ui, &format!("qr-{text}"), svg, inner, inner, s());
    ui.bx(st.size(size).p(0.5))
        .bg(tok::WHITE)
        .radius(ui.r(0.375))
        .clip()
        .child(img)
}

/// A lucide icon the kit's set does not carry yet (same wrapper as `kit::icon::lucide_svg`).
pub fn lucide(ui: &mut Ui, name: &str, size: f32, color: rare_render::Color) -> El {
    let body = match name {
        "user-plus" => {
            r#"<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><line x1="19" x2="19" y1="8" y2="14"/><line x1="22" x2="16" y1="11" y2="11"/>"#
        }
        "shield" => {
            r#"<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/>"#
        }
        "chevron-up" => r#"<path d="m18 15-6-6-6 6"/>"#,
        other => return crate::kit::icon(ui, other, size, color),
    };
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#
    );
    svg_tinted(ui, &format!("lucide-{name}"), svg, size, size, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_times_read_like_the_web_in_utc() {
        // 2026-10-27 18:47:02 UTC
        let t = 1_793_126_822_000;
        assert_eq!(ymd(t), (2026, 10, 27));
        assert_eq!(date_long(t - 20 * 86_400_000), "Oct 7, 2026");
        assert_eq!(date_short(t - 2 * 86_400_000), "Oct 25");
        assert_eq!(time_of(t - 6 * 60_000), "6:41 PM");
        assert_eq!(time_of(0), "12:00 AM");
        assert_eq!(time_of(12 * 3_600_000 + 5 * 60_000), "12:05 PM");
    }

    #[test]
    fn game_art_follows_bits_tsx() {
        let d = FriendsData::default();
        assert_eq!(
            game_art_sources(
                &d,
                &GameRef::steam("steam:1145360", "Hades II", 1145360),
                false
            )[0],
            "https://cdn.cloudflare.steamstatic.com/steam/apps/1145360/library_600x900.jpg"
        );
        assert_eq!(
            game_art_sources(
                &d,
                &GameRef::web("web:act", "ACT", "https://play.actuniverse.com/"),
                false
            ),
            vec!["games/act/cover.jpg".to_string()]
        );
        assert_eq!(
            game_art_sources(&d, &GameRef::web("web:act", "ACT", "u"), true),
            vec!["games/act/hero.jpg".to_string()]
        );
        assert!(game_art_sources(&d, &GameRef::other("x", "X"), false).is_empty());
    }
}

/// Truncating text gets 1 px of slack on the right: the scene re-lays truncating text at its
/// rounded box width, which can be a fraction under the measured width and would add an ellipsis
/// to text that fits (engine request filed). Applied to a whole view tree.
pub fn trunc_slack(mut el: El) -> El {
    if let rare_render::view::Content::Text { style, .. } = &el.content {
        if style.truncate {
            let pr = match el.style.padding.right {
                l if l == rare_render::taffy::prelude::length(0.0) => 1.0,
                _ => 0.0,
            };
            if pr > 0.0 {
                el.style.padding.right = rare_render::taffy::prelude::length(pr);
            }
        }
    }
    el.kids = std::mem::take(&mut el.kids)
        .into_iter()
        .map(trunc_slack)
        .collect();
    el
}
