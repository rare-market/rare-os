//! Shared builders for the streaming screens: layers (a page or a sheet with its own focus),
//! buttons with icons, rich sentences, empty cards, the segmented value chips, the install bar.

use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::taffy::prelude::length;
use rare_render::theme::tok;
use rare_render::{Color, Fit, Id, ImgReq, Node, Src};

/// Everything A can do on these screens.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// A cover: open the game's sheet.
    Tile(String, u32),
    OpenSetup,
    ListAgain(String),
    Wake(String),
    GetMoonlight,
    AddAddress,
    LookAgain,
    HostRow(String),
    CancelPair,
    ShowGames,
    TryAgain(String),
    BackFromPair,
    ChooseResolution,
    ChooseBitrate,
    ChooseCodec,
    Play,
    Resume,
    QuitGame,
    QuitAndPlay,
    WakeAndPlay,
    LookAgainApp,
    SetupFromApp,
}

pub struct Layer {
    pub root: Id,
    /// The sheet card (None for a page).
    pub card: Option<Id>,
    pub nav: Nav,
    pub acts: Vec<Act>,
    /// (cover button, its name line): the name goes white under focus.
    pub names: Vec<(Id, Id)>,
    /// (button, icon): the icon follows the label colour under focus.
    pub icons: Vec<(Id, Id)>,
    pub rails: Vec<Id>,
    pub default: Option<usize>,
}

impl Layer {
    pub fn new(root: Id, card: Option<Id>) -> Layer {
        Layer { root, card, nav: Nav::default(), acts: vec![], names: vec![], icons: vec![], rails: vec![], default: None }
    }

    pub fn reg(&mut self, f: Focusable, act: Act) -> usize {
        let tag = self.acts.len();
        self.nav.add(f.tag(tag as u32));
        self.acts.push(act);
        tag
    }

    pub fn act_of(&self, id: Id) -> Option<&Act> {
        self.nav.items.iter().find(|f| f.id == id).and_then(|f| self.acts.get(f.tag as usize))
    }

    pub fn current_act(&self) -> Option<Act> {
        self.nav.current().and_then(|f| self.acts.get(f.tag as usize)).cloned()
    }

    /// Focuses `want` if it still exists, else the default, else the first item.
    pub fn focus_initial(&mut self, cx: &mut Cx, want: Option<&Act>, now: f64) {
        let pick = want
            .and_then(|a| self.acts.iter().position(|x| x == a))
            .or(self.default)
            .or(if self.acts.is_empty() { None } else { Some(0) });
        if let Some(t) = pick {
            if let Some(id) = self.nav.items.iter().find(|f| f.tag == t as u32).map(|f| f.id) {
                self.nav.focus(cx.scene, id, now, false);
                self.sync(cx, now);
            }
        }
    }

    /// Focus-dependent colours Nav doesn't know: cover names, button icons; and the rail scroll.
    pub fn sync(&mut self, cx: &mut Cx, now: f64) {
        let cur = self.nav.cur;
        for &(b, n) in &self.names {
            let c = if Some(b) == cur { tok::FG } else { tok::FG.alpha(0.85) };
            if let Some(t) = cx.scene.n(n).text.as_mut() {
                t.color.set(c, now, 0.11);
            }
        }
        for &(b, i) in &self.icons {
            let c = if Some(b) == cur { tok::BG } else { tok::FG };
            if let Some(im) = cx.scene.n(i).img.as_mut() {
                im.tint = Some(c);
            }
        }
        if let Some(id) = cur {
            let r = cx.r();
            for &rail in &self.rails {
                let Some(off) = cx.scene.offset_in(id, rail) else { continue };
                let view = cx.scene.rect(rail);
                let cur_x = cx.scene.get(rail).scroll_x.target();
                let max = cx.scene.max_scroll(rail)[0];
                let pad = 2.5 * r;
                let (left, right) = (off[0], off[0] + off[2]);
                let want = if left - cur_x < pad {
                    Some(left - pad)
                } else if right - cur_x > view[2] - pad {
                    Some(right - view[2] + pad)
                } else {
                    None
                };
                if let Some(t) = want {
                    cx.scene.n(rail).scroll_x.glide(t.clamp(0.0, max.max(0.0)), now);
                }
            }
        }
    }
}

pub const MONITOR: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="14" x="2" y="3" rx="2"/><path d="M8 21h8M12 17v4"/></svg>"#;
pub const PLUS: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M12 5v14"/></svg>"#;

pub fn wrap_text(cx: &mut Cx, parent: Id, t: (f32, f32), weight: u16, color: Color, text_: &str) -> Id {
    let r = cx.r();
    cx.scene.add_text(cx.t, parent, s().done(), text_, ts(r, t.0, t.1, weight).wrap(), color)
}

pub fn mono_text(cx: &mut Cx, parent: Id, t: (f32, f32), weight: u16, color: Color, text_: &str, truncate: bool) -> Id {
    let r = cx.r();
    let mut st = ts(r, t.0, t.1, weight).mono();
    let style = if truncate {
        st = st.truncate();
        s().shrink().done()
    } else {
        s().done()
    };
    cx.scene.add_text(cx.t, parent, style, text_, st, color)
}

/// A sentence with some words in the mono code look: `(text, is_code)` pieces; a piece that starts
/// right after another without a space stays glued to it (`github.com/x`, then a comma).
pub fn rich(cx: &mut Cx, parent: Id, t: (f32, f32), color: Color, segs: &[(&str, bool)]) -> Id {
    let r = cx.r();
    let gap = (t.0 * r * 0.27).round();
    let row_ = cx.scene.add(parent, s().row().wrap().gap_xy(gap, 0.0).done(), Node::default());
    let mut groups: Vec<Vec<(String, bool)>> = vec![];
    let mut ended_space = true;
    for (txt, code) in segs {
        if *code {
            if ended_space || groups.is_empty() {
                groups.push(vec![]);
            }
            groups.last_mut().unwrap().push((txt.to_string(), true));
            ended_space = false;
            continue;
        }
        let starts_space = txt.starts_with(' ');
        for (i, w) in txt.split_whitespace().enumerate() {
            let glue = i == 0 && !starts_space && !ended_space && !groups.is_empty();
            if !glue {
                groups.push(vec![]);
            }
            groups.last_mut().unwrap().push((w.to_string(), false));
        }
        ended_space = txt.ends_with(' ') || txt.is_empty();
    }
    for g in groups {
        let cell = cx.scene.add(row_, s().row().done(), Node::default());
        for (w, code) in g {
            if code {
                mono_text(cx, cell, t, 400, tok::FG, &w, false);
            } else {
                text(cx, cell, t, 400, color, &w);
            }
        }
    }
    row_
}

/// ui Button (`button()` plus the web's disabled look and an optional leading icon).
pub struct Btn {
    pub f: Focusable,
    pub icon: Option<Id>,
}

pub fn btn(cx: &mut Cx, parent: Id, label: &str, v: Variant, hint: &str, disabled: bool, icon: Option<(&str, &str, f32)>) -> Btn {
    let mut f = button(cx, parent, label, v, Sz::Md, false).hint(hint);
    let mut ic = None;
    if let Some((name, svg, rem)) = icon {
        let r = cx.r();
        let k = cx.img.request(svg_req(name, svg, rem * r));
        let id = cx.scene.insert(f.id, 0, s().wh(rem * r, rem * r).done(), Node::default().icon(k, tok::FG));
        ic = Some(id);
    }
    if disabled {
        cx.scene.n(f.id).opacity.snap(0.4);
        f.hint = String::new();
    }
    Btn { f, icon: ic }
}

pub fn reg_btn(layer: &mut Layer, b: Btn, act: Act, group: u32, def: bool, disabled: bool) {
    if disabled {
        return;
    }
    let id = b.f.id;
    let t = layer.reg(b.f.group(group), act);
    if let Some(i) = b.icon {
        layer.icons.push((id, i));
    }
    if def {
        layer.default = Some(t);
    }
}

/// ui Empty: title, line, actions row (returned, with the web's mt-[0.5rem]).
pub fn empty_card(cx: &mut Cx, parent: Id, title: &str, body: Option<&str>) -> Id {
    let r = cx.r();
    let card = cx.scene.add(parent, s().col().items_start().gap(0.5 * r).pad(1.5 * r, 1.3 * r, 1.5 * r, 1.3 * r).done(), Node::bg(tok::L1).radius(0.5 * r).border(1.0, tok::HAIR));
    text(cx, card, ty::ROW, 600, tok::FG, title);
    if let Some(b) = body {
        cx.scene.add_text(cx.t, card, s().max_w(34.0 * r).done(), b, ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
    }
    cx.scene.add(card, s().row().gap(0.75 * r).mt(0.5 * r).done(), Node::default())
}

/// A status dot (ui Dot): white when "primary", the muted icon tone otherwise, red when negative.
pub fn status_dot(cx: &mut Cx, parent: Id, tone: Tone) -> Id {
    let c = match tone {
        Tone::Primary => tok::FG,
        Tone::Muted => tok::MUTED,
        Tone::Negative => tok::NEG,
    };
    dot(cx, parent, c)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tone {
    Primary,
    Muted,
    Negative,
}

/// ui Segmented: chips 1 px apart, rounded at the two ends.
pub fn chips(cx: &mut Cx, parent: Id, items: &[String], on: usize) -> Id {
    let r = cx.r();
    let wrap = cx.scene.add(parent, s().row().gap(1.0).no_shrink_().done(), Node::default().radius(0.25 * r).clip());
    for (i, l) in items.iter().enumerate() {
        let c = cx.scene.add(wrap, s().row().items_center().justify_center().h(1.9 * r).min_w(2.4 * r).px(0.6 * r).done(), Node::bg(if i == on { tk::SEG_ACTIVE } else { tk::SEG }));
        text(cx, c, ty::META, 500, if i == on { tok::FG } else { tok::MUTED }, l);
    }
    wrap
}

/// InstallBar: h-[0.35rem] on layer 04, white fill to `progress`; while unknown a short sweep.
pub fn install_bar(cx: &mut Cx, parent: Id, progress: Option<f32>) -> Id {
    let r = cx.r();
    let bar = cx.scene.add(parent, s().h(0.35 * r).grow().done(), Node::bg(tok::L4).radius(0.175 * r).clip());
    match progress {
        Some(p) => cx.scene.add(bar, s().abs(Some(0.0), None, Some(0.0), Some(0.0)).wp(p.clamp(0.0, 1.0)).done(), Node::bg(tok::FG).radius(0.175 * r)),
        None => cx.scene.add(bar, s().abs(Some(0.0), None, Some(0.0), Some(0.0)).wp(0.4).done(), Node::bg(tok::FG.alpha(0.55))),
    };
    bar
}

/// A cover: art cropped to fill, or the generated title tile (components/Art Generated).
pub fn cover(cx: &mut Cx, parent: Id, art: Option<&Src>, title: &str, w: f32, h: f32) {
    let r = cx.r();
    match art {
        Some(src) => {
            let k = cx.img.request(ImgReq { src: src.clone(), w: w.round() as u32, h: h.round() as u32, fit: Fit::Cover { fy: 0.5 } });
            cx.scene.add(parent, s().fill().done(), Node::default().image(k));
        }
        None => {
            // Broken only between words and sized to fit (kit::fit_title); w and h are px.
            let box_ = cx.scene.add(parent, s().fill().items_center().justify_center().p(w * 0.1).done(), Node::default().clip());
            let (text, size) = crate::kit::fit_title(title, w * 0.8 / r, h * 0.8 / r, 1.05, 0.63, 4);
            cx.scene.add_text(cx.t, box_, s().wp(1.0).done(), &text, ts(r, size, size * 1.25, 600).align(rare_render::text::TextAlign::Center), tok::FG.alpha(0.8));
        }
    }
}

/// A negative margin (the rails' -1.4rem trick), top and bottom.
pub fn neg_margin_y(cx: &mut Cx, id: Id, v: f32) {
    let mut st = cx.scene.style(id);
    st.margin.top = length(-v);
    st.margin.bottom = length(-v);
    cx.scene.set_style(id, st);
}
