//! The launcher's one controller file picker: Add a game (an .exe, AppImage, .sh or a folder), a
//! picture for a game's art, and a games folder (Settings > Storage > Add folder). Places first
//! (the Games folders, Home, Downloads, the SD card and USB drives), then the folders and the
//! files that fit, one row each. A opens a folder or picks; B goes up a folder, and closes from the
//! places. No typing.

use crate::act::Act;
use crate::kit::{self, s, tok, Track, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::state::{Overlay, State, Tone};
use rare_library::custom::{kind_of, ArtKind, PickKind};
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::{Focusable, Group};
use rare_render::view::{key, El, Motion};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What the picker is for.
#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    AddGame,
    /// A picture for this game's cover, hero or logo; back to its art page after.
    Image { id: String, kind: ArtKind },
    /// A games folder (Settings > Storage): "storage:folder:<path>" goes to the app's storage
    /// actions. A Games folder that is not there yet is made when it is picked.
    Folder,
}

static NEXT: Mutex<Option<Mode>> = Mutex::new(None);

/// Opens the picker for `mode` (the page reads it when it is built).
pub fn open(st: &mut State, mode: Mode) {
    *NEXT.lock().unwrap() = Some(mode);
    st.open(Some(Overlay::Other(super::PICK)));
}

pub fn take_mode() -> Mode {
    NEXT.lock().unwrap().take().unwrap_or(Mode::AddGame)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Place,
    Dir,
    /// The folder itself as a game.
    ThisFolder,
    /// The open folder as a games folder.
    UseFolder,
    File(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Entry {
    name: String,
    path: PathBuf,
    kind: Kind,
    /// A muted line under a place (where it is).
    sub: Option<String>,
}

fn entry(name: impl Into<String>, path: PathBuf, kind: Kind) -> Entry {
    Entry { name: name.into(), path, kind, sub: None }
}

/// The most rows listed in one folder.
const MAX: usize = 400;

const IMAGE_EXT: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

fn label_of(p: &Path, mode: &Mode) -> Option<&'static str> {
    match mode {
        Mode::AddGame => match kind_of(p)? {
            PickKind::Exe => Some("Windows game"),
            PickKind::AppImage => Some("AppImage"),
            PickKind::Script => Some("Script"),
            PickKind::Program => Some("Linux game"),
            PickKind::Folder => None,
        },
        Mode::Image { .. } => {
            let ext = p.extension()?.to_string_lossy().to_lowercase();
            IMAGE_EXT.contains(&ext.as_str()).then_some("Picture")
        }
        Mode::Folder => None,
    }
}

/// The places: Games folders first for a games folder; Home and its folders first for a file.
fn places(mode: &Mode) -> Vec<Entry> {
    use rare_library::storage::DriveKind;
    let home = rare_core::paths::home();
    let drives: Vec<_> = rare_library::storage::drives().into_iter().filter(|d| d.kind != DriveKind::Internal).collect();
    let drive_name = |d: &rare_library::storage::Drive| if d.kind == DriveKind::Sd { "SD card".to_string() } else { d.label.clone() };
    let pretty = |p: &Path| crate::data::model::pretty_path(&p.to_string_lossy());
    let place = |name: String, p: PathBuf| {
        let sub = if p.is_dir() { pretty(&p) } else { format!("{} (made when you pick it)", pretty(&p)) };
        Entry { name, path: p, kind: Kind::Place, sub: Some(sub) }
    };
    let mut v = Vec::new();
    if *mode == Mode::Folder {
        v.push(place("Games on this device".into(), home.join("Games")));
        for d in &drives {
            v.push(place(format!("Games on {}", if d.kind == DriveKind::Sd { "the SD card".to_string() } else { d.label.clone() }), d.mount.join("Games")));
        }
        v.push(place("Home folder".into(), home.clone()));
    } else {
        v.push(place("Home folder".into(), home.clone()));
        for (name, sub) in [("Games", "Games"), ("Downloads", "Downloads"), ("Pictures", "Pictures"), ("Desktop", "Desktop")] {
            let p = home.join(sub);
            if p.is_dir() {
                v.push(place(name.into(), p));
            }
        }
    }
    for d in &drives {
        v.push(place(drive_name(d), d.mount.clone()));
    }
    v
}

fn list(dir: &Path, mode: &Mode) -> Vec<Entry> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                dirs.push(entry(name, p, Kind::Dir));
            } else if let Some(l) = label_of(&p, mode) {
                files.push(entry(name, p, Kind::File(l)));
            }
        }
    }
    let by_name = |a: &Entry, b: &Entry| a.name.to_lowercase().cmp(&b.name.to_lowercase());
    dirs.sort_by(by_name);
    files.sort_by(by_name);
    let mut v = Vec::new();
    match mode {
        Mode::AddGame if rare_library::local::pick_program(dir).is_some() => v.push(entry("Add this folder as a game", dir.to_path_buf(), Kind::ThisFolder)),
        Mode::Folder => v.push(entry("Use this folder", dir.to_path_buf(), Kind::UseFolder)),
        _ => {}
    }
    // Files first: they are what the player came for.
    v.extend(files);
    v.extend(dirs);
    v.truncate(MAX);
    v
}

pub struct Picker {
    mode: Mode,
    /// None: the places.
    dir: Option<PathBuf>,
    entries: Vec<Entry>,
    /// The tag of each row in the last build, and the row focus goes to next.
    tags: Vec<u32>,
    focus_next: Option<usize>,
    busy: bool,
    ver: u64,
}

impl Picker {
    pub fn new(mode: Mode) -> Picker {
        Picker { entries: places(&mode), mode, dir: None, tags: vec![], focus_next: Some(0), busy: false, ver: 0 }
    }

    fn go(&mut self, dir: Option<PathBuf>, focus: Option<PathBuf>) {
        self.entries = match &dir {
            Some(d) => list(d, &self.mode),
            None => places(&self.mode),
        };
        self.focus_next = Some(focus.and_then(|f| self.entries.iter().position(|e| e.path == f)).unwrap_or(0));
        self.dir = dir;
        self.ver += 1;
    }

    fn title(&self) -> &'static str {
        match self.mode {
            Mode::AddGame => "Add a game",
            Mode::Image { kind: ArtKind::Cover, .. } => "Pick a cover",
            Mode::Image { kind: ArtKind::Hero, .. } => "Pick a hero image",
            Mode::Image { kind: ArtKind::Logo, .. } => "Pick a logo",
            Mode::Folder => "Pick a games folder",
        }
    }

    fn pick(&mut self, st: &mut State, fx: &mut Fx, e: Entry) {
        match self.mode.clone() {
            Mode::AddGame => {
                self.busy = true;
                self.ver += 1;
                st.toast("Adding the game and looking for its art", Tone::Info, rare_render::clock::secs());
                fx.spawn(move |b| {
                    let r = b.add_game(&e.path);
                    let lib = r.is_ok().then(|| b.scan().ok()).flatten();
                    Box::new(move |st: &mut State, now| match r {
                        Ok((id, title)) => {
                            if let Some(l) = lib {
                                st.set_library(l);
                            }
                            st.toast(format!("Added {title}"), Tone::Info, now);
                            if st.overlay == Some(Overlay::Other(super::PICK)) {
                                st.open(Some(Overlay::Game(id)));
                            }
                        }
                        Err(e) => st.toast(e, Tone::Error, now),
                    })
                });
            }
            Mode::Image { id, kind } => {
                let path = e.path.to_string_lossy().into_owned();
                super::artpage::choose(st, fx, &id, kind, path);
                super::artpage::reopen(st, &id);
            }
            Mode::Folder => {
                st.close();
                fx.run(Act::Legacy(format!("storage:folder:{}", e.path.display())));
            }
        }
    }
}

impl Page for Picker {
    fn stamp(&self, _st: &State) -> u64 {
        hash((self.ver, self.busy))
    }

    fn view(&mut self, ui: &mut Ui, _st: &State, acts: &mut Acts) -> El {
        let sub = match &self.dir {
            None => match self.mode {
                Mode::AddGame => "Pick a Windows .exe, an AppImage, a .sh script, or a game's folder".to_string(),
                Mode::Image { .. } => "Pick a PNG, JPG or WebP picture".to_string(),
                Mode::Folder => "Rare looks for one game in each folder inside it".to_string(),
            },
            Some(d) => crate::data::model::pretty_path(&d.to_string_lossy()),
        };
        let head = ui
            .bx(s().col().gap(0.25).mb(1.25))
            .child(ui.text(self.title(), T::new(Ty::Title).w(600)))
            .child(ui.text_s(s().min0(), sub, T::new(Ty::Small).c(tok::MUTED).trunc()));
        let cols = [Track::Rem(1.75), Track::Fr(1.0), Track::Rem(9.0)];
        self.tags.clear();
        let mut rows = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            let tag = acts.tag(Act::Local(i as u32));
            self.tags.push(tag);
            let (icon, right) = match &e.kind {
                Kind::Place => ("folder", String::new()),
                Kind::Dir => ("folder", "Folder".into()),
                Kind::ThisFolder | Kind::UseFolder => ("check", String::new()),
                Kind::File("Picture") => ("images", "Picture".into()),
                Kind::File(l) => ("play", l.to_string()),
            };
            let strong = matches!(e.kind, Kind::ThisFolder | Kind::UseFolder | Kind::File(_));
            let mut name = ui.bx(s().col().min0()).child(ui.text_s(s().min0(), e.name.clone(), T::new(Ty::Row).w(if strong { 600 } else { 400 }).trunc()));
            if let Some(sub) = &e.sub {
                name = name.child(ui.text_s(s().min0(), sub.clone(), T::new(Ty::Meta).c(tok::MUTED).trunc()));
            }
            let cells = vec![
                kit::cell(ui, false).child(kit::icon(ui, icon, 1.1, tok::ICON2)),
                kit::cell(ui, false).child(name),
                kit::cell(ui, true).child(ui.text(right, T::new(Ty::Small).c(tok::MUTED))),
            ];
            let hint_a = match e.kind {
                Kind::Place | Kind::Dir => "Open",
                _ => "Pick",
            };
            rows.push(kit::row(ui, &cols, tag, Some(hint_a), cells).key(key(&e.path)));
        }
        if rows.is_empty() {
            let note = match self.mode {
                Mode::AddGame => "Nothing here starts a game. B goes back a folder.",
                Mode::Image { .. } => "No pictures here. B goes back a folder.",
                Mode::Folder => "No folders in here. B goes back a folder.",
            };
            rows.push(ui.text_s(s().py(1.0), note, T::new(Ty::Body).c(tok::MUTED)));
        }
        let group = format!("gm-pick-{}", hash(&self.dir));
        let list = ui.bx(s().col().gap(0.25).max_w(60.0)).group(Group::new(&group, [0.0; 4])).kids(rows);
        ui.bx(s().col().fill().px(2.5).pt(2.0).pb(4.0)).bg(tok::BG).enter(Motion::FADE).scrolls_y(key(&("gm-pick", &self.dir))).child(head).child(list)
    }

    fn take_focus(&mut self, _cur: Option<&Focusable>) -> Option<u32> {
        let i = self.focus_next.take()?;
        self.tags.get(i).copied()
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        if b != Btn::B {
            return false;
        }
        fx.fb(Feedback::Back);
        match self.dir.clone() {
            None => match &self.mode {
                Mode::Image { id, .. } => super::artpage::reopen(st, id),
                Mode::AddGame | Mode::Folder => st.close(),
            },
            Some(d) => {
                // Up a folder, or back to the places from a place's root.
                let place = places(&self.mode).into_iter().any(|p| p.path == d);
                let up = if place { None } else { d.parent().map(Path::to_path_buf).filter(|p| p.as_os_str().len() > 1) };
                self.go(up, Some(d));
            }
        }
        true
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        if self.busy {
            return;
        }
        let Some(e) = self.entries.get(code as usize).cloned() else { return };
        match e.kind {
            Kind::Place | Kind::Dir => self.go(Some(e.path), None),
            Kind::ThisFolder | Kind::UseFolder | Kind::File(_) => self.pick(st, fx, e),
        }
    }

    fn hints(&self, _st: &State) -> Vec<Hint> {
        vec![hint(Key::B, if self.dir.is_some() { "Up a folder" } else { "Close" })]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_what_fits_files_first() {
        let d = std::env::temp_dir().join(format!("rare-pick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Sub")).unwrap();
        std::fs::write(d.join("game.exe"), b"MZ").unwrap();
        std::fs::write(d.join("notes.txt"), b"").unwrap();
        std::fs::write(d.join("cover.png"), b"").unwrap();
        let add = list(&d, &Mode::AddGame);
        let names: Vec<&str> = add.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Add this folder as a game", "game.exe", "Sub"]);
        let img = list(&d, &Mode::Image { id: "x".into(), kind: ArtKind::Cover });
        assert_eq!(img.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["cover.png", "Sub"]);
        let folder = list(&d, &Mode::Folder);
        assert_eq!(folder.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["Use this folder", "Sub"]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
