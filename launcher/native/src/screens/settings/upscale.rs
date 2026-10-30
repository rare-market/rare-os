//! Upscaling rows for Quick Settings (components/UpscaleRows.tsx), same pattern as the Frame limit
//! row: Upscaling (Off, FSR, NIS), Sharpness (when on), Render size. Each row is a label and chips.

use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::theme::tok;
use rare_render::{Id, Node};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Off,
    Fsr,
    Nis,
}

/// Gamescope upscaling. width/height 0 is native.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Upscale {
    pub mode: Mode,
    /// 0 soft to 20 sharp.
    pub sharpness: u32,
    pub width: u32,
    pub height: u32,
}

impl Default for Upscale {
    fn default() -> Self {
        Upscale { mode: Mode::Off, sharpness: 10, width: 0, height: 0 }
    }
}

pub const MODES: [(Mode, &str); 3] = [(Mode::Off, "Off"), (Mode::Fsr, "FSR"), (Mode::Nis, "NIS")];
pub const SIZES: [(u32, u32, &str); 3] = [(0, 0, "1200p"), (1280, 800, "800p"), (960, 600, "600p")];
pub const SHARP: [u32; 3] = [4, 10, 16];
pub const SHARP_LABELS: [&str; 3] = ["Soft", "Medium", "Sharp"];

/// A press on a chip, applied to the value.
pub fn apply(u: Upscale, group: &str, idx: usize) -> Upscale {
    match group {
        "mode" => Upscale { mode: MODES[idx].0, ..u },
        "sharp" => Upscale { sharpness: SHARP[idx], ..u },
        _ => Upscale { width: SIZES[idx].0, height: SIZES[idx].1, ..u },
    }
}

pub struct UpscaleRows {
    pub value: Upscale,
}

fn chip_row(cx: &mut Cx, nav: &mut Nav, parent: Id, label: &str, labels: &[&str], on: Option<usize>, group_name: &str, group: u32) {
    let r = cx.r();
    let row_ = cx.scene.add(parent, s().row().items_center().gap(0.5 * r).done(), Node::default());
    let lab = cx.scene.add(row_, s().grow().done(), Node::default());
    cx.scene.add_text(cx.t, lab, s().done(), label, ts(r, ty::SMALL.0, ty::SMALL.1, 400), tok::FG.alpha(0.85));
    for (i, l) in labels.iter().enumerate() {
        let is_on = on == Some(i);
        let (base, fg) = if is_on { (tok::FG, tok::BG) } else { (tok::L2, tok::FG) };
        let chip = cx.scene.add(row_, s().row().items_center().justify_center().h(2.25 * r).min_w(3.3 * r).px(0.8 * r).done(), Node::bg(base).radius(0.25 * r));
        let t = cx.scene.add_text(cx.t, chip, s().done(), l, ts(r, ty::SMALL.0, ty::SMALL.1, 500).tabular(), fg);
        nav.add(Focusable { id: chip, kind: Kind::Btn { base, fg, text: t }, group, hint: String::new(), tag: i as u32, sid: group_name.into() });
    }
}

impl UpscaleRows {
    /// Builds the rows into `parent` (a column). Focus groups: 20 mode, 21 sharpness, 22 render size.
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, u: Upscale) -> UpscaleRows {
        let modes: Vec<&str> = MODES.iter().map(|(_, l)| *l).collect();
        chip_row(cx, nav, parent, "Upscaling", &modes, MODES.iter().position(|(m, _)| *m == u.mode), "mode", 20);
        if u.mode != Mode::Off {
            chip_row(cx, nav, parent, "Sharpness", &SHARP_LABELS, SHARP.iter().position(|s| *s == u.sharpness), "sharp", 21);
        }
        let sizes: Vec<&str> = SIZES.iter().map(|(_, _, l)| *l).collect();
        chip_row(cx, nav, parent, "Render size", &sizes, SIZES.iter().position(|(w, h, _)| *w == u.width && *h == u.height), "size", 22);
        UpscaleRows { value: u }
    }

    /// A on the focused chip: the new value (the host applies it and rebuilds the rows).
    pub fn press(&self, nav: &Nav) -> Option<Upscale> {
        let f = nav.current()?;
        if !matches!(f.sid.as_str(), "mode" | "sharp" | "size") {
            return None;
        }
        Some(apply(self.value, &f.sid, f.tag as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chip_changes_only_its_own_field() {
        let u = Upscale::default();
        assert_eq!(apply(u, "mode", 1).mode, Mode::Fsr);
        assert_eq!(apply(u, "mode", 1).sharpness, 10);
        assert_eq!(apply(u, "sharp", 2).sharpness, 16);
        let v = apply(u, "size", 1);
        assert_eq!((v.width, v.height), (1280, 800));
        let v = apply(v, "size", 0);
        assert_eq!((v.width, v.height), (0, 0));
    }
}
