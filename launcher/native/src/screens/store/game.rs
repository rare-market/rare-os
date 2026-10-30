//! A Store game's page (store/GamePage.tsx, overlay "storegame"), in the Sui Games page's layout:
//! key art on the right, the logo, name, about and facts at the bottom left. Link listings open
//! their play, Steam, Epic or website link. Uploaded games show the price and Buy; once owned:
//! Install, Update, Play, Refund while refundable, Make tradable and List when the studio allows
//! resale. Every action goes to the app as `store:<what>:<slug>`; it reports back with `finish`.

use super::buy::{self, buy_sheet, Buy, Coin};
use super::logic::*;
use super::model::*;
use super::parts::{trunc_text, fact_line, game_logo, price_text, Fact};
use crate::data::art::src_of;
use crate::kit::{self, tok, ArtFit, Btn, DotTone, Variant};
use crate::screens::market::host::*;
use rare_render::input::Pad;
use rare_render::scene::{layer, GradDir, Gradient};
use rare_render::view::{key, El, Look};

/// Key art without baked-in text (SuiGames.tsx CLEAN_ART, CLEAN_BANNERS).
const CLEAN_ART: &[(&str, &str)] = &[("act", "/games/act/hero.jpg")];
const CLEAN_BANNERS: &[&str] = &["warped-universe", "eve-frontier", "pawtato-land", "cosmocadia", "brave-frontier-versus", "doubleup", "m10", "darktimes"];

/// The page's key art: a link listing's clean art, an uploaded game's banner.
pub fn art_of(g: &StoreGame) -> Option<String> {
    if !g.is_link() {
        return g.images.banner.clone().filter(|b| !b.is_empty());
    }
    if let Some((_, a)) = CLEAN_ART.iter().find(|(s, _)| *s == g.slug) {
        return Some(a.to_string());
    }
    let b = g.images.banner.as_deref().filter(|b| !b.is_empty())?;
    let ico = b.split(['?', '#']).next().is_some_and(|p| p.to_ascii_lowercase().ends_with(".ico"));
    (CLEAN_BANNERS.contains(&g.slug.as_str()) && !ico).then(|| b.to_string())
}

/// What the page knows besides the game: the store's state for this slug (state.ts) and the app's.
#[derive(Clone, Debug, Default)]
pub struct GameState {
    pub now_ms: f64,
    /// None while loading; Err when it failed.
    pub game: Option<Result<StoreGame, ()>>,
    pub config: Option<StoreConfig>,
    pub account: Account,
    pub licence: Option<Licence>,
    pub installed: Option<InstalledAt>,
    /// The release the licence may download, read from the chain.
    pub release: Option<ChainRelease>,
    pub progress: Option<Progress>,
    /// What the slug is doing right now ("Getting ready", "Approve in Rare Wallet").
    pub busy: Option<String>,
    /// The game is in the Library (`rare:<slug>`) and so can be played.
    pub lib_game: bool,
    pub running: bool,
    /// The SUI price (e8) for the buy sheet's quote.
    pub sui_e8: Option<u64>,
}

/// One action button.
#[derive(Clone, Debug, PartialEq)]
pub struct GBtn {
    pub label: String,
    pub act: Act,
    pub variant: Variant,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    None,
    /// Link listings: play | steam | epic | website.
    Link(&'static str),
    Free,
    Buy,
    Play,
    Update,
    Install,
    Refund,
    Trade,
    List,
    Uninstall,
}

/// A note under the buttons: what just happened, or an error.
pub type Note = Option<(String, bool)>;

/// Owned's derived words: the buttons, the status line, the notes and the decision.
#[derive(Clone, Debug, PartialEq)]
pub struct OwnedView {
    pub buttons: Vec<GBtn>,
    pub status: Option<String>,
    pub notes: Vec<String>,
    pub decision: Option<UpdateDecision>,
    pub size: Option<u64>,
}

fn b(label: impl Into<String>, act: Act, variant: Variant) -> GBtn {
    GBtn { label: label.into(), act, variant, disabled: false }
}

/// LinkActions' buttons: the first one primary.
pub fn link_buttons(g: &StoreGame) -> Vec<GBtn> {
    let mut v = Vec::new();
    if g.links.play.is_some() {
        v.push(b("Play now", Act::Link("play"), Variant::Secondary));
    }
    if g.links.steam.is_some() {
        v.push(b("Get it on Steam", Act::Link("steam"), Variant::Secondary));
    }
    if g.links.epic.is_some() {
        v.push(b("Get it on Epic Games", Act::Link("epic"), Variant::Secondary));
    }
    if g.links.website.is_some() {
        v.push(b("Website", Act::Link("website"), Variant::Secondary));
    }
    if let Some(f) = v.first_mut() {
        f.variant = Variant::Primary;
    }
    v
}

/// GamePage's Owned: which buttons, which words (`working` is the action waiting on the wallet).
pub fn owned_view(g: &StoreGame, st: &GameState, working: Option<Act>) -> OwnedView {
    let now = st.now_ms;
    let cfg = st.config.as_ref();
    let price = price_now(g, now);
    let selling = can_sell(cfg) && g.listed && !g.blocked && g.game_id.is_some();
    let decision = st.licence.as_ref().map(|_| update_decision(st.installed, st.release.as_ref().map(|r| (r.version, r.edition))));
    let size = st.release.as_ref().and_then(|r| r.builds.iter().find(|b| b.platform == "linux-x86_64").or(r.builds.first()).map(|b| b.size));
    let mut buttons = Vec::new();
    match &st.licence {
        None => {
            // Not on sale yet: no buy button at all (a control that can't work isn't shown).
            if !selling {
            } else if price == Some(0) {
                buttons.push(b("Get for free", Act::Free, Variant::Primary));
            } else if let Some(p) = price {
                buttons.push(b(format!("Buy for {}", usd_text(p)), Act::Buy, Variant::Primary));
            }
        }
        Some(l) => {
            if st.installed.is_some() && st.lib_game {
                let v = if decision == Some(UpdateDecision::Update) { Variant::Secondary } else { Variant::Primary };
                buttons.push(b(if st.running { "Resume" } else { "Play" }, Act::Play, v));
            }
            if decision == Some(UpdateDecision::Update) {
                buttons.insert(0, b("Update", Act::Update, Variant::Primary));
            }
            if st.installed.is_none() && decision != Some(UpdateDecision::Unavailable) {
                let label = match size {
                    Some(sz) => format!("Install ({})", size_text(sz)),
                    None => "Install".into(),
                };
                buttons.push(b(label, Act::Install, Variant::Primary));
            }
            if refundable(l, now) && can_sell(cfg) {
                buttons.push(b("Refund", Act::Refund, Variant::Secondary));
            }
            if l.kind == "licence" && g.resale.as_ref().is_some_and(|r| r.enabled) && tradable_now(l, now) && can_resell(cfg) {
                buttons.push(b("Make tradable", Act::Trade, Variant::Secondary));
            }
            if l.kind == "resale" {
                buttons.push(b("List for sale", Act::List, Variant::Secondary));
            }
            if st.installed.is_some() {
                buttons.push(b("Uninstall", Act::Uninstall, Variant::Ghost));
            }
        }
    }
    for x in buttons.iter_mut() {
        x.disabled = x.disabled || working.is_some() || (st.busy.is_some() && x.label != "Play");
    }

    let status = match &st.progress {
        Some(Progress::Download { done, total }) => Some(format!(
            "Downloading {}%, {} of {}",
            ((*done as f64 / (*total).max(1) as f64) * 100.0).floor(),
            size_text(*done),
            size_text(*total)
        )),
        Some(Progress::Verify) => Some("Checking the download against the chain".into()),
        Some(Progress::Unpack) => Some("Installing".into()),
        None => match (&st.busy, &working) {
            (Some(b), _) => Some(b.clone()),
            (None, Some(Act::Buy | Act::Free | Act::Refund | Act::Trade | Act::List)) => Some("Review and approve in Rare Wallet.".into()),
            _ => None,
        },
    };

    let l = st.licence.as_ref();
    let resale_on = g.resale.as_ref().is_some_and(|r| r.enabled);
    let hold = l.filter(|l| l.kind == "licence" && resale_on).and_then(|l| l.tradable_from_ms).filter(|t| now < *t).map(|t| format!("Tradable from {}.", month_day(t)));
    let notes: Vec<String> = [
        (l.is_none() && selling && price.is_some_and(|p| p > 0)).then(|| "Refundable in full for 48 hours.".to_string()),
        (l.is_none() && selling && g.price.as_ref().is_some_and(|p| p.accept_sui)).then(|| "Pay in USDC or SUI.".to_string()),
        hold,
        (l.is_some_and(|l| refundable(l, now)) && resale_on).then(|| "Making it tradable ends the refund window.".to_string()),
        (l.is_some() && g.launch_check).then(|| "Checks your licence before each launch. Works offline for 7 days.".to_string()),
        (l.is_some() && decision == Some(UpdateDecision::Unavailable)).then(|| "No build for your edition yet.".to_string()),
    ]
    .into_iter()
    .flatten()
    .collect();
    OwnedView { buttons, status, notes, decision, size }
}

// Focus tags.
pub const T_ACT: u32 = 100;

pub struct GamePage {
    pub st: GameState,
    pub note: Note,
    /// The action waiting on the app (Rare Wallet, the installer).
    pub working: Option<Act>,
    pub buying: Option<Buy>,
    pub host: Host,
}

impl GamePage {
    pub fn new(cx: &mut Cx, parent: rare_render::Id, st: GameState, now: f64) -> GamePage {
        let mut p = GamePage { st, note: None, working: None, buying: None, host: Host::new(cx, parent) };
        p.render(cx, now);
        p
    }

    pub fn render(&mut self, cx: &mut Cx, now: f64) {
        let (st, note, working, buying) = (&self.st, &self.note, &self.working, &self.buying);
        self.host.render(cx, now, |ui| view(ui, st, note, working.clone(), buying.as_ref()));
    }

    fn game(&self) -> Option<&StoreGame> {
        self.st.game.as_ref().and_then(|g| g.as_ref().ok())
    }

    /// The buttons as shown now.
    pub fn buttons(&self) -> Vec<GBtn> {
        match self.game() {
            Some(g) if g.is_link() => link_buttons(g),
            Some(g) => owned_view(g, &self.st, self.working.clone()).buttons,
            None => Vec::new(),
        }
    }

    /// The app's answer to the action in `working`: Ok with the done line, or the error's words.
    pub fn finish(&mut self, cx: &mut Cx, result: Result<(), String>, now: f64) {
        let name = self.game().map(|g| g.name.clone()).unwrap_or_default();
        let act = self.working.take();
        self.note = match result {
            Ok(()) => match act {
                Some(Act::Install | Act::Update) => Some((format!("{name} is installed."), false)),
                Some(Act::Free) => Some((format!("{name} is in your library."), false)),
                Some(Act::Refund) => Some(("Refunded. The payment is back in your wallet.".into(), false)),
                Some(Act::Trade) => Some(("Tradable. It's in your kiosk, ready to list.".into(), false)),
                _ => None,
            },
            // A rejected signature says nothing (the app passes an empty error).
            Err(e) if e.is_empty() => None,
            Err(e) => Some((plain(&e), true)),
        };
        self.render(cx, now);
    }

    /// The buy sheet's answer: bought (the sheet closes), rejected (back to idle) or an error.
    pub fn bought(&mut self, cx: &mut Cx, result: Result<(), String>, now: f64) {
        let name = self.game().map(|g| g.name.clone()).unwrap_or_default();
        match result {
            Ok(()) => {
                self.buying = None;
                self.note = Some((format!("{name} is in your library. Install it from here."), false));
            }
            Err(e) => {
                if let Some(b) = self.buying.as_mut() {
                    b.phase = if e.is_empty() { buy::Phase::Idle } else { buy::Phase::Error(plain(&e)) };
                }
            }
        }
        self.render(cx, now);
        if self.buying.is_none() {
            self.host.focus_tag(cx, T_ACT, now);
        }
    }

    /// `run`: needs a Rare Wallet account that can sign; then the app does it.
    fn run(&mut self, act: Act, what: &str) -> Out {
        if self.working.is_some() || self.st.busy.is_some() {
            return Out::None;
        }
        if !self.st.account.can_sign() {
            let t = if self.st.account == Account::None { "Add a Rare Wallet account in Wallet first." } else { "This account is watch only. Switch to a Rare Wallet account in Wallet." };
            self.note = Some((t.into(), false));
            return Out::Moved;
        }
        self.working = Some(act);
        self.note = None;
        let slug = self.game().map(|g| g.slug.clone()).unwrap_or_default();
        Out::Act(format!("store:{what}:{slug}"))
    }

    fn activate(&mut self, cx: &mut Cx, now: f64) -> Out {
        let Some(tag) = self.host.tag() else { return Out::None };
        let Some(btn) = tag.checked_sub(T_ACT).and_then(|i| self.buttons().get(i as usize).cloned()) else { return Out::None };
        if btn.disabled {
            return Out::None;
        }
        let slug = self.game().map(|g| g.slug.clone()).unwrap_or_default();
        let out = match btn.act {
            Act::None => Out::None,
            Act::Link(k) => Out::Act(format!("store:{k}:{slug}")),
            Act::Play => Out::Act(format!("store:play:{slug}")),
            Act::Buy => {
                if self.st.account.can_sign() {
                    let g = self.game().cloned().unwrap_or_default();
                    self.buying = Some(Buy::new(&g, self.st.sui_e8, self.st.now_ms));
                    Out::Moved
                } else {
                    self.run(Act::Buy, "buy")
                }
            }
            Act::Free => self.run(Act::Free, "buy:free"),
            Act::Install => self.run(Act::Install, "install"),
            Act::Update => self.run(Act::Update, "install"),
            Act::Refund => self.run(Act::Refund, "refund"),
            Act::Trade => self.run(Act::Trade, "trade"),
            Act::List => self.run(Act::List, "list"),
            Act::Uninstall => self.run(Act::Uninstall, "uninstall"),
        };
        if out != Out::None {
            self.render(cx, now);
        }
        out
    }

    fn press_sheet(&mut self, cx: &mut Cx, p: Pad, now: f64) -> Out {
        let working = matches!(self.buying.as_ref().map(|b| &b.phase), Some(buy::Phase::Working(_)));
        match p {
            Pad::B => {
                if !working {
                    self.buying = None;
                    self.render(cx, now);
                    self.host.focus_tag(cx, T_ACT, now);
                }
                Out::Moved
            }
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.host.step(cx, p, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Pad::A => {
                let coin = match self.host.tag() {
                    Some(buy::T_CLOSE) => {
                        self.buying = None;
                        self.render(cx, now);
                        self.host.focus_tag(cx, T_ACT, now);
                        return Out::Moved;
                    }
                    Some(buy::T_USDC) => Coin::Usdc,
                    Some(buy::T_SUI) if self.buying.as_ref().is_some_and(|b| b.sui_mist.is_some()) => Coin::Sui,
                    _ => return Out::None,
                };
                if working {
                    return Out::None;
                }
                if let Some(b) = self.buying.as_mut() {
                    b.phase = buy::Phase::Working(coin);
                }
                self.render(cx, now);
                let slug = self.game().map(|g| g.slug.clone()).unwrap_or_default();
                Out::Act(format!("store:buy:{slug}:{}", coin.id()))
            }
            _ => Out::None,
        }
    }

    pub fn press(&mut self, cx: &mut Cx, p: Pad, now: f64) -> Out {
        if self.buying.is_some() {
            return self.press_sheet(cx, p, now);
        }
        match p {
            Pad::B => Out::Close,
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.host.step(cx, p, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Pad::A => self.activate(cx, now),
            _ => Out::None,
        }
    }

    pub fn page_hints(&self) -> Vec<(&'static str, String)> {
        let mut h = Vec::new();
        if self.host.tag().is_some() {
            h.push(("a", "Select".to_string()));
        }
        h.push(("b", "Back".to_string()));
        h
    }
}

/// `object-cover object-[fx_fy]` for art whose size is known (local files): the image is drawn
/// at its covering size and offset in a clipped box. Remote art falls back to a centred cover.
fn cover_at(ui: &mut Ui, src: &str, w: f32, h: f32, fx: f32, fy: f32) -> El {
    let dims = match src_of(src) {
        Some(rare_render::Src::File(p)) => image::image_dimensions(&p).ok(),
        _ => None,
    };
    let srcs = vec![src.to_string()];
    let Some((sw, sh)) = dims else {
        return kit::art(ui, s().fill(), &srcs, w, h, ArtFit::Cover(fy), None);
    };
    let k = (w / sw as f32).max(h / sh as f32);
    let (iw, ih) = (sw as f32 * k, sh as f32 * k);
    let (ox, oy) = ((w - iw) * fx, (h - ih) * fy);
    let img = kit::art(ui, s().abs(Some(oy), None, None, Some(ox)).wh(iw, ih), &srcs, iw, ih, ArtFit::Cover(0.5), None);
    ui.bx(s().fill()).clip().child(img)
}

/// The page (GamePage.tsx's JSX): fixed inset-0 over the shell, the art, the body at the bottom.
pub fn view(ui: &mut Ui, st: &GameState, note: &Note, working: Option<Act>, buying: Option<&Buy>) -> El {
    let mut root = ui.bx(s().fill()).bg(tok::BG).portal(layer::PAGE).scope(4);
    let g = match &st.game {
        None => return root.child(ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(2.0)).bg(tok::BRAND.alpha(0.6))),
        Some(Err(())) => {
            let col = ui
                .bx(s().fill().col().justify_end().gap(0.5).px(2.5).pb(4.5))
                .child(ui.text("Couldn't load this game", T::new(Ty::Title).w(600)))
                .child(ui.text("Check the connection and try again.", T::new(Ty::Small).c(tok::MUTED)));
            return root.child(col);
        }
        Some(Ok(g)) => g,
    };
    let now = st.now_ms;

    if let Some(art) = art_of(g) {
        let (w, h) = (ui.w * 0.7 / ui.rem, ui.h / ui.rem);
        let img = cover_at(ui, &art, w, h, 0.8, 0.3);
        let bg = tok::BG;
        let left = ui.bx(s().abs(Some(0.0), None, Some(0.0), Some(0.0)).wp(0.5)).look(Look::default().grad(Gradient { dir: GradDir::ToRight, stops: [bg, bg.alpha(0.5), bg.alpha(0.0)], mid: 0.5 }));
        let bottom = ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).hp(0.6)).look(Look::default().grad(Gradient { dir: GradDir::ToTop, stops: [bg, bg.alpha(0.85), bg.alpha(0.0)], mid: 0.5 }));
        root = root.child(ui.bx(s().abs_pct(Some(0.0), Some(0.0), Some(0.0), Some(0.3))).child(img).child(left).child(bottom));
    }

    // Logo, studio line and the 3rem name.
    let names = ui
        .bx(s().col().gap(0.25))
        .child(ui.text(studio_line(g), T::new(Ty::Small).w(500).c(tok::fg(0.7))))
        .child(ui.text(g.name.clone(), T::new(Ty::Px(3.0, 3.0)).w(600).track(-0.035)));
    let head = ui.bx(s().row().items_center().gap(1.0)).child(game_logo(ui, g, 4.5)).child(names);
    let mut body = ui.bx(s().fill().col().justify_end().gap(1.0).px(2.5).pb(4.5)).child(head);

    let about = g.page.as_ref().map(|p| p.about.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| g.description.clone());
    if !about.is_empty() {
        body = body.child(ui.text_s(s().max_w(44.0), about, T::new(Ty::Body).c(tok::fg(0.8)).lead(0.9375 * 1.625).clamp(3)).key(key("g-about")));
    }

    // Facts: status, where or platforms, tags, owned.
    let small = T::new(Ty::Small).c(tok::fg(0.75));
    let mut facts = ui.bx(s().row().wrap().items_center().gap(1.25));
    if let Some(stt) = g.status.as_deref() {
        let mut f = ui.bx(s().row().items_center().gap(0.45));
        if stt == "live" {
            f = f.child(kit::dot(ui, s(), DotTone::Primary, false));
        }
        facts = facts.child(f.child(ui.text(status_word(stt).to_string(), small)));
    }
    if g.is_link() {
        facts = facts.child(ui.text(link_where(g), small));
    } else if !g.platforms.is_empty() {
        facts = facts.child(ui.text(platform_text(&g.platforms), small));
    }
    if let Some(tags) = g.page.as_ref().map(|p| &p.tags).filter(|t| !t.is_empty()) {
        facts = facts.child(ui.text(tags.join(", "), small));
    }
    if st.licence.is_some() {
        facts = facts.child(ui.text("In your library", small));
    }
    body = body.child(facts.key(key("g-facts")));

    if g.is_link() {
        let btns = link_buttons(g);
        if btns.is_empty() {
            body = body.child(ui.text("This listing has no links yet.", T::new(Ty::Small).c(tok::MUTED)));
        } else {
            body = body.child(action_row(ui, &btns, s()));
        }
        return root.child(body);
    }

    // Owned: buttons, status or note, notes, the stat line.
    let ov = owned_view(g, st, working);
    body = body.child(action_row(ui, &ov.buttons, s().mt(0.25)).key(key("g-actions")));
    let line = ov.status.clone().map(|s| (s, false)).or_else(|| note.clone());
    if let Some((text, bad)) = line {
        body = body.child(ui.text_s(s().max_w(44.0), text, T::new(Ty::Small).tab().c(if bad { tok::NEG } else { tok::fg(0.8) }).wrap()).key(key("g-status")));
    }
    if !ov.notes.is_empty() {
        body = body.child(ui.text_s(s().max_w(44.0), ov.notes.join(" "), T::new(Ty::Small).c(tok::MUTED).wrap()).key(key("g-notes")));
    }
    let price = price_text(ui, s(), g, Ty::Row, now).map(Fact::El);
    let label = st.release.as_ref().map(|r| r.label.clone()).or_else(|| g.latest.as_ref().map(|l| l.label.clone())).filter(|l| !l.is_empty());
    let version = label.map(|l| Fact::El(trunc_text(ui, s(), l, T::new(Ty::Row).w(500).mono())));
    let size = ov.size.or_else(|| g.latest.as_ref().and_then(|l| l.builds.first()).map(|b| b.size)).map(|s| Fact::Text(size_text(s)));
    let l = st.licence.as_ref();
    let facts = vec![
        ("Price", price),
        ("Version", version),
        ("Size", size),
        ("Your licence", l.map(|l| Fact::Text(format!("#{}", l.number)))),
        ("Refund", l.filter(|l| refundable(l, now)).map(|l| Fact::Text(format!("{} left", time_left(l.refundable_until_ms.unwrap_or(0.0) - now))))),
        ("Resale", g.resale.as_ref().filter(|r| r.enabled).map(|r| Fact::Text(format!("{}% royalty", crate::screens::trade::fmt::trim_zeros(&format!("{:.2}", r.royalty_bps as f64 / 100.0)))))),
        ("Sales", (g.sales > 0).then(|| Fact::Text(crate::kit::format::grouped(g.sales as f64, 0, 0)))),
    ];
    // The `.num` span (Fira Code) inside the Inter line makes the browser's line box 1px taller.
    let mono = facts.iter().any(|(l, v)| *l == "Version" && v.is_some());
    let line = fact_line(ui, if mono { s().min_h(1.0 + 1.5 + 1.0 / ui.rem) } else { s() }, facts);
    body = body.child(ui.bx(s().col().mt(0.4).max_w(60.0).pt(1.0 + 1.0 / ui.rem)).child(ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(1.0)).bg(tok::HAIR)).maybe(line).key(key("g-stats")));
    root = root.child(body);
    if let Some(bs) = buying {
        root = root.child(buy_sheet(ui, g, bs, now).key(key("g-buy")));
    }
    root
}

/// The action buttons in their nav group, the first one focus's default.
fn action_row(ui: &mut Ui, btns: &[GBtn], st: S) -> El {
    let mut row = ui.bx(st.row().gap(0.75)).group(kit::group("storegame-actions", false));
    for (i, x) in btns.iter().enumerate() {
        row = row.child(kit::button(ui, Btn::new(x.label.clone(), T_ACT + i as u32).v(x.variant).def(i == 0).disabled(x.disabled)));
    }
    row
}

impl Page for GamePage {
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.page_hints()
    }
}

impl Screen for GamePage {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn state(slug: &str, sell: bool, own: bool) -> (StoreGame, GameState) {
        let d = demo();
        let g = d.game(slug).unwrap().clone();
        let licence = own.then(|| d.licences[0].clone());
        let release = licence.as_ref().and(g.latest.as_ref()).map(|l| ChainRelease::from_latest(&g.slug, l));
        let cfg = normalize_config(if sell { &d.config_sell } else { &d.config_closed });
        let st = GameState { now_ms: d.now, game: Some(Ok(g.clone())), config: Some(cfg), account: Account::Watch(crate::screens::trade::DEMO.into()), licence, release, ..Default::default() };
        (g, st)
    }

    #[test]
    fn no_buy_button_until_the_store_sells() {
        let (g, st) = state("driftline", false, false);
        let ov = owned_view(&g, &st, None);
        assert!(ov.buttons.is_empty());
        assert!(ov.notes.is_empty());
    }

    #[test]
    fn buy_with_the_sale_price_and_its_notes() {
        let (g, st) = state("hollow-keep", true, false);
        let ov = owned_view(&g, &st, None);
        assert_eq!(ov.buttons[0].label, "Buy for $6.99");
        assert_eq!(ov.notes.join(" "), "Refundable in full for 48 hours. Pay in USDC or SUI.");
        let (g, st) = state("pocket-orchard", true, false);
        assert_eq!(owned_view(&g, &st, None).buttons[0].label, "Get for free");
    }

    #[test]
    fn owned_shows_install_refund_and_the_hold() {
        let (g, st) = state("driftline", true, true);
        let ov = owned_view(&g, &st, None);
        let labels: Vec<&str> = ov.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["Install (734 MB)", "Refund"]);
        assert_eq!(ov.decision, Some(UpdateDecision::Install));
        assert_eq!(ov.notes.join(" "), "Tradable from Nov 3. Making it tradable ends the refund window. Checks your licence before each launch. Works offline for 7 days.");
        let mut inst = st.clone();
        inst.installed = Some(InstalledAt { edition: 1, version: 2 });
        inst.lib_game = true;
        let labels: Vec<String> = owned_view(&g, &inst, None).buttons.into_iter().map(|b| b.label).collect();
        assert_eq!(labels, ["Update", "Play", "Refund", "Uninstall"]);
    }

    #[test]
    fn link_listings_open_their_links() {
        let d = demo();
        let g = d.game("act").unwrap();
        let labels: Vec<String> = link_buttons(g).into_iter().map(|b| b.label).collect();
        assert_eq!(labels, ["Play now", "Website"]);
        assert_eq!(art_of(g).as_deref(), Some("/games/act/hero.jpg"));
    }

    #[test]
    fn status_words() {
        let (g, mut st) = state("driftline", true, true);
        st.progress = Some(Progress::Download { done: 367_001_600, total: 734_003_200 });
        assert_eq!(owned_view(&g, &st, None).status.as_deref(), Some("Downloading 50%, 367 MB of 734 MB"));
        st.progress = None;
        assert_eq!(owned_view(&g, &st, Some(Act::Refund)).status.as_deref(), Some("Review and approve in Rare Wallet."));
    }
}
