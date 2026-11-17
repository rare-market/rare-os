//! Library, Emulation, Add games: two ways to put games in the ROM folders without a file
//! manager (rare_emu::add, upload).
//! - From a phone or a computer: a page on the local network, shown as a QR code, while this page
//!   is open; files dropped on it go into their systems' folders.
//! - From a USB drive or an SD card: the games on it, by system, copied in with one press.
//! The library rescans after each game that lands.

use super::emulation::{line, page};
use crate::act::Act;
use crate::kit::{self, s, tok, Btn as B, BtnIcon, Ty, Ui, T};
use crate::page::{hash, Acts, Feedback, Fx, Page};
use crate::state::State;
use rare_emu::add::Found;
use rare_i18n::{tr, trf};
use rare_render::focus::Group;
use rare_render::view::{key, El};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub const ADD: &str = "emuadd";

const START: u32 = 1;
const STOP: u32 = 2;
const COPY: u32 = 3;
const DRIVE: u32 = 100;

#[derive(Default)]
struct Store {
    url: Option<String>,
    error: Option<String>,
    /// The drive looked through, its games, and what copying them did.
    scan: Option<(String, Vec<Found>)>,
    scanning: bool,
    copy: Option<(u64, u64)>,
    copied: Option<usize>,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);
static SESSION: Mutex<Option<rare_emu::upload::Session>> = Mutex::new(None);
static VER: AtomicU64 = AtomicU64::new(0);

fn with<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut g = STORE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Store::default))
}

fn set(f: impl FnOnce(&mut Store)) {
    with(f);
    VER.fetch_add(1, Ordering::Relaxed);
    super::bus::wake();
}

fn ver() -> u64 {
    VER.load(Ordering::Relaxed) ^ rare_emu::upload::version().rotate_left(32)
}

fn library_changed() {
    super::bus::ctx().send(rare_core::Event::LibraryChanged);
}

fn start_session() {
    let Some(ip) = rare_system::ssh::lan_ip() else {
        return set(|s| s.error = Some(tr().features.emu_add_no_network.into()));
    };
    match rare_emu::upload::start(&ip, rare_emu::add::roms_dir(), library_changed) {
        Ok(sess) => {
            let url = sess.url.clone();
            *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(sess);
            set(|s| {
                s.url = Some(url);
                s.error = None;
            });
        }
        Err(e) => set(|s| s.error = Some(e)),
    }
}

fn stop_session() {
    if let Some(sess) = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take() {
        sess.stop();
    }
    set(|s| s.url = None);
}

/// Removable drives: USB drives and the SD card.
fn drives() -> Vec<rare_library::storage::Drive> {
    use rare_library::storage::DriveKind;
    rare_library::storage::drives().into_iter().filter(|d| matches!(d.kind, DriveKind::Usb | DriveKind::Sd)).collect()
}

fn scan_drive(d: &rare_library::storage::Drive) {
    let (label, mount) = (d.label.clone(), d.mount.clone());
    set(|s| {
        s.scanning = true;
        s.scan = None;
        s.copied = None;
    });
    std::thread::spawn(move || {
        let found = rare_emu::add::find_on(&mount, 200_000);
        set(|s| {
            s.scanning = false;
            s.scan = Some((label, found));
        });
    });
}

fn copy_found() {
    let Some(list) = with(|s| s.scan.as_ref().map(|(_, f)| f.iter().filter(|g| g.system.is_some()).cloned().collect::<Vec<_>>())) else { return };
    let total: u64 = list.iter().map(|f| f.bytes).sum();
    set(|s| {
        s.copy = Some((0, total));
        s.copied = None;
    });
    std::thread::spawn(move || {
        let roms = rare_emu::add::roms_dir();
        let mut done = 0u64;
        let mut n = 0;
        let mut last = std::time::Instant::now();
        for f in &list {
            let sys = f.system.unwrap();
            let r = rare_emu::add::copy_in(f, sys, &roms, &mut |b| {
                done += b;
                if last.elapsed().as_millis() > 300 {
                    last = std::time::Instant::now();
                    let d = done;
                    set(|s| s.copy = Some((d, total)));
                }
            });
            match r {
                Ok(_) => n += 1,
                Err(e) => log::warn!("emulation: {e}"),
            }
        }
        set(|s| {
            s.copy = None;
            s.copied = Some(n);
        });
        library_changed();
    });
}

fn size_text(b: u64) -> String {
    crate::kit::format::fmt_bytes(b)
}

// ---------------------------------------------------------------- Add games

/// Library, Emulation, Add games.
#[derive(Default)]
pub struct AddPage;

impl Drop for AddPage {
    // The network page is open only while this page is.
    fn drop(&mut self) {
        stop_session();
    }
}

impl Page for AddPage {
    fn stamp(&self, _st: &State) -> u64 {
        hash(ver())
    }

    fn view(&mut self, ui: &mut Ui, _st: &State, acts: &mut Acts) -> El {
        let t = tr();
        let mut p = page(ui, "plus", t.features.emu_add_title, "", "emu-add");
        p = p.child(ui.text_s(s().mb(1.25).max_w(60.0), trf!(t.features.emu_add_intro, folder = crate::data::model::pretty_path(&rare_emu::add::roms_dir().to_string_lossy())), T::new(Ty::Small).c(tok::fg(0.8)).wrap()));
        let (url, error, scan, scanning, copy, copied) = with(|s| (s.url.clone(), s.error.clone(), s.scan.clone(), s.scanning, s.copy, s.copied));

        // From a phone or a computer.
        p = p.child(kit::section_head(ui, s().mb(0.6), t.features.emu_add_phone, None, None));
        let mut phone = ui.bx(s().col().gap(0.5).mb(1.5).max_w(56.0)).group(Group::new("emu-add-phone", [0.0; 4]));
        match &url {
            None => {
                let b = kit::button(ui, B::new(t.features.emu_add_open, acts.tag(Act::Local(START))).sm().primary().icon(BtnIcon::Lucide("wifi", 1.0, false)));
                phone = phone.child(line(ui, t.features.emu_add_phone_title, t.features.emu_add_phone_line, Some(b), None, true));
            }
            Some(u) => {
                let qr = crate::screens::friends::bits::qr(ui, s(), u, 10.0);
                let text = ui
                    .bx(s().col().gap(0.4).grow().min0())
                    .child(ui.text(t.features.emu_add_scan, T::new(Ty::Body).w(600).wrap()))
                    .child(ui.text(u.clone(), T::new(Ty::Small).c(tok::fg(0.85)).wrap()))
                    .child(ui.text(t.features.emu_add_same_wifi, T::new(Ty::Small).c(tok::MUTED).wrap()))
                    .child(ui.bx(s().mt(0.4)).child(kit::button(ui, B::new(t.features.emu_add_close, acts.tag(Act::Local(STOP))).sm())));
                phone = phone.child(ui.bx(s().row().gap(1.25).items_start()).child(qr).child(text));
                for g in rare_emu::upload::received().iter().rev().take(8) {
                    let state = match (&g.error, g.finished) {
                        (Some(e), _) => e.clone(),
                        (None, true) => trf!(t.features.emu_add_got, system = g.system),
                        (None, false) => format!("{}%", if g.total > 0 { g.done * 100 / g.total } else { 0 }),
                    };
                    phone = phone.child(line(ui, &g.name, &state, None, None, false).key(key(&g.name)));
                }
            }
        }
        p = p.child(phone);

        // From a USB drive or an SD card.
        p = p.child(kit::section_head(ui, s().mb(0.6), t.features.emu_add_drive, None, None));
        let mut rows = Vec::new();
        let list = drives();
        if list.is_empty() {
            rows.push(line(ui, t.features.emu_add_no_drive, t.features.emu_add_no_drive_line, None, None, false));
        }
        for (i, d) in list.iter().enumerate() {
            let sub = trf!(t.features.emu_add_drive_line, size = size_text(d.total));
            rows.push(line(ui, &d.label, &sub, None, Some((acts.tag(Act::Local(DRIVE + i as u32)), t.features.emu_add_look)), url.is_none() && i == 0).key(key(&d.mount.to_string_lossy())));
        }
        if scanning {
            rows.push(line(ui, t.features.emu_add_looking, "", None, None, false));
        }
        if let Some((label, found)) = &scan {
            let known: Vec<&Found> = found.iter().filter(|f| f.system.is_some()).collect();
            let mut by: Vec<(&str, usize)> = Vec::new();
            for f in &known {
                let n = f.system.unwrap().short;
                match by.iter_mut().find(|(s, _)| *s == n) {
                    Some(e) => e.1 += 1,
                    None => by.push((n, 1)),
                }
            }
            let what = by.iter().map(|(s, n)| format!("{s} {n}")).collect::<Vec<_>>().join(", ");
            let unknown = found.len() - known.len();
            let mut sub = if known.is_empty() { t.features.emu_add_none_found.to_string() } else { what };
            if unknown > 0 {
                sub = format!("{sub}  \u{b7}  {}", trf!(t.features.emu_add_unknown, n = unknown));
            }
            let right = match (copy, copied) {
                (Some(_), _) => None,
                (None, _) if !known.is_empty() => Some(kit::button(ui, B::new(trf!(t.features.emu_add_copy_n, n = known.len(), size = size_text(known.iter().map(|f| f.bytes).sum())), acts.tag(Act::Local(COPY))).sm().primary())),
                _ => None,
            };
            rows.push(line(ui, &trf!(t.features.emu_add_on_drive, drive = label), &sub, right, None, false).key(key("found")));
            if let Some((d, total)) = copy {
                let frac = if total > 0 { d as f32 / total as f32 } else { 0.0 };
                rows.push(ui.bx(s().px(1.0)).child(super::downloads::bar(ui, None, frac, false)));
            }
            if let Some(n) = copied {
                rows.push(line(ui, &trf!(t.features.emu_add_copied, n = n), t.features.emu_add_in_library, None, None, false).key(key("copied")));
            }
        }
        p = p.child(ui.bx(s().col().gap(0.25).mb(1.5).max_w(56.0)).group(Group::new("emu-add-drive", [0.0; 4])).kids(rows));

        if let Some(e) = error {
            p = p.child(ui.text_s(s().mt(1.0).max_w(56.0), e, T::new(Ty::Small).c(tok::NEG).wrap()));
        }
        p
    }

    fn act(&mut self, _st: &mut State, fx: &mut Fx, code: u32) {
        match code {
            START => {
                fx.fb(Feedback::Select);
                start_session();
            }
            STOP => {
                fx.fb(Feedback::Select);
                stop_session();
            }
            COPY => {
                fx.fb(Feedback::Select);
                copy_found();
            }
            c if c >= DRIVE && c < DRIVE + 50 => {
                if let Some(d) = drives().get((c - DRIVE) as usize) {
                    fx.fb(Feedback::Select);
                    scan_drive(d);
                }
            }
            _ => {}
        }
    }

    fn tick(&mut self, _st: &mut State, _fx: &mut Fx, now: f64) -> Option<f64> {
        // Files arriving, a drive being read or copied: the lines move.
        let busy = with(|s| s.url.is_some() || s.scanning || s.copy.is_some());
        busy.then_some(now + 0.5)
    }
}
