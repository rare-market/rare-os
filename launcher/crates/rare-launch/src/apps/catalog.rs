//! The app catalog (`src/apps/catalog.json`, shared with the UI) and how each app opens on
//! this device: pure functions over a snapshot of what is installed, so they are tested here
//! without a handheld.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const RAW: &str = include_str!("../../../../src/apps/catalog.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Controller {
    /// Rare's webview drives the page's own buttons (webgame.js focus mode).
    Focus,
    /// The pad works as a mouse and keyboard while the app is in front (apps::pointer).
    Pointer,
    /// The app reads the controller itself; Rare sends nothing.
    Gamepad,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Way {
    pub webview: Option<String>,
    pub chrome_app: Option<String>,
    pub chrome: Option<bool>,
    pub flatpak: Option<String>,
    pub exec: Option<Vec<String>>,
    /// A steam:// URL: Steam's own UI (Big Picture), opened in the running Steam client.
    pub steam: Option<String>,
    pub label: String,
    pub controller: Option<Controller>,
    /// The key B sends in pointer mode: "escape" or "alt+left".
    pub back: Option<String>,
    pub user_agent: Option<String>,
    /// A webview app that wants Rare Wallet in its page (a Sui dapp). Webview apps open in app
    /// mode otherwise: the web game window without the wallet or the Local API.
    pub wallet: Option<bool>,
    /// A cloud game stream in Rare's window: the host decodes
    /// video on the GPU (VA-API over GL) instead of running WebGPU on Vulkan.
    pub stream: Option<bool>,
    /// Webview with controller gamepad: on these hosts the page reads the real controller through
    /// the Gamepad API; on any other host (a sign-in page) the d-pad walks the page's buttons.
    pub pad_hosts: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub icon: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub drm: bool,
    /// What playing needs (a subscription, an account), in plain words.
    #[serde(default)]
    pub plan: Option<String>,
    pub ways: Vec<Way>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Webview,
    ChromeApp,
    Chrome,
    Flatpak,
    Exec,
    Steam,
}

impl Way {
    /// The way's kind, when exactly one is set.
    pub fn kind(&self) -> Option<Kind> {
        let set = [
            (self.webview.is_some(), Kind::Webview),
            (self.chrome_app.is_some(), Kind::ChromeApp),
            (self.chrome == Some(true), Kind::Chrome),
            (self.flatpak.is_some(), Kind::Flatpak),
            (self.exec.as_ref().is_some_and(|e| !e.is_empty()), Kind::Exec),
            (self.steam.is_some(), Kind::Steam),
        ];
        let mut it = set.iter().filter(|(on, _)| *on).map(|(_, k)| *k);
        let k = it.next()?;
        it.next().is_none().then_some(k)
    }

    pub fn controller(&self) -> Controller {
        self.controller.unwrap_or(match self.kind() {
            Some(Kind::Webview) => Controller::Focus,
            Some(Kind::Steam) => Controller::Gamepad,
            _ => Controller::Pointer,
        })
    }

    pub fn back(&self) -> Back {
        match self.back.as_deref() {
            Some("alt+left") => Back::AltLeft,
            Some("escape") => Back::Escape,
            _ if matches!(self.kind(), Some(Kind::ChromeApp | Kind::Chrome)) => Back::AltLeft,
            _ => Back::Escape,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Back {
    Escape,
    AltLeft,
}

#[derive(Deserialize)]
struct File {
    apps: Vec<AppEntry>,
}

pub fn all() -> &'static [AppEntry] {
    static A: OnceLock<Vec<AppEntry>> = OnceLock::new();
    A.get_or_init(|| serde_json::from_str::<File>(RAW).map(|f| f.apps).unwrap_or_default())
}

pub fn get(id: &str) -> Option<&'static AppEntry> {
    all().iter().find(|a| a.id == id)
}

/// The id apps run under in the launcher's runner: "app:<id>" never clashes with library ids
/// (desktop-entry apps are "app:<16 hex digits>").
pub fn run_id(id: &str) -> String {
    format!("app:{id}")
}

// ---------- the device ----------

/// A Chromium-based browser. `drm`: ships Widevine, so Netflix and other protected video play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browser {
    pub name: &'static str,
    /// A Flatpak app id, or a program on PATH.
    pub id: &'static str,
    pub flatpak: bool,
    pub drm: bool,
}

/// Best first. Chromium has no Widevine; Brave asks before installing it, so it counts as none.
pub const BROWSERS: &[Browser] = &[
    Browser { name: "Google Chrome", id: "com.google.Chrome", flatpak: true, drm: true },
    Browser { name: "Google Chrome", id: "google-chrome-stable", flatpak: false, drm: true },
    Browser { name: "Google Chrome", id: "google-chrome", flatpak: false, drm: true },
    Browser { name: "Microsoft Edge", id: "com.microsoft.Edge", flatpak: true, drm: true },
    Browser { name: "Microsoft Edge", id: "microsoft-edge", flatpak: false, drm: true },
    Browser { name: "Chromium", id: "org.chromium.Chromium", flatpak: true, drm: false },
    Browser { name: "Brave", id: "com.brave.Browser", flatpak: true, drm: false },
    Browser { name: "Chromium", id: "chromium-browser", flatpak: false, drm: false },
    Browser { name: "Chromium", id: "chromium", flatpak: false, drm: false },
];

/// The Flatpak to offer when an app needs a browser and the device has none.
pub const CHROME_FLATPAK: &str = "com.google.Chrome";

/// What is installed, gathered once per status call.
#[derive(Debug, Clone, Default)]
pub struct Env {
    /// The `flatpak` command exists.
    pub flatpak: bool,
    /// Installed Flatpak app ids (user or system), among those the catalog and BROWSERS use.
    pub flatpaks: Vec<String>,
    /// Programs found on PATH, among those the catalog and BROWSERS use.
    pub programs: Vec<String>,
    pub home: PathBuf,
    /// Page zoom for Chrome windows (1.5 on the SuiPlay's 7" 1920 x 1200 screen).
    pub scale: Option<f32>,
    /// The user installation already has the flathub remote.
    pub user_flathub: bool,
}

impl Env {
    fn has_flatpak(&self, id: &str) -> bool {
        self.flatpak && self.flatpaks.iter().any(|f| f == id)
    }
    fn has_program(&self, p: &str) -> bool {
        self.programs.iter().any(|x| x == p)
    }
    pub fn browser(&self, drm: bool) -> Option<&'static Browser> {
        BROWSERS.iter().find(|b| (!drm || b.drm) && if b.flatpak { self.has_flatpak(b.id) } else { self.has_program(b.id) })
    }
}

/// Every Flatpak id and program the catalog could use, for the installed-state scan.
pub fn wanted() -> (Vec<String>, Vec<String>) {
    let mut flatpaks: Vec<String> = BROWSERS.iter().filter(|b| b.flatpak).map(|b| b.id.to_string()).collect();
    let mut programs: Vec<String> = BROWSERS.iter().filter(|b| !b.flatpak).map(|b| b.id.to_string()).collect();
    for a in all() {
        for w in &a.ways {
            flatpaks.extend(w.flatpak.clone());
            if let Some(p) = w.exec.as_ref().and_then(|e| e.first()) {
                programs.push(p.clone());
            }
            if w.steam.is_some() {
                programs.push("steam".into());
            }
        }
    }
    flatpaks.sort();
    flatpaks.dedup();
    programs.sort();
    programs.dedup();
    (flatpaks, programs)
}

// ---------- planning ----------

/// How an app opens. Spawn carries the exact command line.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// `wallet`: Rare Wallet in the page (the way asks for it); app mode otherwise.
    Webview { url: String, controller: Controller, wallet: bool, web: WebOpts },
    Spawn { program: String, args: Vec<String>, controller: Controller, back: Back },
    /// Steam's UI: the URL goes to the Steam client (started if needed), which takes the screen.
    Steam { url: String },
}

/// A webview way's window settings beyond the controller.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WebOpts {
    pub user_agent: Option<String>,
    pub stream: bool,
    pub pad_hosts: Vec<String>,
}

impl WebOpts {
    fn of(w: &Way) -> WebOpts {
        WebOpts { user_agent: w.user_agent.clone(), stream: w.stream == Some(true), pad_hosts: w.pad_hosts.clone().unwrap_or_default() }
    }
}

/// Something to install first: a Flatpak and the one-line command that installs it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub flatpak: String,
    pub name: String,
    pub command: String,
}

/// The one-line, no-sudo install command for a Flathub app in the user installation.
pub fn install_command(flatpak: &str, user_flathub: bool) -> String {
    let install = format!("flatpak install --user -y flathub {flatpak}");
    if user_flathub {
        install
    } else {
        format!("flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo && {install}")
    }
}

fn flatpak_name(id: &str) -> String {
    if let Some(b) = BROWSERS.iter().find(|b| b.id == id) {
        return b.name.to_string();
    }
    for a in all() {
        if let Some(w) = a.ways.iter().find(|w| w.flatpak.as_deref() == Some(id)) {
            return w.label.clone();
        }
    }
    id.to_string()
}

/// Whether a way can open on this device, and if not, what to install.
pub fn way_ready(app: &AppEntry, way: &Way, env: &Env) -> Result<(), Option<String>> {
    match way.kind() {
        Some(Kind::Webview) => Ok(()),
        Some(Kind::ChromeApp | Kind::Chrome) => env.browser(app.drm).map(|_| ()).ok_or_else(|| env.flatpak.then(|| CHROME_FLATPAK.to_string())),
        Some(Kind::Flatpak) => {
            let id = way.flatpak.clone().unwrap_or_default();
            if env.has_flatpak(&id) {
                Ok(())
            } else {
                Err(env.flatpak.then_some(id))
            }
        }
        Some(Kind::Exec) => {
            let p = way.exec.as_ref().and_then(|e| e.first()).cloned().unwrap_or_default();
            if Path::new(&p).is_absolute() || env.has_program(&p) {
                Ok(())
            } else {
                Err(None)
            }
        }
        Some(Kind::Steam) => env.has_program("steam").then_some(()).ok_or(None),
        None => Err(None),
    }
}

/// An app's state on this device, for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub id: String,
    /// The way A opens, if any can open now.
    pub way: Option<usize>,
    /// What the default way opens in ("Telegram Desktop", "Google Chrome", "Telegram Web").
    pub opens_in: Option<String>,
    /// Other ways that can open now ("Open in Chrome").
    pub other_ways: Vec<usize>,
    /// Install this to open the app at all (`way` is None) or the better way (`way` is a fallback).
    pub offer: Option<Offer>,
    pub controller: Option<Controller>,
}

pub fn status(app: &AppEntry, env: &Env) -> Status {
    let ready: Vec<usize> = app.ways.iter().enumerate().filter(|(_, w)| way_ready(app, w, env).is_ok()).map(|(i, _)| i).collect();
    let way = ready.first().copied();
    // The best way is not available: offer what it needs, when that is a Flatpak.
    let offer = app.ways.first().and_then(|w| way_ready(app, w, env).err().flatten()).map(|f| Offer { name: flatpak_name(&f), command: install_command(&f, env.user_flathub), flatpak: f });
    let opens_in = way.map(|i| {
        let w = &app.ways[i];
        match w.kind() {
            Some(Kind::ChromeApp | Kind::Chrome) => env.browser(app.drm).map(|b| b.name.to_string()).unwrap_or_else(|| w.label.clone()),
            _ => w.label.clone(),
        }
    });
    Status { id: app.id.clone(), way, opens_in, other_ways: ready.iter().skip(1).copied().collect(), offer, controller: way.map(|i| app.ways[i].controller()) }
}

/// Chrome's flags for a fullscreen app window (or Chrome itself when `url` is None).
pub fn chrome_args(b: &Browser, app_id: &str, url: Option<&str>, user_agent: Option<&str>, env: &Env) -> (String, Vec<String>) {
    let mut a: Vec<String> = Vec::new();
    let program = if b.flatpak {
        a.push("run".into());
        a.push(b.id.into());
        "flatpak".to_string()
    } else {
        b.id.to_string()
    };
    if let Some(u) = url {
        a.push(format!("--app={u}"));
        // Each app window keeps its own sign-in and runs as its own browser, so Close ends just
        // that app. Flatpak browsers can only write inside their own data folder.
        let dir = if b.flatpak {
            env.home.join(".var/app").join(b.id).join("data/rare-apps").join(app_id)
        } else {
            env.home.join(".local/share/rare/apps").join(app_id)
        };
        a.push(format!("--user-data-dir={}", dir.display()));
    }
    a.push("--start-fullscreen".into());
    a.push("--no-first-run".into());
    a.push("--no-default-browser-check".into());
    a.push(rare_library::browser::VIDEO_DECODE.into());
    if let Some(s) = env.scale {
        a.push(format!("--force-device-scale-factor={s}"));
    }
    if let Some(ua) = user_agent {
        a.push(format!("--user-agent={ua}"));
    }
    (program, a)
}

/// How to open `app` (with way `which`, or the default), or what to install first.
pub fn plan(app: &AppEntry, which: Option<usize>, env: &Env) -> Result<Plan, String> {
    let st = status(app, env);
    let i = match which {
        Some(i) if i < app.ways.len() => i,
        Some(_) => return Err("Unknown way to open this app".into()),
        None => st.way.ok_or_else(|| match &st.offer {
            Some(o) => format!("{} needs {}. Get it from the app's page.", app.name, o.name),
            None => format!("{} can't open on this device", app.name),
        })?,
    };
    let w = &app.ways[i];
    if let Err(need) = way_ready(app, w, env) {
        return Err(match need {
            Some(f) => format!("{} needs {}", app.name, flatpak_name(&f)),
            None => format!("{} can't open this way here", app.name),
        });
    }
    let (controller, back) = (w.controller(), w.back());
    Ok(match w.kind() {
        Some(Kind::Webview) => Plan::Webview { url: w.webview.clone().unwrap_or_default(), controller, wallet: w.wallet == Some(true), web: WebOpts::of(w) },
        Some(k @ (Kind::ChromeApp | Kind::Chrome)) => {
            let b = env.browser(app.drm).ok_or("No browser")?;
            let url = if k == Kind::ChromeApp { w.chrome_app.as_deref() } else { None };
            let (program, args) = chrome_args(b, &app.id, url, w.user_agent.as_deref(), env);
            Plan::Spawn { program, args, controller, back }
        }
        Some(Kind::Flatpak) => Plan::Spawn { program: "flatpak".into(), args: vec!["run".into(), w.flatpak.clone().unwrap_or_default()], controller, back },
        Some(Kind::Exec) => {
            let mut e = w.exec.clone().unwrap_or_default();
            let program = e.remove(0);
            Plan::Spawn { program, args: e, controller, back }
        }
        Some(Kind::Steam) => Plan::Steam { url: w.steam.clone().unwrap_or_default() },
        None => return Err("This app has no way to open".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(flatpaks: &[&str]) -> Env {
        Env { flatpak: true, flatpaks: flatpaks.iter().map(|s| s.to_string()).collect(), programs: vec![], home: PathBuf::from("/home/b"), scale: None, user_flathub: true }
    }

    #[test]
    fn catalog_is_valid() {
        let apps = all();
        let ids: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        for want in ["steam", "telegram", "x", "netflix", "chrome", "youtube", "discord", "xcloud", "geforcenow"] {
            assert!(ids.contains(&want), "{want} missing");
        }
        let mut seen = std::collections::HashSet::new();
        for a in apps {
            assert!(seen.insert(&a.id), "duplicate id {}", a.id);
            assert!(!a.ways.is_empty(), "{} has no way to open", a.id);
            assert!(a.icon.starts_with("apps/"), "{} icon", a.id);
            for w in &a.ways {
                let k = w.kind().unwrap_or_else(|| panic!("{}: a way must set exactly one kind", a.id));
                assert!(!w.label.is_empty(), "{}: way without a label", a.id);
                if a.drm {
                    assert!(matches!(k, Kind::ChromeApp | Kind::Chrome), "{}: DRM apps only open in Chrome", a.id);
                }
                if w.controller() == Controller::Focus {
                    assert_eq!(k, Kind::Webview, "{}: focus mode is for Rare's webview", a.id);
                }
                for u in [&w.webview, &w.chrome_app].into_iter().flatten() {
                    assert!(u.starts_with("https://"), "{}: {u} is not https", a.id);
                }
            }
        }
    }

    #[test]
    fn netflix_needs_chrome_with_widevine() {
        let n = get("netflix").unwrap();
        // Chromium alone can't play Netflix: offer Google Chrome.
        let st = status(n, &env(&["org.chromium.Chromium"]));
        assert_eq!(st.way, None);
        let o = st.offer.unwrap();
        assert_eq!(o.flatpak, "com.google.Chrome");
        assert_eq!(o.command, "flatpak install --user -y flathub com.google.Chrome");
        assert!(plan(n, None, &env(&["org.chromium.Chromium"])).unwrap_err().contains("Google Chrome"));

        let mut e = env(&["com.google.Chrome"]);
        e.scale = Some(1.5);
        let p = plan(n, None, &e).unwrap();
        assert_eq!(
            p,
            Plan::Spawn {
                program: "flatpak".into(),
                args: vec![
                    "run".into(),
                    "com.google.Chrome".into(),
                    "--app=https://www.netflix.com/browse".into(),
                    "--user-data-dir=/home/b/.var/app/com.google.Chrome/data/rare-apps/netflix".into(),
                    "--start-fullscreen".into(),
                    "--no-first-run".into(),
                    "--no-default-browser-check".into(),
                    rare_library::browser::VIDEO_DECODE.into(),
                    "--force-device-scale-factor=1.5".into(),
                ],
                controller: Controller::Pointer,
                back: Back::AltLeft,
            }
        );
    }

    #[test]
    fn steam_is_an_app_that_opens_big_picture() {
        let steam = get("steam").unwrap();
        let mut e = env(&[]);
        // No Steam client: it can't open, and there is nothing to install from Flathub.
        assert!(status(steam, &e).way.is_none());
        assert!(status(steam, &e).offer.is_none());
        e.programs.push("steam".into());
        let st = status(steam, &e);
        assert_eq!(st.way, Some(0));
        assert_eq!(st.controller, Some(Controller::Gamepad), "Steam reads the pad itself");
        assert_eq!(plan(steam, None, &e).unwrap(), Plan::Steam { url: "steam://open/bigpicture".into() });
        assert!(wanted().1.contains(&"steam".to_string()));
    }

    #[test]
    fn telegram_prefers_the_desktop_app() {
        let t = get("telegram").unwrap();
        // Not installed: Telegram Web in Rare, with the desktop app on offer.
        let st = status(t, &env(&[]));
        assert_eq!(st.way, Some(1));
        assert_eq!(st.opens_in.as_deref(), Some("Telegram Web"));
        assert_eq!(st.controller, Some(Controller::Focus));
        assert_eq!(st.offer.as_ref().unwrap().flatpak, "org.telegram.desktop");
        assert_eq!(plan(t, None, &env(&[])).unwrap(), Plan::Webview { url: "https://web.telegram.org/k/".into(), controller: Controller::Focus, wallet: false, web: WebOpts::default() });
        // Installed: the desktop app, nothing to offer.
        let st = status(t, &env(&["org.telegram.desktop"]));
        assert_eq!(st.way, Some(0));
        assert!(st.offer.is_none());
        assert_eq!(st.other_ways, vec![1]);
        assert_eq!(
            plan(t, None, &env(&["org.telegram.desktop"])).unwrap(),
            Plan::Spawn { program: "flatpak".into(), args: vec!["run".into(), "org.telegram.desktop".into()], controller: Controller::Pointer, back: Back::Escape }
        );
    }

    #[test]
    fn x_and_chrome() {
        let x = get("x").unwrap();
        // The webview always works; Chrome becomes a second way once a browser is there.
        assert_eq!(status(x, &env(&[])).other_ways, Vec::<usize>::new());
        assert_eq!(status(x, &env(&["org.chromium.Chromium"])).other_ways, vec![1]);
        let Plan::Spawn { program, args, .. } = plan(x, Some(1), &env(&["org.chromium.Chromium"])).unwrap() else { panic!() };
        assert_eq!(program, "flatpak");
        assert_eq!(args[1], "org.chromium.Chromium");

        let c = get("chrome").unwrap();
        let st = status(c, &env(&[]));
        assert_eq!(st.way, None);
        assert_eq!(st.offer.unwrap().command, "flatpak install --user -y flathub com.google.Chrome");
        // Chrome itself: the default profile, no app window flags.
        let Plan::Spawn { args, .. } = plan(c, None, &env(&["com.google.Chrome"])).unwrap() else { panic!() };
        assert_eq!(args, vec!["run", "com.google.Chrome", "--start-fullscreen", "--no-first-run", "--no-default-browser-check", rare_library::browser::VIDEO_DECODE]);
        // A browser on PATH runs directly and keeps its profile in Rare's folder.
        let mut e = env(&[]);
        e.programs = vec!["google-chrome-stable".into()];
        let Plan::Spawn { program, args, .. } = plan(get("youtube").unwrap(), None, &e).unwrap() else { panic!() };
        assert_eq!(program, "google-chrome-stable");
        assert!(args.contains(&"--user-data-dir=/home/b/.local/share/rare/apps/youtube".to_string()));
        assert!(args.iter().any(|a| a.starts_with("--user-agent=") && a.contains("Leanback")));
    }

    #[test]
    fn cloud_gaming_opens_in_rare_with_the_real_pad() {
        for (id, host, ua) in [("xcloud", "www.xbox.com", "Edg/"), ("geforcenow", "play.geforcenow.com", "CrOS")] {
            let a = get(id).unwrap();
            assert!(a.plan.as_deref().is_some_and(|p| !p.is_empty()), "{id}: plan note");
            // Rare's window always works; Chrome is the second way once it is installed.
            let st = status(a, &env(&[]));
            assert_eq!(st.way, Some(0));
            assert_eq!(st.controller, Some(Controller::Gamepad));
            let Plan::Webview { url, controller, wallet, web } = plan(a, None, &env(&[])).unwrap() else { panic!("{id}: not a webview") };
            assert!(url.contains(host), "{id}: {url}");
            assert_eq!(controller, Controller::Gamepad);
            assert!(!wallet && web.stream);
            assert_eq!(web.pad_hosts, vec![host.to_string()]);
            assert!(web.user_agent.unwrap().contains(ua));
            assert_eq!(status(a, &env(&["com.google.Chrome"])).other_ways, vec![1]);
        }
    }

    #[test]
    fn install_command_adds_flathub_when_missing() {
        assert_eq!(
            install_command("org.telegram.desktop", false),
            "flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo && flatpak install --user -y flathub org.telegram.desktop"
        );
        // Without flatpak at all there is nothing to offer.
        let mut e = env(&[]);
        e.flatpak = false;
        assert!(status(get("netflix").unwrap(), &e).offer.is_none());
    }
}
