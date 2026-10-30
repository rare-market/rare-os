//! The shell around every screen (App.tsx, components/Chrome.tsx, Backdrop.tsx), as views: the top
//! bar (brand, LB, the five tabs, RB, account, Wi-Fi, battery, clock), the bottom bar (rare.market's
//! status bar on the left, controller hints on the right), the toasts under the top bar and the art
//! behind Home. The app mounts each under its own host (app.rs), plus the Options sheet, the boot
//! splash and Steam's hand-off card from the modules below.

pub mod boot;
pub mod handoff;
pub mod options;
pub mod placeholder;

use crate::kit::art::ArtFit;
use crate::kit::{self, s, tok, DotTone, GSize, NumKind, NumOptions, Pb, TabItem, Ty, Ui, T};
use crate::state::{State, Tone, TABS};
use rare_render::focus::hints::Hint;
use rare_render::scene::{GradDir, Gradient, Shadow};
use rare_render::view::{key, El, Motion};

/// "8:05 PM"; RR_CLOCK fixes it (golden images).
pub fn clock_text() -> String {
    std::env::var("RR_CLOCK").unwrap_or_else(|_| {
        // A time zone changed in Settings (or by timedated) shows at once.
        rare_system::support::reload_zone();
        rare_render::platform::clock_text()
    })
}

/// The top bar: header h-[4.25rem] items-center gap-[1.5rem] px-[2.5rem].
pub fn top_bar(ui: &mut Ui, st: &State, clock: &str) -> El {
    let mw = 1.7 * 235.0 / 256.0;
    let mark = kit::art(ui, s().wh(mw, 1.7), &["brand/mark.png".to_string()], mw, 1.7, ArtFit::Contain(0.5, 0.5), None);
    let brand = ui.bx(s().row().items_center().gap(0.6)).child(mark).child(ui.text("rare", T::new(Ty::px(1.35)).w(600).track(-0.02)));
    // Friends gets a dot for unread messages, requests and party invites.
    let news = st.friends.as_ref().is_some_and(|f| f.st.badge() > 0);
    let items: Vec<TabItem> = TABS.iter().map(|(t, l)| TabItem::new(*l).dot(*t == crate::state::Tab::Friends && news)).collect();
    let lb = kit::glyph(ui, Pb::Lb, GSize::Sm);
    let rb = kit::glyph(ui, Pb::Rb, GSize::Sm);
    let tabs = top_tabs(ui, &items, st.tab.index().unwrap_or(usize::MAX));
    // Setup's account pages (the Wallet tab's create and sign-in pages) are still setup: no tabs.
    let nav = if crate::features::firstrun::wallet_from_setup() { ui.bx(s()) } else { ui.bx(s().row().items_center().gap(1.0).ml(0.5).hp(1.0)).child(lb).child(tabs).child(rb).name("topbar-tabs") };
    // text-fg/85 on the right group: the icons draw in it.
    let fg85 = tok::fg(0.85);
    let mut dev = ui.bx(s().row().items_center().gap(0.9));
    // Remote access (SSH) is on.
    if st.device.as_ref().is_some_and(|d| d.ssh) {
        dev = dev.child(kit::icon(ui, "terminal", 1.15, fg85).name("ssh-on"));
    }
    // A tool on the network is sending controller input.
    if st.remote_input {
        dev = dev.child(kit::icon(ui, "gamepad-2", 1.15, fg85).name("remote-input"));
    }
    match &st.device {
        Some(d) if d.wifi.is_none() => dev = dev.child(ui.text("Offline", T::new(Ty::Small).c(tok::MUTED))),
        _ => dev = dev.child(kit::icon(ui, "wifi", 1.15, fg85)),
    }
    if let Some(b) = st.device.as_ref().and_then(|d| d.battery.as_ref()) {
        let ic = kit::icon(ui, kit::icon::battery_name(b.percent, b.charging), 1.35, fg85);
        dev = dev.child(ui.bx(s().row().items_center().gap(0.35)).child(ic).child(ui.text(format!("{}%", b.percent), T::new(Ty::Small).tab().c(fg85))));
    }
    // The tabs never give way (nodes do not shrink by default): this group does, the profile
    // chip's name truncating first.
    let mut right = ui.bx(s().row().items_center().gap(1.25).ml_auto().shrink().min0());
    if let Some(dl) = downloads_chip(ui, st) {
        right = right.child(dl);
    }
    if let Some(chip) = profile_chip(ui, st) {
        right = right.child(chip);
    }
    let right = right.child(dev).child(ui.text(clock, T::new(Ty::Body).w(500).tab())).name("topbar-right");
    ui.bx(s().row().items_center().gap(1.5).h(4.25).px(2.5).wp(1.0)).child(brand).child(nav).child(right)
}

/// The top tabs: kit::tabs' look in a plain row that never scrolls or shrinks, so all six
/// always show whole (the right side gives way instead).
fn top_tabs(ui: &mut Ui, items: &[TabItem], value: usize) -> El {
    let tr = rare_render::view::Transition { dur: 0.2, ease: rare_render::anim::Ease::STANDARD, color_ease: rare_render::anim::Ease::STANDARD };
    let mut row = ui.bx(s().row().items_stretch().gap(1.25).hp(1.0));
    for (i, it) in items.iter().enumerate() {
        let on = i == value;
        let mut span = ui.bx(s().row().items_center()).child(ui.text(it.label.clone(), T::new(Ty::Body).w(500).c(if on { tok::FG } else { tok::MUTED })).transition(tr));
        if it.dot {
            span = span.child(ui.bx(s().abs(Some(-0.1), Some(-0.55), None, None).size(0.4)).bg(tok::FG).radius(ui.r(0.2)));
        }
        span = span.child(ui.bx(s().abs(None, Some(0.0), Some(-0.3), Some(0.0)).h_px(2.0)).bg(tok::FG).radius(1.0).opacity(if on { 1.0 } else { 0.0 }));
        row = row.child(ui.bx(s().row().items_center()).child(span).key(key(&it.label)));
    }
    row
}

/// The Downloads chip's nav tag: A on it opens Downloads (app.rs).
pub const DL_TAG: u32 = 0xFFFF_FF02;

/// While Steam downloads anything: a download icon and the running one's progress (or how many
/// wait), focusable (Up from the page's top row) and A opens Downloads. Nothing otherwise.
fn downloads_chip(ui: &mut Ui, st: &State) -> Option<El> {
    let items = &st.feat.downloads.snap.items;
    if items.is_empty() {
        return None;
    }
    let run = items.iter().find(|d| d.active && !d.paused);
    let text = match run {
        Some(d) => format!("{}%", (crate::features::downloads::fraction(d) * 100.0).floor() as u32),
        None if items.len() == 1 => "Paused".to_string(),
        None => format!("{} paused", items.len()),
    };
    let fg85 = tok::fg(0.85);
    let chip = ui.bx(s().row().items_center().gap(0.4).px(0.6).py(0.3)).radius(ui.r(0.75)).child(kit::icon(ui, "download", 1.15, fg85)).child(ui.text(text, T::new(Ty::Small).tab().c(fg85)));
    Some(kit::nav(kit::row_look(chip), DL_TAG, Some("Downloads"), None, false).name("downloads-chip"))
}

/// The profile chip's nav tag: A on it opens the profile's Change picture (app.rs).
pub const CHIP_TAG: u32 = 0xFFFF_FF01;

/// sui/ui/bits.tsx `ProfileChip`: picture, name and the portfolio value with today's change. Left
/// out until the wallet has an account. Focusable (Up from the page's top row); A opens Change
/// picture.
fn profile_chip(ui: &mut Ui, st: &State) -> Option<El> {
    let a = st.account.as_ref()?;
    let pl = st.live.profile.as_ref().filter(|p| p.address.eq_ignore_ascii_case(&a.address));
    let name = crate::data::account::shown_name(a, pl);
    // One value: the total (the last one known while a read is under way; left out until the
    // first). Today's change only when it is a cent or more, so an empty wallet reads $0.00 once.
    let total = pl.and_then(|p| p.total_usd).or(a.total_usd);
    let change = pl.and_then(|p| p.change_usd).or(a.change_usd).filter(|c| c.abs() >= 0.005);
    // The pick on this device first (it shows the moment it is made), then rare.market's.
    let pic = match crate::data::live::picked(&a.address).map(|p| p.image_url).filter(|u| !u.is_empty()).or_else(|| pl.and_then(|p| p.avatar.clone())).as_deref() {
        Some(src) => kit::avatar(ui, s(), Some(src), &name, 1.75, false),
        None => crate::screens::market::parts::account_avatar(ui, s(), &a.address, 1.75),
    };
    let label = T::new(Ty::Label);
    let mut under = ui.bx(s().row().items_center().gap(0.4).mt(0.2));
    let mut any = false;
    // The total in the name's white: muted grey did not read over bright Home art.
    if let Some(v) = kit::num(ui, s(), NumKind::Money, total, NumOptions::default(), label.w(500).c(tok::FG), false) {
        under = under.child(v);
        any = true;
    }
    if let Some(d) = kit::num(ui, s(), NumKind::Pnl, change, NumOptions::default(), label, true) {
        under = under.child(d);
        any = true;
    }
    // max-w-[10rem] truncate: a long name ends in an ellipsis instead of pushing the tabs.
    let mut col = ui.bx(s().col().shrink().min0().max_w(10.0)).child(ui.text(name, T::new(Ty::Small).w(600).c(tok::FG).trunc()));
    if any {
        col = col.child(under);
    }
    // A flat dark backing (no glow) keeps the name and total readable over any game's art.
    let chip = ui.bx(s().row().items_center().gap(0.6).px(0.6).py(0.3).shrink().min0()).bg(tok::BG.alpha(0.62)).radius(ui.r(0.75)).child(pic).child(col);
    Some(kit::nav(kit::row_look(chip), CHIP_TAG, Some("Change picture"), None, false))
}

/// rare.market's status bar: SUI price, network throughput, gas, what is running. Each cell has a
/// hairline after it; an unknown value leaves its cell out.
fn status(ui: &mut Ui, st: &State, crowded: bool) -> El {
    let l = &st.live;
    let meta = T::new(Ty::Meta);
    let cell = |ui: &Ui, first: bool| ui.bx(s().row().items_center().gap(0.5).hp(1.0).px(1.0).pl(if first { 2.5 } else { 1.0 })).child(ui.bx(s().abs(Some(0.0), Some(0.0), Some(0.0), None).w_px(1.0)).bg(tok::HAIR));
    let mut c1 = cell(ui, true).child(kit::sui_logo(ui, 0.95)).child(ui.text("SUI", meta.w(500)));
    if let Some(p) = kit::num(ui, s(), NumKind::Price, l.sui_usd, NumOptions::default(), meta, true) {
        c1 = c1.child(p.name("sui-price"));
    }
    if let Some(c) = kit::num(ui, s(), NumKind::Change, l.sui_change, NumOptions::default(), T::new(Ty::Label), true) {
        c1 = c1.child(c);
    }
    let mut row = ui.bx(s().row().items_stretch().hp(1.0)).child(c1.key(key("sui")));
    if let Some(tps) = l.tps {
        row = row.child(cell(ui, false).child(ui.text("TPS", meta.c(tok::MUTED))).child(ui.text(kit::format::grouped(tps as f64, 0, 0), meta.tab().c(if l.degraded { tok::ATTN } else { tok::FG }))).key(key("tps")));
    }
    // A crowded hint bar (eight hints and a running game) gives up Gas rather than cut a hint short.
    if let Some(gas) = l.gas.filter(|_| !(crowded && !st.running.is_empty())) {
        row = row.child(cell(ui, false).child(ui.text("Gas", meta.c(tok::MUTED))).child(ui.text(format!("{} MIST", kit::format::grouped(gas as f64, 0, 0)), meta.tab())).key(key("gas")));
    }
    if let Some(id) = st.running.first() {
        let text = match st.game(id) {
            Some(g) => format!("Playing {}", g.title),
            None if id.starts_with("app:") => format!("{} open", st.run_name(id)),
            None if id.starts_with("stream:") => match id.rsplit(':').next().and_then(|n| n.parse::<u32>().ok()).and_then(|n| st.pc.as_ref()?.apps.iter().find(|a| a.id == n)) {
                Some(a) => format!("Streaming {}", a.name),
                None => "Game running".into(),
            },
            None => "Game running".into(),
        };
        let d = kit::dot(ui, s(), DotTone::Brand, true);
        row = row.child(cell(ui, false).child(d).child(ui.text(text, meta)).key(key("running")));
    }
    row
}

/// The bottom bar: border-t border-hair bg-bg text-small text-fg/80, status left, hints right.
pub fn footer(ui: &mut Ui, st: &State, hints: &[Hint]) -> El {
    let gap = if hints.len() > 7 { 0.45 } else if hints.len() > 6 { 1.0 } else { 1.4 };
    let mut hrow = ui.bx(s().row().items_center().gap(gap).ml_auto().pr(if hints.len() > 7 { 1.5 } else { 2.5 }));
    for h in hints {
        hrow = hrow.child(kit::hint(ui, h.b, &h.label, T::new(Ty::Small).c(tok::fg(0.8))).key(key(&(h.b.name(), &h.label))));
    }
    // First run shows no market cells: nothing there means anything yet to someone setting up.
    let setup = matches!(st.overlay, Some(crate::state::Overlay::Other("firstrun"))) || crate::features::firstrun::wallet_from_setup();
    let st_row = if setup { ui.bx(s()) } else { status(ui, st, hints.len() > 7) };
    ui.bx(s().row().items_stretch().h(3.0).wp(1.0)).bg(tok::BG).child(ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(1.0)).bg(tok::HAIR)).child(st_row).child(hrow)
}

/// The toasts: top right, just under the top bar (it is 4.25rem high), lined up with its right
/// edge (2.5rem), stacking downward with the newest on top; the HUD over a game puts its toasts in
/// the same place (rare_system::hud::toast_origin). A dot and a sentence each, popping in.
pub fn toasts(ui: &mut Ui, st: &State) -> El {
    let mut col = ui.bx(s().abs(Some(4.75), Some(2.5), None, None).col().items_end().gap(0.5).max_w(27.0));
    for t in st.toasts.iter().rev() {
        let d = kit::dot(ui, s(), if t.tone == Tone::Error { DotTone::Negative } else { DotTone::Brand }, false);
        col = col.child(
            ui.bx(s().row().items_center().gap(0.6).px(1.1).py(0.6))
                .bg(tok::L3)
                .radius(ui.r(0.5))
                .border(1.0, tok::BORDER_LIGHT)
                .shadow(&Shadow::xl(0.3))
                .enter(Motion::POP)
                .child(d)
                .child(ui.text_s(s().shrink().min0(), t.text.clone(), T::new(Ty::Small).wrap()))
                .key(t.id),
        );
    }
    col
}

/// The art behind the top of Home: the last two games' heroes stacked, the new one fading in
/// over the old (`.fade`), then the scrims that blend it into the page.
pub fn backdrop(ui: &mut Ui, layers: &[(String, Vec<String>)]) -> El {
    let w = ui.w / ui.rem;
    let mut root = ui.bx(s().fill()).bg(tok::BG).clip();
    for (k, srcs) in layers {
        let img = kit::art(ui, s().fill(), srcs, w, 40.0, ArtFit::Cover(0.25), None);
        root = root.child(ui.bx(s().fill()).enter(Motion::FADE).exit(Motion::FADE).child(img).key(key(k)));
    }
    if !layers.is_empty() {
        let bg = tok::BG;
        root = root
            .child(ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h(7.0)).look(rare_render::Look::default().grad(Gradient { dir: GradDir::ToBottom, stops: [bg.alpha(0.8), bg.alpha(0.4), bg.alpha(0.0)], mid: 0.5 })).key(key("s1")))
            .child(ui.bx(s().abs(Some(0.0), None, Some(0.0), Some(0.0)).wp(0.62)).look(rare_render::Look::default().grad(Gradient { dir: GradDir::ToRight, stops: [bg, bg.alpha(0.55), bg.alpha(0.0)], mid: 0.5 })).key(key("s2")))
            .child(ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).hp(0.45)).look(rare_render::Look::default().grad(Gradient { dir: GradDir::ToTop, stops: [bg, bg.alpha(0.6), bg.alpha(0.0)], mid: 0.5 })).key(key("s3")));
    }
    root
}
