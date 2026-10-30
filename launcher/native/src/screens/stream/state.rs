//! PC streaming state (stream/state.ts): PCs, their games, pairing, Moonlight's install, and Play /
//! Resume / Quit as a plain state machine. The backend calls sit behind `StreamApi` (rare-stream
//! replaces the fixture later). Everything that waited on a timer in the web (the 1 s pairing poll,
//! the 3 s wake poll, the 1.5 s launching flag) is driven by `tick(now)` instead.

use super::logic::*;

/// The calls state.ts makes (`api` there); every one can fail with the native error text.
pub trait StreamApi {
    fn status(&mut self) -> Result<Status, String>;
    fn refresh(&mut self) -> Result<Vec<Host>, String>;
    fn discover(&mut self) -> Result<Vec<Host>, String>;
    fn add(&mut self, address: &str) -> Result<Host, String>;
    fn pair_start(&mut self, address: &str) -> Result<PairView, String>;
    fn pair_poll(&mut self) -> Result<Option<PairView>, String>;
    fn pair_cancel(&mut self);
    fn apps(&mut self, uuid: &str) -> Result<Vec<StreamApp>, String>;
    fn launch(&mut self, uuid: &str, app: u32, replace: bool) -> Result<(), String>;
    fn resume(&mut self, id: &str) -> Result<(), String>;
    fn quit(&mut self, uuid: &str) -> Result<(), String>;
    fn wake(&mut self, uuid: &str) -> Result<(), String>;
    fn set_settings(&mut self, s: &StreamSettings) -> Result<(), String>;
    fn install(&mut self) -> Result<(), String>;
    /// The launcher's running list (run ids), kept by the runner.
    fn running(&mut self) -> Vec<String>;
    /// A second handle for a worker thread, so a slow call (launch, quit, resume) never blocks the
    /// screen. None means the calls run in place (the fixtures).
    fn fork(&self) -> Option<Box<dyn StreamApi + Send>> {
        None
    }
    /// Moonlight's install progress since the last ask: (progress, done, error).
    fn install_poll(&mut self) -> Option<(Option<f32>, bool, Option<String>)> {
        None
    }
}

/// A finished background call: applies its result to the state on the UI thread.
type Job = Box<dyn FnOnce(&mut StreamState) + Send>;

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub text: String,
    pub error: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Installing {
    /// 0 to 1, None while flatpak hasn't said.
    pub progress: Option<f32>,
    pub error: Option<String>,
}

/// errors.ts `failed`: one plain sentence, the raw error goes to the log.
pub fn failed(what: &str, raw: &str, next: &str) -> String {
    log::warn!("error: {what}: {raw}");
    if next.is_empty() {
        what.to_string()
    } else {
        format!("{what} {next}")
    }
}

pub fn rare_way(guest: bool) -> &'static str {
    if guest {
        "Hold View"
    } else {
        "Press the Rare button"
    }
}

const WAKE_EVERY: f64 = 3.0;
const WAKE_TRIES: u32 = 20;
const PAIR_EVERY: f64 = 1.0;
const LAUNCH_HOLD: f64 = 1.5;
const JOB_EVERY: f64 = 0.1;
const INSTALL_EVERY: f64 = 0.25;

struct WakeJob {
    uuid: String,
    tries: u32,
    next: f64,
    /// Play after the PC is up: (app id, replace).
    then: Option<(u32, bool)>,
}

pub struct StreamState {
    pub api: Box<dyn StreamApi>,
    pub loaded: bool,
    pub moonlight: Option<String>,
    pub offer: Option<Offer>,
    pub decode: Option<Decode>,
    pub settings: StreamSettings,
    pub hosts: Vec<Host>,
    pub apps: Vec<(String, Vec<StreamApp>)>,
    /// Looking for PCs on the network.
    pub scanning: bool,
    /// Listing a PC's games.
    pub listing: Option<String>,
    /// Waking a PC (uuid).
    pub waking: Option<String>,
    pub pairing: Option<PairView>,
    pub installing: Option<Installing>,
    /// A PC picked while Moonlight was still installing: pairing starts once it is in.
    pub pending_pair: Option<String>,
    pub running: Vec<String>,
    pub launching: Option<String>,
    pub guest: bool,
    pub toasts: Vec<Toast>,
    wake_job: Option<WakeJob>,
    pair_next: f64,
    launch_until: Option<f64>,
    /// A PC that was just paired: the screen shows its games (state.ts `paired`).
    pub just_paired: Option<String>,
    /// What a background call is doing now ("Quitting", "Starting"); the sheet shows it busy.
    pub busy: Option<String>,
    jobs_tx: std::sync::mpsc::Sender<Job>,
    jobs_rx: std::sync::mpsc::Receiver<Job>,
    last_now: f64,
    install_next: f64,
}

impl StreamState {
    pub fn new(api: Box<dyn StreamApi>) -> StreamState {
        let (jobs_tx, jobs_rx) = std::sync::mpsc::channel();
        StreamState {
            busy: None,
            jobs_tx,
            jobs_rx,
            last_now: 0.0,
            install_next: 0.0,
            api,
            loaded: false,
            moonlight: None,
            offer: None,
            decode: None,
            settings: default_settings(),
            hosts: vec![],
            apps: vec![],
            scanning: false,
            listing: None,
            waking: None,
            pairing: None,
            installing: None,
            pending_pair: None,
            running: vec![],
            launching: None,
            guest: false,
            toasts: vec![],
            wake_job: None,
            pair_next: 0.0,
            launch_until: None,
            just_paired: None,
        }
    }

    /// Runs a slow call off the UI thread; its result lands through `tick`. Without a second
    /// handle (the fixtures) it runs in place.
    fn background(&mut self, label: &str, work: impl FnOnce(&mut dyn StreamApi) -> Job + Send + 'static) {
        match self.api.fork() {
            Some(mut api) => {
                self.busy = Some(label.to_string());
                let tx = self.jobs_tx.clone();
                std::thread::spawn(move || {
                    let apply = work(&mut *api);
                    let _ = tx.send(apply);
                });
            }
            None => {
                let apply = work(&mut *self.api);
                apply(self);
            }
        }
    }

    fn toast(&mut self, t: impl Into<String>, error: bool) {
        self.toasts.push(Toast { text: t.into(), error });
    }

    pub fn take_toasts(&mut self) -> Vec<Toast> {
        std::mem::take(&mut self.toasts)
    }

    pub fn apps_of(&self, uuid: &str) -> &[StreamApp] {
        self.apps.iter().find(|(u, _)| u == uuid).map(|(_, l)| l.as_slice()).unwrap_or(&[])
    }

    fn set_apps(&mut self, uuid: &str, list: Vec<StreamApp>) {
        match self.apps.iter_mut().find(|(u, _)| u == uuid) {
            Some(e) => e.1 = list,
            None => self.apps.push((uuid.to_string(), list)),
        }
    }

    pub fn host(&self, uuid: &str) -> Option<&Host> {
        self.hosts.iter().find(|h| h.uuid == uuid)
    }

    pub fn load(&mut self) {
        match self.api.status() {
            Ok(s) => {
                self.loaded = true;
                self.moonlight = s.moonlight;
                self.offer = s.offer;
                self.decode = s.decode;
                self.settings = s.settings;
                self.hosts = s.hosts;
                self.apps = s.apps;
            }
            Err(_) => self.loaded = true,
        }
        self.running = self.api.running();
        self.refresh();
    }

    pub fn refresh(&mut self) {
        let Ok(hosts) = self.api.refresh() else { return };
        self.hosts = hosts.clone();
        // A paired PC whose games Rare hasn't listed yet.
        for h in hosts {
            if h.paired && h.online && self.apps_of(&h.uuid).is_empty() && self.moonlight.is_some() {
                self.list_apps(&h.uuid);
            }
        }
    }

    pub fn scan(&mut self) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        match self.api.discover() {
            Ok(h) => self.hosts = h,
            Err(e) => {
                let t = failed("Couldn't look for PCs.", &e, "Try again.");
                self.toast(t, true);
            }
        }
        self.scanning = false;
    }

    pub fn add(&mut self, address: &str) -> Option<Host> {
        match self.api.add(address) {
            Ok(h) => {
                self.hosts.retain(|x| x.uuid != h.uuid);
                self.hosts.push(h.clone());
                Some(h)
            }
            Err(e) => {
                let t = failed("Couldn't add that PC. Check the address and try again.", &e, "");
                self.toast(t, true);
                None
            }
        }
    }

    pub fn pair(&mut self, host: &Host, now: f64) {
        let Some(addr) = host.address.clone() else {
            return self.toast(format!("Rare doesn't know where {} is. Add it by address.", host.name), true);
        };
        match self.api.pair_start(&addr) {
            Ok(p) => {
                let done = p.phase == PairPhase::Paired;
                self.pairing = Some(p.clone());
                if done {
                    self.on_paired(&p);
                } else {
                    self.pair_next = now + PAIR_EVERY;
                }
            }
            Err(e) => {
                let t = failed("Couldn't pair with that PC.", &e, "Try again.");
                self.toast(t, true);
            }
        }
    }

    pub fn cancel_pair(&mut self) {
        self.pairing = None;
        self.api.pair_cancel();
    }

    fn on_paired(&mut self, p: &PairView) {
        self.toast(format!("{} is paired", p.name), false);
        self.just_paired = Some(p.uuid.clone());
        self.load();
        self.list_apps(&p.uuid);
    }

    pub fn list_apps(&mut self, uuid: &str) {
        if self.listing.as_deref() == Some(uuid) {
            return;
        }
        self.listing = Some(uuid.to_string());
        match self.api.apps(uuid) {
            Ok(l) => self.set_apps(uuid, l),
            Err(e) => {
                let t = failed("Couldn't list the games on that PC.", &e, "Try again.");
                self.toast(t, true);
            }
        }
        self.listing = None;
    }

    /// Play: wakes the PC if needed; `replace` quits what the PC runs first.
    pub fn play(&mut self, host: &Host, app: &StreamApp, replace: bool, now: f64) {
        let rid = run_id(&host.uuid, app.id);
        if self.running.contains(&rid) {
            return self.resume(host, app);
        }
        if !host.online {
            if !host.can_wake {
                return self.toast(format!("{} isn't answering. Turn it on, then try again.", host.name), true);
            }
            // The launch follows when the PC answers (`tick`).
            return self.wake_start(host, Some((app.id, replace)), now);
        }
        self.launch(host, app, replace, now);
    }

    fn launch(&mut self, h: &Host, app: &StreamApp, replace: bool, now: f64) {
        let rid = run_id(&h.uuid, app.id);
        self.launching = Some(rid.clone());
        let replace = replace || h.current_game.is_some_and(|g| g != app.id);
        let (uuid, id) = (h.uuid.clone(), app.id);
        let text = format!("Streaming {} from {}. {} to come back to Rare.", app.name, h.name, rare_way(self.guest));
        self.launch_until = Some(now + LAUNCH_HOLD);
        self.last_now = now;
        self.background("Starting", move |api| {
            let r = api.launch(&uuid, id, replace);
            Box::new(move |st: &mut StreamState| {
                st.busy = None;
                match r {
                    Ok(()) => {
                        st.running = st.api.running();
                        st.toast(text, false);
                    }
                    Err(e) => {
                        let t = failed("Couldn't start the game.", &e, "Try again.");
                        st.toast(t, true);
                    }
                }
            })
        });
    }

    pub fn resume(&mut self, host: &Host, app: &StreamApp) {
        let id = run_id(&host.uuid, app.id);
        self.background("Opening", move |api| {
            let r = api.resume(&id);
            Box::new(move |st: &mut StreamState| {
                st.busy = None;
                if let Err(e) = r {
                    let t = failed("Couldn't resume the game.", &e, "Try again.");
                    st.toast(t, true);
                }
            })
        });
    }

    pub fn quit(&mut self, host: &Host) {
        self.toast(format!("Quitting the game on {}", host.name), false);
        let uuid = host.uuid.clone();
        self.background("Quitting", move |api| {
            let r = api.quit(&uuid);
            Box::new(move |st: &mut StreamState| {
                st.busy = None;
                match r {
                    Ok(()) => {
                        for h in st.hosts.iter_mut().filter(|h| h.uuid == uuid) {
                            h.current_game = None;
                        }
                        st.running = st.api.running();
                    }
                    Err(e) => {
                        let t = failed("Couldn't close the game.", &e, "Try again.");
                        st.toast(t, true);
                    }
                }
            })
        });
    }

    /// Wake a PC, then ask every 3 s for a minute (`tick`).
    pub fn wake_start(&mut self, host: &Host, then: Option<(u32, bool)>, now: f64) {
        self.waking = Some(host.uuid.clone());
        match self.api.wake(&host.uuid) {
            Ok(()) => self.wake_job = Some(WakeJob { uuid: host.uuid.clone(), tries: 0, next: now + WAKE_EVERY, then }),
            Err(e) => {
                self.waking = None;
                let t = failed("Couldn't wake that PC.", &e, "Try again.");
                self.toast(t, true);
            }
        }
    }

    pub fn set_settings(&mut self, s: StreamSettings) {
        self.settings = s.clone();
        if let Err(e) = self.api.set_settings(&s) {
            let t = failed("Couldn't save the streaming settings.", &e, "Try again.");
            self.toast(t, true);
        }
    }

    pub fn install(&mut self) {
        self.installing = Some(Installing { progress: None, error: None });
        if let Err(e) = self.api.install() {
            self.install_done(Some(failed("Couldn't set up streaming.", &e, "Check your Wi-Fi and try again.")));
        }
    }

    /// The Apps installer's "app-install" event for Moonlight.
    pub fn install_progress(&mut self, progress: Option<f32>) {
        self.installing = Some(Installing { progress, error: None });
    }

    pub fn install_done(&mut self, error: Option<String>) {
        match error {
            Some(e) => {
                self.installing = Some(Installing { progress: None, error: Some(e.clone()) });
                self.toast(e, true);
            }
            None => {
                self.installing = None;
                self.toast("Moonlight is installed", false);
            }
        }
        self.load();
    }

    /// Leaving the setup sheet stops a pairing that hasn't finished.
    pub fn leave_setup(&mut self) {
        if self.pairing.as_ref().is_some_and(|p| p.phase == PairPhase::Waiting) {
            self.cancel_pair();
        }
    }

    /// When `tick` next has something to do.
    pub fn next_deadline(&self) -> Option<f64> {
        let mut d: Vec<f64> = vec![];
        if self.pairing.as_ref().is_some_and(|p| p.phase == PairPhase::Waiting) {
            d.push(self.pair_next);
        }
        if let Some(j) = &self.wake_job {
            d.push(j.next);
        }
        if let Some(t) = self.launch_until {
            d.push(t);
        }
        if self.busy.is_some() {
            d.push(self.last_now + JOB_EVERY);
        }
        if self.installing.as_ref().is_some_and(|i| i.error.is_none()) {
            d.push(self.install_next);
        }
        d.into_iter().reduce(f64::min)
    }

    /// Returns true when something the screen shows changed.
    pub fn tick(&mut self, now: f64) -> bool {
        let mut changed = false;
        self.last_now = now;
        while let Ok(job) = self.jobs_rx.try_recv() {
            job(self);
            changed = true;
        }
        if self.installing.as_ref().is_some_and(|i| i.error.is_none()) && now >= self.install_next {
            self.install_next = now + INSTALL_EVERY;
            if let Some((p, done, err)) = self.api.install_poll() {
                changed = true;
                if done {
                    self.install_done(err);
                } else {
                    self.install_progress(p);
                }
            }
        }
        if self.launch_until.is_some_and(|t| now >= t) && self.busy.is_none() {
            self.launch_until = None;
            self.launching = None;
            changed = true;
        }
        if let Some(p) = self.pairing.clone().filter(|p| p.phase == PairPhase::Waiting) {
            if now >= self.pair_next {
                self.pair_next = now + PAIR_EVERY;
                if let Ok(Some(v)) = self.api.pair_poll() {
                    if v.pin == p.pin {
                        self.pairing = Some(v.clone());
                        changed = true;
                        if v.phase == PairPhase::Paired {
                            self.on_paired(&v);
                        }
                    }
                }
            }
        }
        if let Some(mut j) = self.wake_job.take() {
            if now >= j.next {
                j.tries += 1;
                j.next = now + WAKE_EVERY;
                self.refresh();
                changed = true;
                let up = self.host(&j.uuid).cloned().filter(|h| h.online);
                if let Some(h) = up {
                    self.waking = None;
                    if let Some((app, replace)) = j.then {
                        if let Some(a) = self.apps_of(&h.uuid).iter().find(|a| a.id == app).cloned() {
                            self.launch(&h, &a, replace, now);
                        }
                    }
                } else if j.tries >= WAKE_TRIES {
                    self.waking = None;
                    let name = self.host(&j.uuid).map(|h| h.name.clone()).unwrap_or_default();
                    self.toast(format!("{name} didn't wake up. Check Wake-on-LAN is on in its BIOS and network settings."), true);
                } else {
                    self.wake_job = Some(j);
                }
            } else {
                self.wake_job = Some(j);
            }
        }
        changed
    }
}

/// What a running id is, for the status bar and the running list.
pub fn stream_title(st: &StreamState, rid: &str) -> Option<String> {
    let (uuid, app) = parse_run_id(rid)?;
    let host = st.host(&uuid);
    let a = st.apps_of(&uuid).iter().find(|a| a.id == app);
    Some(match (a, host) {
        (Some(a), _) => a.name.clone(),
        (None, Some(h)) => format!("A game on {}", h.name),
        _ => "PC game".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{Mode, MockApi};
    use super::*;

    fn state(mode: Mode) -> StreamState {
        let mut s = StreamState::new(Box::new(MockApi::new(mode)));
        s.load();
        s
    }

    #[test]
    fn load_lists_games_for_a_paired_pc() {
        let s = state(Mode::Normal);
        assert!(s.loaded && s.moonlight.is_some());
        assert_eq!(s.apps_of(super::super::fixtures::PC).len(), 8);
        assert_eq!(stream_title(&s, &run_id(super::super::fixtures::PC, 1245620)).as_deref(), Some("ELDEN RING"));
        assert_eq!(stream_title(&s, "stream:zzz:1").as_deref(), Some("PC game"));
    }

    #[test]
    fn no_moonlight_offers_the_install() {
        let mut s = state(Mode::NoMoonlight);
        assert!(s.moonlight.is_none() && s.offer.is_some());
        s.install();
        assert_eq!(s.installing, Some(Installing { progress: None, error: None }));
        s.install_progress(Some(0.5));
        s.install_done(None);
        assert!(s.installing.is_none() && s.moonlight.is_some());
        assert_eq!(s.take_toasts()[0].text, "Moonlight is installed");
    }

    #[test]
    fn pairs_through_the_poll() {
        let mut s = state(Mode::Normal);
        s.scan();
        let den = s.hosts.iter().find(|h| !h.paired).cloned().unwrap();
        s.pair(&den, 0.0);
        assert_eq!(s.pairing.as_ref().unwrap().pin, "4821");
        assert_eq!(s.next_deadline(), Some(1.0));
        for i in 1..=20 {
            s.tick(i as f64);
        }
        assert_eq!(s.pairing.as_ref().unwrap().phase, PairPhase::Paired);
        assert!(s.host(&den.uuid).unwrap().paired);
        assert!(s.toasts.iter().any(|t| t.text == "Living-room is paired"));
        assert_eq!(s.just_paired.as_deref(), Some(den.uuid.as_str()));
    }

    #[test]
    fn leaving_setup_cancels_a_waiting_pairing() {
        let mut s = state(Mode::Pairing);
        s.scan();
        let den = s.hosts.iter().find(|h| !h.paired).cloned().unwrap();
        s.pair(&den, 0.0);
        s.leave_setup();
        assert!(s.pairing.is_none());
    }

    #[test]
    fn pair_without_an_address_says_so() {
        let mut s = state(Mode::Normal);
        let mut h = s.hosts[0].clone();
        h.address = None;
        s.pair(&h, 0.0);
        assert_eq!(s.take_toasts()[0], Toast { text: "Rare doesn't know where Gaming-PC is. Add it by address.".into(), error: true });
    }

    #[test]
    fn add_checks_the_answer() {
        let mut s = state(Mode::Normal);
        assert!(s.add("192.168.0.31").is_some());
        assert!(s.add("10.0.0.9").is_none());
        assert_eq!(s.take_toasts()[0].text, "Couldn't add that PC. Check the address and try again.");
    }

    #[test]
    fn play_launches_and_replaces_a_busy_pc() {
        let mut s = state(Mode::Busy);
        let h = s.hosts[0].clone();
        let a = s.apps_of(&h.uuid)[0].clone();
        s.play(&h, &a, false, 0.0);
        assert_eq!(s.running, vec![run_id(&h.uuid, a.id)]);
        assert_eq!(s.launching, Some(run_id(&h.uuid, a.id)));
        assert!(s.toasts[0].text.starts_with("Streaming Cyberpunk 2077 from Gaming-PC. Press the Rare button"));
        s.tick(2.0);
        assert!(s.launching.is_none());
        // Playing again resumes.
        s.play(&h, &a, false, 3.0);
        assert_eq!(s.running.len(), 1);
        s.quit(&h);
        assert!(s.running.is_empty());
        assert_eq!(s.host(&h.uuid).unwrap().current_game, None);
    }

    #[test]
    fn play_on_an_asleep_pc_wakes_it_then_launches() {
        let mut s = state(Mode::Asleep);
        let h = s.hosts[0].clone();
        assert!(!h.online);
        let a = s.apps_of(&h.uuid)[1].clone();
        s.play(&h, &a, false, 0.0);
        assert_eq!(s.waking.as_deref(), Some(h.uuid.as_str()));
        assert!(s.running.is_empty());
        s.tick(3.0);
        assert!(s.waking.is_none());
        assert_eq!(s.running.len(), 1);
    }

    #[test]
    fn a_pc_that_never_wakes_gives_up_after_a_minute() {
        let mut s = state(Mode::NeverWakes);
        let h = s.hosts[0].clone();
        s.wake_start(&h, None, 0.0);
        for i in 1..=20 {
            s.tick(i as f64 * 3.0);
        }
        assert!(s.waking.is_none());
        assert_eq!(s.toasts.last().unwrap().text, "Gaming-PC didn't wake up. Check Wake-on-LAN is on in its BIOS and network settings.");
    }

    #[test]
    fn an_offline_pc_without_wol_says_to_turn_it_on() {
        let mut s = state(Mode::Offline);
        let h = s.hosts[0].clone();
        let a = StreamApp { id: 1, name: "x".into(), art: None, hdr: false };
        s.play(&h, &a, false, 0.0);
        assert_eq!(s.toasts[0].text, "Gaming-PC isn't answering. Turn it on, then try again.");
    }

    #[test]
    fn settings_save_and_errors_are_one_sentence() {
        let mut s = state(Mode::Normal);
        let mut n = s.settings.clone();
        n.bitrate_mbps = 30;
        s.set_settings(n);
        assert_eq!(s.settings.bitrate_mbps, 30);
        assert_eq!(failed("Couldn't x.", "raw", ""), "Couldn't x.");
        assert_eq!(rare_way(true), "Hold View");
    }

    /// A backend whose Quit takes a while: the screen must not wait for it.
    struct Slow;
    impl StreamApi for Slow {
        fn status(&mut self) -> Result<Status, String> { Err("x".into()) }
        fn refresh(&mut self) -> Result<Vec<Host>, String> { Ok(vec![]) }
        fn discover(&mut self) -> Result<Vec<Host>, String> { Ok(vec![]) }
        fn add(&mut self, _: &str) -> Result<Host, String> { Err("x".into()) }
        fn pair_start(&mut self, _: &str) -> Result<PairView, String> { Err("x".into()) }
        fn pair_poll(&mut self) -> Result<Option<PairView>, String> { Ok(None) }
        fn pair_cancel(&mut self) {}
        fn apps(&mut self, _: &str) -> Result<Vec<StreamApp>, String> { Ok(vec![]) }
        fn launch(&mut self, _: &str, _: u32, _: bool) -> Result<(), String> { Ok(()) }
        fn resume(&mut self, _: &str) -> Result<(), String> { Ok(()) }
        fn quit(&mut self, _: &str) -> Result<(), String> {
            std::thread::sleep(std::time::Duration::from_millis(300));
            Err("stuck".into())
        }
        fn wake(&mut self, _: &str) -> Result<(), String> { Ok(()) }
        fn set_settings(&mut self, _: &StreamSettings) -> Result<(), String> { Ok(()) }
        fn install(&mut self) -> Result<(), String> { Ok(()) }
        fn running(&mut self) -> Vec<String> { vec![] }
        fn fork(&self) -> Option<Box<dyn StreamApi + Send>> { Some(Box::new(Slow)) }
    }

    #[test]
    fn quit_runs_on_a_worker_and_lands_through_tick() {
        let mut s = StreamState::new(Box::new(Slow));
        let h = state(Mode::Busy).hosts[0].clone();
        let t = std::time::Instant::now();
        s.quit(&h);
        assert!(t.elapsed().as_millis() < 100, "quit blocked the caller");
        assert_eq!(s.busy.as_deref(), Some("Quitting"));
        assert!(s.next_deadline().is_some());
        for i in 0..100 {
            if s.tick(i as f64 * 0.1) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(s.busy.is_none());
        assert_eq!(s.toasts.last().unwrap().text, "Couldn't close the game. Try again.");
    }
}
