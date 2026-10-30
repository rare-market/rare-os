//! First run on boot (App.tsx opens it when `onboarded` is not set in launcher.json). The steps are
//! WS14's screens/firstrun (welcome, Wi-Fi, sign in, Steam); this is the page around them: the
//! saved step, the worker answers (NetworkManager, Steam's account), the keyboard for the Wi-Fi
//! password and the finish. The Quick key opens Quick settings over setup and comes back here.

use super::config;
use crate::act::Act;
use crate::page::{hash, Acts, Fx, Page};
use crate::screens::firstrun::{controller, emulators, passkey, picture, shell, steam, stores, time, wifi};
use crate::screens::wallet::home::stack::{PasskeyMode, WalletView};
use crate::screens::wallet::onboard::passkey_flow::Step as FlowStep;
use crate::screens::wallet::render::{build, Built};
use crate::state::{Overlay, State, Tab, Tone};
use rare_render::view::El;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};

/// Quick settings was opened from setup: closing it reopens setup.
static FROM_SETUP: AtomicBool = AtomicBool::new(false);
/// Setup handed the account step to the Wallet tab's own pages: setup comes back when they end.
/// First run was on screen this session and has not finished.
static IN_SETUP: AtomicBool = AtomicBool::new(false);

/// First run is under way: the Rare button over what setup opened (Steam to sign in) comes back
/// to setup instead of the Rare menu.
pub fn in_setup() -> bool {
    IN_SETUP.load(Ordering::Relaxed) && !config::get_bool("onboarded")
}

/// 0 no, 1 asked for, 2 the Wallet tab is up with them.
static IN_WALLET: AtomicU8 = AtomicU8::new(0);

/// Setup's account step is on the Wallet tab's create or sign-in pages: they are still setup (no
/// tabs, their own titles), and setup comes back when they end.
pub fn wallet_from_setup() -> bool {
    IN_WALLET.load(Ordering::Relaxed) != 0 && !config::get_bool("onboarded")
}

const PW_KEY: &str = "firstrun-wifi-password";

/// Setup is not done: the web launcher's rule (`!c.onboarded && !nosignin`, or `?firstrun`).
pub fn boot(st: &mut State) {
    let want = std::env::var("RR_FIRSTRUN").is_ok();
    let skip = std::env::var("RR_NOSIGNIN").is_ok() || std::env::var("RR_SHOT").is_ok() || std::env::var("RR_OPEN").is_ok();
    if want || (!skip && !config::get_bool("onboarded")) {
        st.open(Some(Overlay::Other("firstrun")));
    }
}

/// Every update: Quick settings opened from setup closed, or the wallet's create or sign-in pages
/// ended (an account was added, or B left them, or another tab was picked), so setup comes back:
/// at the picture with an account, else at the account step again.
pub fn tick(st: &mut State) {
    if FROM_SETUP.load(Ordering::Relaxed) && st.overlay.is_none() {
        FROM_SETUP.store(false, Ordering::Relaxed);
        st.open(Some(Overlay::Other("firstrun")));
    }
    let w = IN_WALLET.load(Ordering::Relaxed);
    if w == 1 && st.tab == Tab::Wallet && crate::screens::wallet::page::flow_open() {
        IN_WALLET.store(2, Ordering::Relaxed);
    } else if (w == 2 || (w == 1 && st.tab == Tab::Wallet)) && st.overlay.is_none() && (st.tab != Tab::Wallet || !crate::screens::wallet::page::flow_open()) {
        IN_WALLET.store(0, Ordering::Relaxed);
        if config::get_bool("onboarded") {
            return;
        }
        let step = if crate::data::account::current().is_some() { "picture" } else { "passkey" };
        config::set(&[("onboardingStep", step.into())]);
        st.open(Some(Overlay::Other("firstrun")));
    }
}

// ------------------------------------------------------------------ the system (src-tauri/onboard.rs)

fn nmcli(args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("nmcli");
    cmd.args(args);
    rare_launch::launch::host_env(&mut cmd);
    let out = cmd.output().map_err(|e| format!("nmcli: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(err.strip_prefix("Error: ").unwrap_or(&err).to_string())
    }
}

/// One line of `nmcli -t`: ':' separates, '\' escapes.
fn split_terse(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    out.last_mut().unwrap().push(n);
                }
            }
            ':' => out.push(String::new()),
            _ => out.last_mut().unwrap().push(c),
        }
    }
    out
}

fn parse_wifi(text: &str) -> Vec<wifi::WifiNet> {
    let mut nets: Vec<wifi::WifiNet> = Vec::new();
    for line in text.lines() {
        let f = split_terse(line);
        if f.len() < 4 || f[1].is_empty() {
            continue;
        }
        let net = wifi::WifiNet { ssid: f[1].clone(), signal: f[2].parse().unwrap_or(0), secure: !f[3].trim().is_empty() && f[3].trim() != "--", active: f[0].trim() == "*" };
        match nets.iter_mut().find(|n| n.ssid == net.ssid) {
            Some(n) => {
                if net.active || (!n.active && net.signal > n.signal) {
                    *n = net;
                }
            }
            None => nets.push(net),
        }
    }
    nets.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
    nets
}

fn wifi_scan() -> Result<Vec<wifi::WifiNet>, String> {
    nmcli(&["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list", "--rescan", "auto"]).map(|t| parse_wifi(&t))
}

fn wifi_connect(ssid: &str, password: Option<&str>) -> Result<(), String> {
    let mut args = vec!["dev", "wifi", "connect", ssid];
    if let Some(p) = password.filter(|p| !p.is_empty()) {
        args.push("password");
        args.push(p);
    }
    nmcli(&args).map(|_| ())
}

fn net_status() -> wifi::Net {
    let managed = nmcli(&["-t", "-f", "RUNNING", "general"]).map(|s| s.trim() == "running").unwrap_or(false);
    let online = managed && nmcli(&["networking", "connectivity", "check"]).map(|s| s.trim() == "full").unwrap_or(false);
    let wifi = if managed {
        nmcli(&["-t", "-f", "ACTIVE,SSID", "dev", "wifi"])
            .ok()
            .and_then(|out| out.lines().find_map(|l| l.strip_prefix("yes:").map(|s| split_terse(s).join(":"))))
            .filter(|s| !s.is_empty())
    } else {
        None
    };
    wifi::Net { online, wifi, managed }
}

/// The Steam account that signed in last (loginusers.vdf).
pub fn steam_account() -> Option<String> {
    for root in rare_library::steam::find_roots() {
        let Ok(text) = std::fs::read_to_string(root.path.join("config/loginusers.vdf")) else { continue };
        let v = rare_library::vdf::parse(&text);
        let users = v.get("users").map(|u| u.entries().to_vec()).unwrap_or_default();
        let pick = users
            .iter()
            .find(|(_, u)| u.str("MostRecent") == Some("1"))
            .or_else(|| users.first())
            .and_then(|(_, u)| u.str("PersonaName").or_else(|| u.str("AccountName")).map(String::from));
        if pick.is_some() {
            return pick;
        }
    }
    None
}

/// Shots and tests: two systems with games, one emulator installed.
fn emu_fixture() -> Vec<rare_emu::setup::Choice> {
    let games: &[(&str, usize)] = &[("retroarch", 12), ("dolphin", 3), ("pcsx2", 1)];
    rare_emu::catalog::EMULATORS
        .iter()
        .map(|e| {
            let roms = games.iter().find(|(id, _)| *id == e.id).map(|g| g.1).unwrap_or(0);
            let installed = e.id == "ppsspp";
            rare_emu::setup::Choice { id: e.id.into(), name: e.name.into(), plays: e.plays.into(), size_mb: e.size_mb, installed, roms, picked: !installed && roms > 0 }
        })
        .collect()
}

// ------------------------------------------------------------------ the page

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Sh(shell::Msg),
    Wifi(wifi::Msg),
    Steam(steam::Msg),
    Pass(passkey::Msg),
    Pic(picture::Msg),
    Emu(emulators::Msg),
    Time(time::Msg),
    Stores(stores::Msg),
    Pad(controller::Msg),
}

enum Ans {
    Net(wifi::Net),
    Scan(Result<Vec<wifi::WifiNet>, String>),
    Joined(String, Result<(), String>),
    Skip(usize, bool),
    Account(Option<String>),
    Online(bool),
    Emus(Vec<rare_emu::setup::Choice>),
    Zone(Option<String>),
    Stores(rare_stores::Accounts),
}

pub struct FirstRunPage {
    sh: shell::State,
    wifi: wifi::State,
    /// None until Steam answered
    account: Option<Option<String>>,
    opened: bool,
    online: bool,
    pending_ssid: Option<String>,
    built: Built<Msg>,
    ver: u64,
    tx: Sender<Ans>,
    rx: Receiver<Ans>,
    next_poll: f64,
    next_soon: Option<f64>,
    entered: Option<usize>,
    /// The emulator choice (None until read) and what is ticked.
    emus: Option<Vec<rare_emu::setup::Choice>>,
    picked: Vec<String>,
    /// the system's time zone (read on a worker), and whether it changed on the time step
    zone: Option<String>,
    zone_picked: bool,
    store_accounts: rare_stores::Accounts,
    pad_check: controller::State,
}

impl Default for FirstRunPage {
    fn default() -> Self {
        IN_SETUP.store(true, Ordering::Relaxed);
        let saved = std::env::var("RR_STEP").ok().or_else(|| config::get_str("onboardingStep"));
        let (tx, rx) = channel();
        FirstRunPage { sh: shell::State::resume(saved.as_deref()), wifi: wifi::State::default(), account: None, opened: false, online: true, pending_ssid: None, built: Built::default(), ver: 0, tx, rx, next_poll: 0.0, next_soon: None, entered: None, emus: None, picked: Vec::new(), zone: None, zone_picked: false, store_accounts: rare_stores::Accounts::default(), pad_check: controller::State::default() }
    }
}

fn now() -> f64 {
    rare_render::clock::secs()
}

impl FirstRunPage {
    fn work(&self, f: impl FnOnce() -> Ans + Send + 'static) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
    }

    /// A step came up: reset what it shows and start what it reads.
    fn enter(&mut self) {
        if self.entered == Some(self.sh.i) {
            return;
        }
        self.entered = Some(self.sh.i);
        self.next_poll = 0.0;
        self.next_soon = None;
        match self.sh.step() {
            "wifi" => self.wifi = wifi::State::default(),
            "passkey" => self.work(|| Ans::Online(net_status().online)),
            // resumed here without an account (the wallet was left before one was added): on to Steam
            "picture" if crate::data::account::current().is_none() && std::env::var("RR_FIXTURE").is_err() => {
                if let Some(k) = shell::STEPS.iter().position(|s| *s == "steam") {
                    self.sh.i = k;
                    if !config::get_bool("onboarded") {
                        config::set(&[("onboardingStep", "steam".into())]);
                    }
                    return self.enter();
                }
            }
            "steam" => {
                self.account = None;
                self.work(|| Ans::Online(net_status().online));
            }
            "emulators" => {
                if std::env::var("RR_FIXTURE").is_ok() {
                    let c = emu_fixture();
                    self.picked = c.iter().filter(|x| x.picked).map(|x| x.id.clone()).collect();
                    self.emus = Some(c);
                } else {
                    self.emus = None;
                    self.work(|| Ans::Emus(rare_emu::setup::choices()));
                }
            }
            "time" => {
                self.zone_picked = false;
                self.work(|| Ans::Zone(rare_system::support::time_info().zone));
            }
            "stores" => self.work(|| Ans::Stores(rare_stores::accounts())),
            "controller" => self.pad_check = controller::State::default(),
            _ => {}
        }
        self.ver += 1;
    }

    /// Starts the setup for these emulators (in the background; Library, Emulation shows it).
    fn set_up(&mut self, st: &mut State, fx: &mut Fx, ids: Vec<String>) {
        config::set(&[("emuSetup", "chosen".into())]);
        if !ids.is_empty() {
            crate::features::emulation::start_setup(Some(ids));
        }
        self.handle(st, fx, Msg::Sh(shell::Msg::Next));
    }

    fn shell_effect(&mut self, st: &mut State, fx: &mut Fx, mut e: Option<shell::Effect>) {
        while let Some(eff) = e.take() {
            match eff {
                shell::Effect::ShouldSkip(k) => {
                    if matches!(shell::STEPS[k], "wifi" | "stores") {
                        // Wi-Fi when online, the stores when offline: nothing to do there
                        let wifi = shell::STEPS[k] == "wifi";
                        self.work(move || Ans::Skip(k, net_status().online == wifi));
                    } else if shell::STEPS[k] == "picture" {
                        // only with an account to give it to
                        e = shell::skip_answer(&mut self.sh, k, crate::data::account::current().is_none());
                    } else {
                        e = shell::skip_answer(&mut self.sh, k, false);
                    }
                }
                shell::Effect::SaveStep(s) => {
                    if !config::get_bool("onboarded") {
                        config::set(&[("onboardingStep", s.into())]);
                    }
                    self.enter();
                }
                shell::Effect::SetLanguage(c) => config::set(&[("language", c.into())]),
                shell::Effect::Sound(_) => {}
                shell::Effect::Finish => {
                    config::set(&[("onboarded", true.into()), ("onboardingStep", serde_json::Value::Null)]);
                    IN_SETUP.store(false, Ordering::Relaxed);
                    fx.run(Act::Scan);
                    st.close();
                    st.toast("Press the Rare button in any game for the Rare menu", Tone::Info, now());
                }
            }
        }
        self.ver += 1;
    }

    fn wifi_effect(&mut self, st: &mut State, fx: &mut Fx, e: Option<wifi::Effect>) {
        match e {
            Some(wifi::Effect::Next) => {
                let e = shell::update(&mut self.sh, shell::Msg::Next);
                self.shell_effect(st, fx, e);
            }
            Some(wifi::Effect::Back) => {
                let e = shell::update(&mut self.sh, shell::Msg::Back);
                self.shell_effect(st, fx, e);
            }
            Some(wifi::Effect::Scan) => self.work(|| Ans::Scan(wifi_scan())),
            Some(wifi::Effect::AskPassword(ssid)) => {
                st.ask_secret(PW_KEY, format!("Password for {ssid}"), None);
                self.pending_ssid = Some(ssid);
            }
            Some(wifi::Effect::Connect { ssid, password }) => self.work(move || {
                let r = wifi_connect(&ssid, password.as_deref());
                Ans::Joined(ssid, r)
            }),
            Some(wifi::Effect::NextSoon) => self.next_soon = Some(now() + 1.4),
            None => {}
        }
        self.ver += 1;
    }

    fn handle(&mut self, st: &mut State, fx: &mut Fx, m: Msg) {
        match m {
            Msg::Sh(m) => {
                let e = shell::update(&mut self.sh, m);
                self.shell_effect(st, fx, e);
            }
            Msg::Wifi(m) => {
                let e = wifi::update(&mut self.wifi, m);
                self.wifi_effect(st, fx, e);
            }
            Msg::Steam(steam::Msg::Open) => {
                self.opened = true;
                self.ver += 1;
                fx.spawn(|b| {
                    let r = b.steam_big_picture();
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            st.toast(e, Tone::Error, now);
                        }
                    })
                });
            }
            Msg::Steam(steam::Msg::Continue) => self.handle(st, fx, Msg::Sh(shell::Msg::Next)),
            Msg::Steam(steam::Msg::Back) => self.handle(st, fx, Msg::Sh(shell::Msg::Back)),
            Msg::Pass(passkey::Msg::Next) => self.handle(st, fx, Msg::Sh(shell::Msg::Next)),
            Msg::Pass(passkey::Msg::Back) => self.handle(st, fx, Msg::Sh(shell::Msg::Back)),
            Msg::Pass(passkey::Msg::Mode(mode)) => {
                let v = match mode {
                    passkey::Mode::Create => WalletView::Passkey { mode: PasskeyMode::Create },
                    _ => WalletView::Passkey { mode: PasskeyMode::Signin },
                };
                self.leave_for_wallet(st, fx, v);
            }
            Msg::Pass(passkey::Msg::Phrase) => self.leave_for_wallet(st, fx, WalletView::CreatePhrase),
            Msg::Pass(passkey::Msg::Other) => self.leave_for_wallet(st, fx, WalletView::Add),
            Msg::Pass(_) => {}
            // A on a picture uses it and moves on (one press; Continue keeps the one marked).
            Msg::Pic(picture::Msg::Use(id)) => {
                crate::data::live::pick_default(&id);
                self.handle(st, fx, Msg::Sh(shell::Msg::Next));
            }
            Msg::Pic(picture::Msg::Continue) => self.handle(st, fx, Msg::Sh(shell::Msg::Next)),
            Msg::Pic(picture::Msg::Back) => self.handle(st, fx, Msg::Sh(shell::Msg::Back)),
            Msg::Emu(emulators::Msg::Toggle(id)) => {
                if let Some(k) = self.picked.iter().position(|x| *x == id) {
                    self.picked.remove(k);
                } else {
                    self.picked.push(id);
                }
                self.ver += 1;
            }
            Msg::Emu(emulators::Msg::Now) => {
                let ids = self.picked.clone();
                self.set_up(st, fx, ids);
            }
            Msg::Emu(emulators::Msg::All) => {
                let ids = self.emus.as_ref().map(|c| c.iter().filter(|x| !x.installed).map(|x| x.id.clone()).collect()).unwrap_or_default();
                self.set_up(st, fx, ids);
            }
            Msg::Emu(emulators::Msg::Later) => {
                if config::get_str("emuSetup").is_none() {
                    config::set(&[("emuSetup", "later".into())]);
                }
                self.handle(st, fx, Msg::Sh(shell::Msg::Next));
            }
            Msg::Emu(emulators::Msg::Back) => self.handle(st, fx, Msg::Sh(shell::Msg::Back)),
            // Settings' own picker opens over setup; the step reads the zone again while it is up
            Msg::Time(time::Msg::Pick) => fx.run(Act::Legacy("support:tz-open".into())),
            Msg::Time(time::Msg::Continue) | Msg::Stores(stores::Msg::Continue) | Msg::Pad(controller::Msg::Continue) => self.handle(st, fx, Msg::Sh(shell::Msg::Next)),
            Msg::Time(time::Msg::Back) | Msg::Stores(stores::Msg::Back) | Msg::Pad(controller::Msg::Back) => self.handle(st, fx, Msg::Sh(shell::Msg::Back)),
            Msg::Stores(stores::Msg::SignIn(k)) => fx.run(Act::Legacy(format!("stores:signin:{k}"))),
        }
    }

    /// The wallet's own screens take the passkey flow; setup resumes at the picture after it.
    fn leave_for_wallet(&mut self, st: &mut State, fx: &mut Fx, v: WalletView) {
        config::set(&[("onboardingStep", "picture".into())]);
        IN_WALLET.store(1, Ordering::Relaxed);
        crate::screens::wallet::page::open_on(v);
        st.close();
        fx.run(Act::Tab(Tab::Wallet));
    }
}

impl Page for FirstRunPage {
    fn stamp(&self, st: &State) -> u64 {
        hash((self.ver, st.library.as_ref().map(|l| l.games.len())))
    }

    fn view(&mut self, ui: &mut crate::kit::Ui, st: &State, acts: &mut Acts) -> El {
        self.enter();
        let main: Option<crate::screens::wallet::markup::El<Msg>> = match self.sh.step() {
            "wifi" => Some(wifi::view(&self.wifi).map(&Msg::Wifi)),
            "passkey" => Some(passkey::view(passkey::Mode::Start, FlowStep::Choose, self.online, None, None).map(&Msg::Pass)),
            "picture" => {
                let current = crate::data::account::current().map(|(a, _)| crate::avatars::id_of_src(&crate::data::live::picture(&a)).unwrap_or_default().to_string()).unwrap_or_default();
                Some(picture::view(&current).map(&Msg::Pic))
            }
            "steam" => {
                let games = st.library.as_ref().map(|l| l.games.iter().filter(|g| g.source == "steam").count()).unwrap_or(0);
                Some(steam::view(self.account.clone().flatten().as_deref(), self.opened, self.online, games).map(&Msg::Steam))
            }
            "emulators" => Some(match &self.emus {
                Some(c) => emulators::view(c, &self.picked).map(&Msg::Emu),
                None => crate::screens::firstrun::kit::step(crate::screens::wallet::markup::text("Emulators"), None, vec![crate::screens::firstrun::kit::working(true, Some("Looking for your games"))], vec![], false),
            }),
            "time" => Some(time::view(self.zone.as_deref(), &crate::screens::shell::clock_text(), self.zone_picked).map(&Msg::Time)),
            "stores" => {
                let a = &self.store_accounts;
                let rows = [stores::Row { key: "epic", name: "Epic Games", account: a.epic.as_deref() }, stores::Row { key: "gog", name: "GOG", account: a.gog.as_deref() }];
                Some(stores::view(&rows, self.online).map(&Msg::Stores))
            }
            "controller" => Some(controller::view(&self.pad_check).map(&Msg::Pad)),
            _ => None,
        };
        // The welcome page shows ACT's key art while ACT is live in the bundled catalog (FirstRun.tsx).
        static ACT_LIVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let act_live = *ACT_LIVE.get_or_init(|| crate::screens::sui::catalog::seed().iter().any(|g| g.slug == "act" && crate::screens::sui::catalog::is_active(g)));
        let mut m = shell::view_with(&self.sh, main, act_live, &Msg::Sh);
        m.class = m.class.replace("fixed inset-0 z-40", "h-full w-full");
        let (el, built) = build(ui, acts, &m);
        self.built = built;
        el
    }

    fn hints(&self, _st: &State) -> Vec<rare_render::focus::hints::Hint> {
        use rare_render::focus::hints::{hint, Key};
        let mut v = Vec::new();
        if self.sh.i > 0 && !self.sh.finishing {
            v.push(hint(Key::B, "Back"));
        } else if self.sh.i == 0 && config::get_bool("onboarded") {
            v.push(hint(Key::B, "Back"));
        }
        // the controller check lights Y instead
        if self.sh.step() != "controller" {
            v.push(hint(Key::Y, "Skip setup"));
        }
        v
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: crate::page::Btn, _cur: Option<(&rare_render::focus::Focusable, Option<&Act>)>) -> bool {
        use crate::page::Btn;
        // The controller check lights what it gets; the d-pad still moves the highlight.
        if self.sh.step() == "controller" && !self.sh.finishing {
            let key = match b {
                Btn::X => "x",
                Btn::Y => "y",
                Btn::Lb => "lb",
                Btn::Rb => "rb",
                Btn::Lt => "lt",
                Btn::Rt => "rt",
                Btn::Select => "select",
                Btn::Start => "start",
                Btn::Up => "up",
                Btn::Down => "down",
                Btn::Left => "left",
                Btn::Right => "right",
                _ => "",
            };
            if controller::press(&mut self.pad_check, key) {
                self.ver += 1;
                return !matches!(b, Btn::Up | Btn::Down | Btn::Left | Btn::Right);
            }
        }
        // Setup opened again from Settings (Run setup): B on its first page goes back to Settings.
        if b == Btn::B && self.sh.i == 0 && config::get_bool("onboarded") {
            st.close();
            return true;
        }
        let name = match b {
            Btn::B => "b",
            Btn::Y => "y",
            Btn::Lb => "lb",
            Btn::Rb => "rb",
            Btn::Start => "start",
            Btn::Guide => "guide",
            Btn::Qam => {
                FROM_SETUP.store(true, Ordering::Relaxed);
                return false;
            }
            _ => return false,
        };
        if let Some(m) = self.built.pad(name) {
            self.handle(st, fx, m);
        }
        true
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        if let Some(m) = self.built.msg(code) {
            self.handle(st, fx, m);
        }
    }

    fn tick(&mut self, st: &mut State, fx: &mut Fx, t: f64) -> Option<f64> {
        if let Some(ans) = st.take_answer(PW_KEY) {
            if let Some(ssid) = self.pending_ssid.take() {
                let e = wifi::password(&mut self.wifi, ssid, ans);
                self.wifi_effect(st, fx, e);
            }
        }
        while let Ok(a) = self.rx.try_recv() {
            match a {
                Ans::Net(n) => {
                    self.online = n.online;
                    let e = wifi::status(&mut self.wifi, n);
                    self.wifi_effect(st, fx, e);
                }
                Ans::Scan(r) => wifi::scanned(&mut self.wifi, r),
                Ans::Joined(ssid, r) => {
                    wifi::joined(&mut self.wifi, &ssid, r);
                    self.work(|| Ans::Net(net_status()));
                }
                Ans::Skip(k, skip) => {
                    let e = shell::skip_answer(&mut self.sh, k, skip);
                    self.shell_effect(st, fx, e);
                }
                Ans::Account(a) => self.account = Some(a),
                Ans::Online(o) => {
                    self.online = o;
                    if self.sh.step() == "stores" && !o {
                        self.store_accounts = rare_stores::Accounts::default();
                    }
                }
                Ans::Zone(z) => {
                    if self.zone.is_some() && z != self.zone {
                        self.zone_picked = true;
                    }
                    self.zone = z;
                }
                Ans::Stores(a) => self.store_accounts = a,
                Ans::Emus(c) => {
                    self.picked = c.iter().filter(|x| x.picked).map(|x| x.id.clone()).collect();
                    self.emus = Some(c);
                }
            }
            self.ver += 1;
        }
        if self.next_soon.is_some_and(|at| t >= at) {
            self.next_soon = None;
            let e = shell::update(&mut self.sh, shell::Msg::Next);
            self.shell_effect(st, fx, e);
        }
        // Wi-Fi status every 3 s; Steam's account every 3 s (coming back from Steam looks again).
        if t >= self.next_poll {
            match self.sh.step() {
                "wifi" => {
                    self.next_poll = t + 3.0;
                    self.work(|| Ans::Net(net_status()));
                }
                "steam" => {
                    self.next_poll = t + 3.0;
                    self.work(|| Ans::Account(steam_account()));
                }
                // the picker sets it over the step; a sign-in window ends with the account saved
                "time" => {
                    self.next_poll = t + 1.0;
                    self.work(|| Ans::Zone(rare_system::support::time_info().zone));
                }
                "stores" => {
                    self.next_poll = t + 2.0;
                    self.work(|| Ans::Stores(rare_stores::accounts()));
                }
                _ => self.next_poll = t + 3.0,
            }
        }
        Some(self.next_soon.map_or(self.next_poll, |n| n.min(self.next_poll)).min(t + 0.25))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nmcli() {
        let nets = parse_wifi(" :Home:60:WPA2\n*:Home:80:WPA2\n :Cafe:90:\n");
        assert_eq!(nets[0].ssid, "Home");
        assert!(nets[0].active);
        assert_eq!(nets.len(), 2);
    }

    #[test]
    fn steps_resume() {
        std::env::set_var("RR_STEP", "steam");
        let p = FirstRunPage::default();
        assert_eq!(p.sh.step(), "steam");
        std::env::remove_var("RR_STEP");
    }
}
