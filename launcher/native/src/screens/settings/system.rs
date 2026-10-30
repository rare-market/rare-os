//! The System section (screens/settings/SystemSections.tsx SystemSection): update rows, language,
//! Desktop mode.

use super::update::{fixture_info, UpdateRows, FIXTURE_NOW_MS};
use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::screens::overlay::common::*;
use rare_render::theme::tok;
use rare_render::Id;

/// lib/locales.ts: English only for now.
pub const LANGUAGES: [(&str, &str); 1] = [("en", "English")];

pub fn language_name(code: Option<&str>) -> &'static str {
    LANGUAGES.iter().find(|(c, _)| Some(*c) == code).map(|(_, n)| *n).unwrap_or(LANGUAGES[0].1)
}

pub struct SystemSection {
    update: UpdateRows,
    language_btn: Option<Id>,
    desktop_btn: Id,
    rows: RowSet,
    support: super::support::SupportRows,
}

impl SystemSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, _now: f64) -> SystemSection {
        let (info, now_ms) = match super::live::get() {
            Some(s) => (s.system, crate::app::wall_ms()),
            None => (fixture_info(), FIXTURE_NOW_MS),
        };
        let update = UpdateRows::build(cx, nav, parent, info, now_ms);
        let p = panel(cx, parent);
        let body = if LANGUAGES.len() > 1 { "The language of Rare OS." } else { "Rare OS is in English for now." };
        let row_ = setting_row(cx, p, "Language", Some(body), true);
        let name = language_name(cfg.language.as_deref());
        let language_btn = if LANGUAGES.len() > 1 {
            let b = button(cx, row_.right, name, Variant::Secondary, Sz::Md, false).sid("language").tag(10);
            Some(nav.add(b.group(10)))
        } else {
            text(cx, row_.right, ty::SMALL, 400, tok::MUTED, name);
            None
        };
        let row_ = setting_row(cx, p, "Desktop mode", Some("A full desktop for installing apps and managing files."), false);
        let b = button(cx, row_.right, "Switch", Variant::Secondary, Sz::Md, false).sid("desktop-mode").tag(11);
        let desktop_btn = nav.add(b.group(10));
        let info = super::live::get().map(|s| s.ssh).unwrap_or_else(super::remote::fixture_default);
        let mut rows = RowSet::default();
        super::remote::build_system(cx, nav, parent, &mut rows, &info);
        let support = super::support::SupportRows::build(cx, nav, parent);
        SystemSection { update, language_btn, desktop_btn, rows, support }
    }
}

impl SectionView for SystemSection {
    fn press(&mut self, cx: &mut Cx, nav: &mut Nav, cfg: &mut Config, now: f64) -> SectionOut {
        if let Some(a) = self.update.press(nav) {
            return SectionOut::Act(a.into());
        }
        if let Some(f) = nav.current().cloned() {
            if let Some(out) = super::remote::press(&mut self.rows, &[], cx, nav, &f, now) {
                return out;
            }
        }
        if let Some(out) = self.support.press(cx, nav, now) {
            return out;
        }
        let Some(cur) = nav.cur else { return SectionOut::None };
        if Some(cur) == self.language_btn {
            let i = LANGUAGES.iter().position(|(c, _)| Some(*c) == cfg.language.as_deref().or(Some("en"))).unwrap_or(0);
            cfg.language = Some(LANGUAGES[(i + 1) % LANGUAGES.len()].0.into());
            return SectionOut::Changed;
        }
        if cur == self.desktop_btn {
            return SectionOut::Act("power:desktop".into());
        }
        SectionOut::None
    }
}
