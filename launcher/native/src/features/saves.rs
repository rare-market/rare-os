//! Cloud saves (CloudSaves.tsx) over rare-sui's saves module: detect the save sets on this
//! device, unlock the save key with the wallet, list backups on Walrus, back up, restore. Every
//! upload and restore starts from a press; nothing runs in the background. Signing and paying go
//! through the wallet's TxSigner (its review sheet); the key comes from `deriveSecret`.

use crate::act::Act;
use crate::kit::Ui;
use crate::page::{hash, Acts, Btn, Fx, Page};
use crate::screens::sui::saves::{self as ui, Backup, Cloud, Cost, Msg, SaveSet, Step};
use crate::screens::wallet::live;
use crate::screens::wallet::render::{build, Built};
use crate::screens::wallet::service;
use crate::state::{State, Tone};
use rare_render::focus::Focusable;
use rare_render::view::El;
use rare_sui::saves::{self, crypto, local, meta};
use std::sync::mpsc::{channel, Receiver, Sender};

/// What a worker reports back.
enum Up {
    Sets(Result<Vec<local::SaveSet>, String>),
    Estimate(String, Cost),
    Key(Result<crypto::SaveKey, String>),
    Backups(Result<(Vec<meta::Backup>, Option<u64>), String>),
    Step(String, Step),
    BackedUp(Result<meta::Backup, String>, String),
    Restored(Result<String, String>, String),
}

pub struct SavesPage {
    cloud: Cloud,
    raw_sets: Vec<local::SaveSet>,
    raw: Vec<meta::Backup>,
    epoch: Option<u64>,
    address: Option<String>,
    key: Option<crypto::SaveKey>,
    confirm: Option<String>,
    built: Built<Msg>,
    tx: Sender<Up>,
    rx: Receiver<Up>,
    ver: u64,
    started: bool,
    fixture: bool,
    /// A Back up pressed before the saves were unlocked: it runs once the key is there.
    pending: Option<String>,
}

impl Default for SavesPage {
    fn default() -> Self {
        let (tx, rx) = channel();
        let fixture = std::env::var("RR_FIXTURE").is_ok();
        let mut p = SavesPage { cloud: Cloud { days: 90, ..Default::default() }, raw_sets: vec![], raw: vec![], epoch: None, address: None, key: None, confirm: None, built: Built::default(), tx, rx, ver: 0, started: false, fixture, pending: None };
        if fixture {
            p.address = Some(crate::screens::wallet::home::fixtures::ME.to_string());
            p.cloud = fixture_cloud();
        } else {
            p.address = service::wallet().and_then(|w| w.keystore.current_signing_account().ok().flatten()).map(|a| a.address);
            p.cloud.testnet = live::sui().map(|s| s.network() == rare_sui::Network::Testnet).unwrap_or(false);
        }
        p
    }
}

fn fixture_cloud() -> Cloud {
    let now = now_ms();
    Cloud {
        sets: Some(vec![
            SaveSet { id: "rpcs3".into(), emulator: "RPCS3".into(), label: "Saves".into(), files: 1204, bytes: 52_000_000, modified: Some(now - 7_200_000) },
            SaveSet { id: "retroarch".into(), emulator: "RetroArch".into(), label: "Save files".into(), files: 38, bytes: 910_000, modified: Some(now - 86_400_000) },
        ]),
        backups: Some(vec![Backup { blob_object_id: "0xb1".into(), set_id: "dolphin".into(), emulator: "Dolphin".into(), label: "Memory cards".into(), bytes: 8_000_000, created: now - 3 * 86_400_000, days_left: Some(27) }]),
        key: true,
        days: 90,
        estimates: vec![("rpcs3".into(), Cost { wal_frost: 120_000_000, tip_mist: 2_000_000, gas_mist: 4_000_000 }), ("retroarch".into(), Cost { wal_frost: 8_000_000, tip_mist: 1_000_000, gas_mist: 4_000_000 })],
        ..Default::default()
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn view_set(s: &local::SaveSet) -> SaveSet {
    SaveSet { id: s.id.clone(), emulator: s.emulator.clone(), label: s.label.clone(), files: s.files, bytes: s.bytes, modified: s.modified.map(|m| m as i64) }
}

fn view_backup(b: &meta::Backup, epoch: Option<u64>, testnet: bool) -> Backup {
    Backup { blob_object_id: b.blob_object_id.clone(), set_id: b.meta.id.clone(), emulator: b.meta.emulator.clone(), label: b.meta.label.clone(), bytes: b.meta.bytes, created: b.meta.created, days_left: meta::days_left(b, epoch, testnet) }
}

fn step_of(s: saves::store::Step) -> Step {
    match s {
        saves::store::Step::Encode => Step::Encode,
        saves::store::Step::Register => Step::Register,
        saves::store::Step::Upload => Step::Upload,
        saves::store::Step::Certify => Step::Certify,
        saves::store::Step::Done => Step::Done,
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().timeout(std::time::Duration::from_secs(120)).build().unwrap_or_default()
}

fn spawn(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().name("rare-saves".into()).spawn(f).ok();
}

impl SavesPage {
    fn sync(&mut self) {
        let t = self.cloud.testnet;
        self.cloud.backups = if self.key.is_some() { Some(self.raw.iter().map(|b| view_backup(b, self.epoch, t)).collect()) } else { None };
        self.cloud.key = self.key.is_some();
        self.ver += 1;
    }

    fn detect(&mut self) {
        let tx = self.tx.clone();
        spawn(move || {
            let _ = tx.send(Up::Sets(local::detect()));
        });
    }

    fn estimate_all(&mut self) {
        let Some(sui) = live::sui().cloned() else { return };
        let days = self.cloud.days;
        self.cloud.estimates.clear();
        for s in &self.raw_sets {
            let (tx, id, size, sui) = (self.tx.clone(), s.id.clone(), s.bytes + crypto::SEAL_OVERHEAD as u64, sui.clone());
            spawn(move || {
                // gzip shrinks saves; the estimate uses the raw size so the number is never low.
                let r = live::rt().block_on(saves::store::estimate(&sui, &http(), sui.network(), size, days));
                if let Ok(e) = r {
                    let _ = tx.send(Up::Estimate(id, Cost { wal_frost: e.wal_frost as u128, tip_mist: e.tip_mist as u128, gas_mist: e.gas_mist as u128 }));
                }
            });
        }
    }

    fn unlock(&mut self) {
        let Some(addr) = self.address.clone() else {
            self.cloud.error = Some("Add a Rare Wallet account first, in Wallet.".into());
            self.ver += 1;
            return;
        };
        self.cloud.error = None;
        let tx = self.tx.clone();
        let rx = service::work(move |w| {
            let mut auth = rare_wallet::passkey_account::Ctap { state: &w.passkey, sink: w.sink(), rt: w.rt.clone() };
            let via = *w.via.lock().unwrap();
            let secret = rare_wallet::secret::derive::derive_secret(&w.signing, &mut auth, via, crypto::SAVES_CONTEXT, Some(&addr), Some("Unlock cloud saves")).map_err(|e| e.to_string())?;
            crypto::derive_save_key(&secret[..], &addr).map_err(|e| e.to_string())
        });
        spawn(move || {
            let r = rx.recv().unwrap_or_else(|_| Err("The wallet stopped. Try again.".into())).and_then(|r| r);
            let _ = tx.send(Up::Key(r));
        });
        self.cloud.busy = Some((String::new(), Step::Encode));
        self.ver += 1;
    }

    fn refresh(&mut self) {
        let (Some(addr), Some(key), Some(sui)) = (self.address.clone(), self.key.clone(), live::sui().cloned()) else { return };
        let tx = self.tx.clone();
        spawn(move || {
            let n = sui.network();
            let r = live::rt().block_on(async {
                let b = saves::walrus::list_backups(&sui, n, &addr, &key).await?;
                let e = saves::walrus::current_epoch(&sui, n).await.ok();
                Ok::<_, rare_sui::Error>((b, e))
            });
            let _ = tx.send(Up::Backups(r.map_err(|e| e.to_string())));
        });
    }

    fn back_up(&mut self, id: &str) {
        // Every way out says why; locked saves unlock first, then back up.
        let Some(key) = self.key.clone() else {
            let pending = self.address.is_some();
            self.unlock();
            if pending {
                // The row shows it working while the key is made, then the backup runs.
                self.pending = Some(id.to_string());
                self.cloud.busy = Some((id.to_string(), Step::Encode));
                self.ver += 1;
            }
            return;
        };
        let fail = |p: &mut SavesPage, e: &str| {
            p.cloud.error = Some(e.to_string());
            p.ver += 1;
        };
        let Some(set) = self.raw_sets.iter().find(|s| s.id == id).cloned() else { return fail(self, "That save isn't on this device anymore.") };
        let Some(sui) = live::sui().cloned() else { return fail(self, "Couldn't reach Sui. Check the connection and try again.") };
        let Some(signer) = crate::features::signer::current() else {
            self.cloud.error = Some("Add a Rare Wallet account you can sign with first".into());
            self.ver += 1;
            return;
        };
        let (tx, days) = (self.tx.clone(), self.cloud.days);
        self.cloud.busy = Some((id.to_string(), Step::Pack));
        self.cloud.error = None;
        self.ver += 1;
        spawn(move || {
            let tx2 = tx.clone();
            let sid = set.id.clone();
            let on_step = move |s: saves::store::Step| {
                let _ = tx2.send(Up::Step(sid.clone(), step_of(s)));
            };
            let r = live::rt().block_on(saves::backup(&sui, &http(), signer.as_ref(), &key, &set, days, Some(&on_step), local::pack, now_ms()));
            let _ = tx.send(Up::BackedUp(r.map_err(|e| e.to_string()), format!("{} {}", set.emulator, set.label.to_lowercase())));
        });
    }

    fn restore(&mut self, blob: &str) {
        let (Some(key), Some(b), Some(sui)) = (self.key.clone(), self.raw.iter().find(|b| b.blob_object_id == blob).cloned(), live::sui().cloned()) else { return };
        let tx = self.tx.clone();
        self.cloud.busy = Some((b.meta.id.clone(), Step::Download));
        self.cloud.error = None;
        self.ver += 1;
        spawn(move || {
            let r = live::rt().block_on(saves::restore(&http(), sui.network(), &key, &b));
            let _ = tx.send(Up::Restored(r.map_err(|e| e.to_string()), b.meta.emulator.clone()));
        });
    }
}

impl Page for SavesPage {
    fn stamp(&self, st: &State) -> u64 {
        hash((self.ver, st.ver.live, crate::features::cloudsync::ver()))
    }

    fn view(&mut self, ui: &mut Ui, _st: &State, acts: &mut Acts) -> El {
        use crate::features::cloudsync as cs;
        let funds = cs::funds();
        self.cloud.auto = ui::Auto { on: cs::is_on(), unlocking: cs::unlocking(), funds, uploads_left: funds.map(cs::uploads_left) };
        let mut m = ui::view(&self.cloud, self.address.as_deref(), self.confirm.as_deref(), now_ms(), 0);
        m.class = m.class.replace("fixed inset-0 z-30", "h-full w-full");
        let (el, built) = build(ui, acts, &m);
        self.built = built;
        el
    }

    fn pad(&mut self, _st: &mut State, _fx: &mut Fx, b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        if b == Btn::X {
            if let Some(Msg::Keep) = self.built.pad("x") {
                self.cloud.days = ui::keep_next(self.cloud.days);
                if !self.fixture {
                    self.estimate_all();
                }
                self.ver += 1;
                return true;
            }
        }
        false
    }

    fn act(&mut self, _st: &mut State, _fx: &mut Fx, code: u32) {
        let Some(m) = self.built.msg(code) else { return };
        match m {
            Msg::Unlock => self.unlock(),
            Msg::BackUp(id) => {
                self.confirm = None;
                self.back_up(&id);
            }
            Msg::Restore(blob) => {
                if let Some(blob) = ui::restore_press(&mut self.confirm, &blob) {
                    self.restore(&blob);
                }
                self.ver += 1;
            }
            Msg::Keep => {}
            Msg::AutoOn => crate::features::cloudsync::turn_on(),
            Msg::AutoOff => crate::features::cloudsync::turn_off(),
            Msg::AddFunds => crate::features::cloudsync::add_funds(),
        }
    }

    fn tick(&mut self, st: &mut State, _fx: &mut Fx, now: f64) -> Option<f64> {
        if !self.started && !self.fixture {
            self.started = true;
            self.detect();
        }
        let mut changed = false;
        while let Ok(u) = self.rx.try_recv() {
            changed = true;
            match u {
                Up::Sets(Ok(sets)) => {
                    self.cloud.sets = Some(sets.iter().map(view_set).collect());
                    self.raw_sets = sets;
                    self.estimate_all();
                }
                Up::Sets(Err(e)) => {
                    self.cloud.sets = Some(vec![]);
                    self.cloud.error = Some(e);
                }
                Up::Estimate(id, c) => {
                    self.cloud.estimates.retain(|(i, _)| *i != id);
                    self.cloud.estimates.push((id, c));
                }
                Up::Key(Ok(k)) => {
                    self.cloud.busy = None;
                    self.key = Some(k);
                    self.sync();
                    self.refresh();
                    if let Some(id) = self.pending.take() {
                        self.back_up(&id);
                    }
                }
                Up::Key(Err(e)) => {
                    self.pending = None;
                    self.cloud.busy = None;
                    self.cloud.error = Some(e);
                }
                Up::Backups(Ok((b, e))) => {
                    self.raw = b;
                    self.epoch = e;
                    self.sync();
                }
                Up::Backups(Err(e)) => self.cloud.error = Some(e),
                Up::Step(id, s) => self.cloud.busy = Some((id, s)),
                Up::BackedUp(r, what) => {
                    self.cloud.busy = None;
                    match r {
                        Ok(b) => {
                            self.raw.insert(0, b);
                            self.sync();
                            self.refresh();
                            st.toast(format!("{what} backed up"), Tone::Info, now);
                        }
                        Err(e) => self.cloud.error = Some(e),
                    }
                }
                Up::Restored(r, emu) => {
                    self.cloud.busy = None;
                    match r {
                        Ok(moved) if !moved.is_empty() => st.toast(format!("Restored. Your previous saves are kept in {}", moved.rsplit('/').next().unwrap_or("")), Tone::Info, now),
                        Ok(_) => st.toast(format!("{emu} saves restored"), Tone::Info, now),
                        Err(e) => self.cloud.error = Some(e),
                    }
                    self.detect();
                }
            }
        }
        if changed {
            self.ver += 1;
        }
        if self.fixture {
            None
        } else {
            Some(now + 0.25)
        }
    }
}
