//! Boot (components/Boot.tsx): the picture Plymouth draws, so the hand-off from the boot splash
//! cannot be seen. The gem (147 x 160 px, the file Plymouth uses) centred horizontally with its top
//! at 45 % of the free height, on the page colour. It cross-fades into Home (300 ms) once the first
//! library lands, never before 900 ms, and gives up waiting at 8 s. RR_BOOT=hold keeps it up (the
//! `?boot` screenshot). The specular sweep needs a mask (an engine request) and is not drawn yet.

use crate::data::art::src_of;
use crate::kit::tok;
use crate::kit::ui::s;
use crate::page::Feedback;
use rare_render::{Ease, Fit, Id, ImageLoader, ImgReq, Node, Scene};

const MIN: f64 = 0.9;
const GIVE_UP: f64 = 8.0;
const OUT: f32 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    On,
    Out,
    Gone,
}

pub struct Boot {
    root: Id,
    pub phase: Phase,
    t0: f64,
    out_at: f64,
    hold: bool,
}

impl Boot {
    /// The splash in its own layer (z 80, above everything).
    pub fn build(scene: &mut Scene, img: &mut ImageLoader, now: f64, hold: bool) -> Boot {
        let layer = scene.layer(80);
        let rem = scene.rem;
        let root = scene.add(layer, s().fill().to_style(rem), Node::bg(tok::BG));
        let (w, h) = (scene.w, scene.h);
        // px, not rem: the gem is Plymouth's bitmap at 1:1.
        let (gw, gh) = (147.0, 160.0);
        let left = ((w - gw) * 0.5).round() / rem;
        let top = ((h - gh) * 0.45).round() / rem;
        let mut node = Node::default();
        if let Some(src) = src_of("brand/boot-gem.png") {
            node = node.image(img.request(ImgReq { src, w: gw as u32, h: gh as u32, fit: Fit::Contain })).contain([0.5, 0.5]);
        }
        scene.add(root, s().abs(Some(top), None, None, Some(left)).w_px(gw).h_px(gh).to_style(rem), node);
        Boot { root, phase: Phase::On, t0: now, out_at: 0.0, hold }
    }

    /// Advances the phase: the next time it needs a frame, and the sound to play.
    pub fn tick(&mut self, scene: &mut Scene, ready: bool, now: f64, animate: bool) -> (Option<f64>, Option<Feedback>) {
        match self.phase {
            Phase::On => {
                if self.hold {
                    return (None, None);
                }
                if (ready && now - self.t0 >= MIN) || now - self.t0 >= GIVE_UP {
                    self.phase = Phase::Out;
                    self.out_at = now;
                    scene.n(self.root).opacity.set_ease(0.0, now, if animate { OUT } else { 0.0 }, Ease::PRO);
                    return (Some(now + OUT as f64), Some(Feedback::Boot));
                }
                (Some(if ready { self.t0 + MIN } else { self.t0 + GIVE_UP }), None)
            }
            Phase::Out => {
                if now - self.out_at >= OUT as f64 || !animate {
                    self.phase = Phase::Gone;
                    scene.remove(self.root);
                    return (None, None);
                }
                (Some(self.out_at + OUT as f64), None)
            }
            Phase::Gone => (None, None),
        }
    }
}
