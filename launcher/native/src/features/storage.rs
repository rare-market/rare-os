//! Storage on the device: the drives and their free space, what takes the space, moving games
//! between this device and an SD card, and the SD card itself (insert and remove, format, set up
//! for games). Settings > Storage draws from `snapshot()`; workers write here and move `version()`.
//! The app drains `take_out()` every update (toasts, a rescan after a move, favourites to re-key,
//! a Games folder to add).
//!
//! The work is in rare-library (storage, movegame, sdcard); this module runs it off the UI thread
//! and closes Steam around the steps that edit Steam's library files.

use rare_library::movegame::{self, Phase};
use rare_library::sdcard::{self, FormatStage};
use rare_library::storage::{self as lst, Drive, DriveKind, Item, SdCard};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The work in progress (one at a time).
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// Steam is being closed before a step that edits its files.
    ClosingSteam { key: Option<String> },
    Move { key: String, to: String, phase: Phase, done: u64, total: u64 },
    Format(Option<FormatStage>),
    SetUp,
}

#[derive(Clone, Debug)]
pub struct Snap {
    pub drives: Vec<Drive>,
    pub sd: SdCard,
    /// The card has a registered Steam library and a Games folder.
    pub sd_set_up: bool,
    /// None until the first measuring is done.
    pub items: Option<Vec<Item>>,
    pub measuring: bool,
    pub job: Option<Job>,
    /// The first reading landed (before it, Storage shows the fixture drives on a fixture run).
    pub read: bool,
}

impl Default for Snap {
    fn default() -> Self {
        Snap { drives: Vec::new(), sd: SdCard::None, sd_set_up: false, items: None, measuring: false, job: None, read: false }
    }
}

/// What the app does for the storage workers.
#[derive(Default)]
pub struct Out {
    pub toasts: Vec<(String, bool)>,
    pub rescan: bool,
    /// Library ids that changed with a move (old, new).
    pub renames: Vec<(String, String)>,
    /// Games folders to add to the launcher's list.
    pub folders: Vec<String>,
}

static SNAP: Mutex<Option<Snap>> = Mutex::new(None);
static OUT: Mutex<Option<Out>> = Mutex::new(None);
static VERSION: AtomicU64 = AtomicU64::new(0);
static CANCEL: AtomicBool = AtomicBool::new(false);
static MEASURING: AtomicBool = AtomicBool::new(false);
static POLLING: AtomicBool = AtomicBool::new(false);

pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

pub fn snapshot() -> Snap {
    SNAP.lock().ok().and_then(|s| s.clone()).unwrap_or_default()
}

fn edit(f: impl FnOnce(&mut Snap)) {
    if let Ok(mut s) = SNAP.lock() {
        f(s.get_or_insert_with(Snap::default));
    }
    VERSION.fetch_add(1, Ordering::Relaxed);
}

fn out(f: impl FnOnce(&mut Out)) {
    if let Ok(mut o) = OUT.lock() {
        f(o.get_or_insert_with(Out::default));
    }
}

fn toast(t: impl Into<String>, err: bool) {
    let t = t.into();
    out(|o| o.toasts.push((t, err)));
}

pub fn take_out() -> Out {
    OUT.lock().ok().and_then(|mut o| o.take()).unwrap_or_default()
}

pub fn busy() -> bool {
    snapshot().job.is_some()
}

fn set_job(j: Option<Job>) {
    edit(|s| s.job = j);
}

fn spawn(w: &Waker, f: impl FnOnce() + Send + 'static) {
    let w = w.clone();
    std::thread::spawn(move || {
        f();
        w();
    });
}

/// Drives and the SD card, read now (cheap: /proc/mounts, /sys, statvfs).
fn read_drives() -> (Vec<Drive>, SdCard, bool) {
    let drives = lst::drives();
    let sd = lst::sd_card();
    let set_up = match &sd {
        SdCard::Ready { mount, .. } => sdcard::is_set_up(mount),
        _ => false,
    };
    (drives, sd, set_up)
}

/// Reads the drives and measures everything again. `fresh` walks every folder instead of trusting
/// the sizes kept from last time.
pub fn refresh(w: &Waker, fresh: bool) {
    if MEASURING.swap(true, Ordering::Relaxed) {
        return;
    }
    edit(|s| s.measuring = true);
    spawn(w, move || {
        let (drives, sd, set_up) = read_drives();
        edit(|s| {
            s.drives = drives;
            s.sd = sd;
            s.sd_set_up = set_up;
            s.read = true;
        });
        let items = rare_library::cache::load().map(|lib| {
            let mut cache = lst::SizeCache::load();
            let mut v = lst::items(&lib, &mut cache, fresh);
            cache.save();
            // Android's apps and Android itself (they live inside Android, not in a folder).
            v.extend(super::android::storage_items());
            v.sort_by(|a, b| b.bytes.cmp(&a.bytes));
            v
        });
        edit(|s| {
            if items.is_some() || s.items.is_none() {
                s.items = Some(items.unwrap_or_default());
            }
            s.measuring = false;
        });
        MEASURING.store(false, Ordering::Relaxed);
        if ANNOUNCE.swap(false, Ordering::Relaxed) {
            let (n, bytes) = snapshot().items.map(|v| (v.len(), v.iter().map(|i| i.bytes).sum::<u64>())).unwrap_or((0, 0));
            toast(format!("Measured again: {n} items, {} in all.", crate::screens::settings::storage::size_text(bytes)), false);
        }
    });
}

/// Whether two readings differ enough to redraw: a drive or the card came or went, or free space
/// moved by more than 50 MB (a download running does not redraw every poll).
fn differs(a: &[Drive], b: &[Drive]) -> bool {
    a.len() != b.len() || a.iter().zip(b).any(|(x, y)| x.device != y.device || x.mount != y.mount || x.total != y.total || x.free.abs_diff(y.free) > 50_000_000)
}

/// Watches the SD card slot and the drives' free space every two seconds on its own thread, and
/// wakes the app only when something changed. A card going in or out says so.
pub fn poll(w: &Waker) {
    if POLLING.swap(true, Ordering::Relaxed) {
        return;
    }
    let w = w.clone();
    let _ = std::thread::Builder::new().name("rare-storage-watch".into()).spawn(move || loop {
        let (drives, sd, set_up) = read_drives();
        let before = snapshot();
        let changed = before.sd != sd || before.sd_set_up != set_up || differs(&before.drives, &drives);
        let formatting = matches!(before.job, Some(Job::Format(_)) | Some(Job::SetUp));
        if before.read && !formatting {
            match (&before.sd, &sd) {
                (SdCard::None, SdCard::Ready { .. }) if set_up => toast("SD card ready", false),
                (SdCard::None, SdCard::Ready { .. }) => toast("SD card ready. Set it up for games in Settings, Storage.", false),
                (SdCard::None, SdCard::NotReady { .. }) => toast("This SD card needs formatting before it can hold games. Settings, Storage.", false),
                (SdCard::Ready { .. } | SdCard::NotReady { .. }, SdCard::None) => toast("SD card removed", false),
                _ => {}
            }
        }
        if changed || !before.read {
            let sd_moved = before.read && matches!(before.sd, SdCard::Ready { .. }) != matches!(sd, SdCard::Ready { .. });
            edit(|s| {
                s.drives = drives;
                s.sd = sd;
                s.sd_set_up = set_up;
                s.read = true;
            });
            if sd_moved && !formatting {
                // Games on the card come and go with it.
                out(|o| o.rescan = true);
            }
            w();
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    });
}

/// Closes Steam quietly when it runs. Some(true): it was running (start it again after).
fn close_steam() -> Result<bool, String> {
    if !rare_launch::launch::steam_running() {
        return Ok(false);
    }
    let _ = rare_launch::launch::host_command("steam", &["-shutdown".to_string()], "steam").status();
    for _ in 0..60 {
        if !rare_launch::launch::steam_running() {
            // Steam writes its files on the way out; give it a moment after the process ends.
            std::thread::sleep(std::time::Duration::from_millis(500));
            return Ok(true);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Err("Steam didn't close. Close it and try again.".into())
}

fn reopen_steam(was: bool) {
    if was {
        let _ = rare_launch::launch::steam_prewarm();
    }
}

/// The drive a row's game would move to: the SD card (or a USB drive) from this device, this
/// device from anywhere else. None when there is nowhere to go.
pub fn move_target<'a>(item: &Item, drives: &'a [Drive]) -> Option<&'a Drive> {
    let here = item.path.as_deref().and_then(|p| lst::drive_of(p, drives))?;
    if here.kind == DriveKind::Internal {
        drives.iter().find(|d| d.kind == DriveKind::Sd).or_else(|| drives.iter().find(|d| d.kind == DriveKind::Usb))
    } else {
        drives.iter().find(|d| d.kind == DriveKind::Internal)
    }
}

/// "the SD card", "this device", "STICK".
pub fn place(d: &Drive) -> String {
    match d.kind {
        DriveKind::Internal => "this device".into(),
        DriveKind::Sd => "the SD card".into(),
        DriveKind::Usb => d.label.clone(),
    }
}

/// Moves the game in row `key` to the other drive. The caller checked no game is running.
pub fn start_move(w: &Waker, key: &str) {
    if busy() {
        toast("Wait for the current move to finish.", false);
        return;
    }
    let snap = snapshot();
    // Every way out says why, so Move never looks like it did nothing.
    let Some(item) = snap.items.as_ref().and_then(|v| v.iter().find(|i| i.key == key)).cloned() else {
        return toast("Couldn't find that game's files on this device.", true);
    };
    if let Some(why) = item.no_move.clone() {
        return toast(why, false);
    }
    let Some(dest) = move_target(&item, &snap.drives).cloned() else {
        toast("Insert an SD card to move games to it.", false);
        return;
    };
    let Some(lib) = rare_library::cache::load() else { return toast("Couldn't read your library. Scan again from the Library tab.", true) };
    let Some(game) = item.game_id.as_ref().and_then(|id| lib.games.iter().find(|g| &g.id == id)).cloned() else {
        return toast("That game isn't in your library anymore.", true);
    };
    let plan = match movegame::plan(&game, &dest) {
        Ok(p) => p,
        Err(e) => return toast(e, true),
    };
    CANCEL.store(false, Ordering::Relaxed);
    let key = key.to_string();
    let to = place(&dest);
    let steam = matches!(plan.kind, movegame::MoveKind::Steam { .. });
    set_job(Some(if steam { Job::ClosingSteam { key: Some(key.clone()) } } else { Job::Move { key: key.clone(), to: to.clone(), phase: Phase::Copying, done: 0, total: plan.bytes } }));
    let w2 = w.clone();
    spawn(w, move || {
        let was = if steam {
            match close_steam() {
                Ok(b) => b,
                Err(e) => {
                    set_job(None);
                    return toast(e, true);
                }
            }
        } else {
            false
        };
        let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let r = movegame::run(
            &plan,
            &game,
            &mut |p| {
                // Redraw a few times a second, not on every chunk.
                if last.elapsed().as_millis() >= 250 || p.done == p.total || p.phase == Phase::Finishing {
                    last = std::time::Instant::now();
                    set_job(Some(Job::Move { key: key.clone(), to: to.clone(), phase: p.phase, done: p.done, total: p.total }));
                    w2();
                }
            },
            &CANCEL,
        );
        reopen_steam(was);
        set_job(None);
        match r {
            Ok(m) => {
                match &m.warning {
                    Some(wn) => toast(wn.clone(), true),
                    None => toast(format!("{} is on {} now.", plan.title, to), false),
                }
                edit(|s| {
                    if let Some(i) = s.items.as_mut().and_then(|v| v.iter_mut().find(|i| i.key == key)) {
                        i.path = Some(plan.to.clone());
                        if m.old_id != m.new_id {
                            i.key = m.new_id.clone();
                            i.game_id = Some(m.new_id.clone());
                        }
                    }
                });
                out(|o| {
                    o.rescan = true;
                    if m.old_id != m.new_id {
                        o.renames.push((m.old_id.clone(), m.new_id.clone()));
                    }
                });
            }
            Err(e) if CANCEL.load(Ordering::Relaxed) => {
                let _ = e;
                toast(format!("Move cancelled. {} stays where it was.", plan.title), false);
            }
            Err(e) => toast(format!("Couldn't move {}. {e} It stays where it was.", plan.title), true),
        }
        let (drives, sd, set_up) = read_drives();
        edit(|s| {
            s.drives = drives;
            s.sd = sd;
            s.sd_set_up = set_up;
        });
    });
}

pub fn cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

/// Erases and formats the card, then sets it up for games.
pub fn start_format(w: &Waker) {
    if busy() {
        return;
    }
    set_job(Some(Job::Format(None)));
    let w2 = w.clone();
    spawn(w, move || {
        let r = sdcard::format(&mut |st| {
            set_job(Some(Job::Format(Some(st))));
            w2();
        });
        if let Err(e) = r {
            set_job(None);
            return toast(e, true);
        }
        // The automount runs at the end of the script; wait for the mount to show.
        let mut mount = None;
        for _ in 0..30 {
            if let SdCard::Ready { mount: m, .. } = lst::sd_card() {
                mount = Some(m);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let Some(mount) = mount else {
            set_job(None);
            return toast(sdcard::format_error(5), true);
        };
        set_up(&mount);
    });
}

/// Adds the card as a Steam library and a Games folder.
pub fn start_set_up(w: &Waker) {
    if busy() {
        return;
    }
    let SdCard::Ready { mount, .. } = snapshot().sd else {
        return toast("Insert an SD card first.", false);
    };
    set_job(Some(Job::SetUp));
    spawn(w, move || set_up(&mount));
}

fn set_up(mount: &std::path::Path) {
    set_job(Some(Job::SetUp));
    let was = match close_steam() {
        Ok(b) => b,
        Err(e) => {
            set_job(None);
            return toast(e, true);
        }
    };
    let r = sdcard::set_up_for_games(mount);
    reopen_steam(was);
    let (drives, sd, set_up) = read_drives();
    edit(|s| {
        s.drives = drives;
        s.sd = sd;
        s.sd_set_up = set_up;
        s.job = None;
    });
    match r {
        Ok((_, games)) => {
            toast("The SD card is ready for games. Steam can install to it, and games you put in its Games folder show up in Rare.", false);
            let g = games.to_string_lossy().into_owned();
            out(|o| {
                o.folders.push(g);
                o.rescan = true;
            });
        }
        Err(e) => toast(format!("Couldn't set up the SD card. {e}"), true),
    }
}

/// Empties one of Rare's caches.
pub fn clear_cache(w: &Waker, key: &str) {
    let key = key.to_string();
    spawn(w, move || match lst::clear_cache(&key) {
        Ok(freed) => {
            let z = crate::kit::format::fmt_size(Some(freed as f64));
            toast(if z.is_empty() { "Cleared the image cache.".to_string() } else { format!("Cleared {z}.") }, false);
            edit(|s| {
                if let Some(v) = s.items.as_mut() {
                    for i in v.iter_mut().filter(|i| i.key == format!("cache:{key}")) {
                        i.bytes = 0;
                    }
                }
            });
        }
        Err(e) => toast(e, true),
    });
}

/// The Storage section was drawn on the device: measure when the last numbers are old.
pub fn want_fresh() {
    WANT.store(true, Ordering::Relaxed);
}

static WANT: AtomicBool = AtomicBool::new(false);

struct Tick {
    measured: f64,
    seen: u64,
    library: u64,
    stale: bool,
}

static TICK: Mutex<Tick> = Mutex::new(Tick { measured: f64::NEG_INFINITY, seen: 0, library: 0, stale: false });

/// Every update from the app: starts the card slot watcher once, measures when Storage wants it, and says when
/// the open Settings page should redraw (the numbers moved).
/// `library`: the library's version (an uninstall or a scan measures again when Storage is open).
pub fn app_tick(w: &Waker, now: f64, library: u64) -> bool {
    let Ok(mut t) = TICK.lock() else { return false };
    poll(w);
    t.stale |= library != t.library;
    t.library = library;
    if WANT.swap(false, Ordering::Relaxed) && (now - t.measured > 60.0 || t.stale) && snapshot().job.is_none() {
        t.measured = now;
        t.stale = false;
        refresh(w, false);
    }
    let v = version();
    let moved = v != t.seen;
    t.seen = v;
    moved
}

/// Measure again now (the button).
pub fn measure(w: &Waker, now: f64) {
    if let Ok(mut t) = TICK.lock() {
        t.measured = now;
    }
    if MEASURING.load(Ordering::Relaxed) {
        toast("Still measuring.", false);
        return;
    }
    // Measure again says it started and when it is done (the sizes often come back the same).
    toast("Measuring again.", false);
    ANNOUNCE.store(true, Ordering::Relaxed);
    refresh(w, true);
}

/// The next measuring that ends says so (Measure again was pressed).
static ANNOUNCE: AtomicBool = AtomicBool::new(false);

/// Redraw Storage (a sort or Show all changed).
pub fn redraw() {
    VERSION.fetch_add(1, Ordering::Relaxed);
}

/// Deletes a prefix a removed game left behind (rare-library's guard decides what may go).
pub fn delete_prefix(w: &Waker, path: &str) {
    let path = path.to_string();
    spawn(w, move || match lst::delete_prefix(std::path::Path::new(&path)) {
        Ok(freed) => {
            toast(format!("Deleted {}.", crate::screens::settings::storage::size_text(freed)), false);
            edit(|s| {
                if let Some(v) = s.items.as_mut() {
                    v.retain(|i| i.key != format!("orphan:{path}"));
                }
            });
        }
        Err(e) => toast(e, true),
    });
}
