//! Ledger transport: raw APDUs over USB HID (every model) or Bluetooth LE (Nano X, Stax, Flex).
//! The Sui app protocol runs on top of `LedgerState::exchange` (JS: src/wallet/native.ts, LedgerNative).
//!
//! One device is open at a time. Exchanges are serialized by one mutex and wait as long as the
//! person needs to confirm on the device; `ledger_close` (or opening another device) cancels a
//! waiting exchange within one read tick.

pub mod ble;
pub mod framing;
pub mod usb;

#[cfg(target_os = "linux")]
mod bluez_agent;

use crate::{err, Sink};
use base64::Engine as _;
use framing::{ble_frames, hid_frames, Reassembler};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

/// One read wait; also how fast a cancel takes effect.
const READ_TICK: Duration = Duration::from_millis(500);
/// Upper bound for one exchange. Long on purpose: the person may be reading a big transaction.
const EXCHANGE_LIMIT: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LedgerDevice {
    /// "usb:<hid path>" or "ble:<bluetooth id>"
    pub id: String,
    pub transport: &'static str,
    pub name: String,
    pub model: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paired: Option<bool>,
}

/// Model and display name from a USB product id: legacy ids, else the high byte
/// (@ledgerhq/devices identifyUSBProductId).
pub fn model_from_pid(pid: u16) -> (&'static str, &'static str) {
    let legacy = match pid {
        0x0001 => Some(0x10),
        0x0004 => Some(0x40),
        0x0005 => Some(0x50),
        0x0006 => Some(0x60),
        0x0007 => Some(0x70),
        0x0008 => Some(0x80),
        _ => None,
    };
    match legacy.unwrap_or(pid >> 8) {
        0x10 => ("nanoS", "Nano S"),
        0x40 => ("nanoX", "Nano X"),
        0x50 => ("nanoSP", "Nano S Plus"),
        0x60 => ("stax", "Stax"),
        0x70 => ("flex", "Flex"),
        0x80 => ("unknown", "Nano Gen5"),
        _ => ("unknown", "Ledger"),
    }
}

/// One frame in, one frame out. USB and BLE implement it; tests use a scripted fake.
pub(crate) trait FrameIo: Send {
    fn write(&mut self, frame: &[u8]) -> Result<(), String>;
    /// Waits up to `wait` for one frame; Ok(None) when nothing came.
    fn read(&mut self, wait: Duration) -> Result<Option<Vec<u8>>, String>;
    /// BLE frame size, or None for 64-byte HID framing.
    fn ble_frame(&self) -> Option<usize>;
}

/// Sends one APDU and reassembles the response. `live` turns false when the exchange was
/// cancelled (close, or another device opened).
pub(crate) fn exchange_on(io: &mut dyn FrameIo, apdu: &[u8], live: &dyn Fn() -> bool, limit: Duration) -> Result<Vec<u8>, String> {
    let ble = io.ble_frame();
    let frames = match ble {
        Some(size) => ble_frames(apdu, size),
        None => hid_frames(apdu),
    };
    for f in &frames {
        if !live() {
            return Err(err("cancelled", "Cancelled"));
        }
        io.write(f)?;
    }
    let mut r = if ble.is_some() { Reassembler::ble() } else { Reassembler::hid() };
    let start = Instant::now();
    loop {
        if !live() {
            return Err(err("cancelled", "Cancelled"));
        }
        if start.elapsed() > limit {
            return Err(err("timeout", "No answer from your Ledger"));
        }
        let Some(frame) = io.read(READ_TICK)? else { continue };
        // A late MTU answer is not part of any APDU response.
        if ble.is_some() && r.is_empty() && frame.first() == Some(&0x08) {
            continue;
        }
        match r.push(&frame) {
            Ok(Some(resp)) => return Ok(resp),
            Ok(None) => {}
            Err(e) => return Err(err("disconnected", format!("Bad answer from your Ledger ({e}), reconnect it"))),
        }
    }
}

struct Open {
    io: Box<dyn FrameIo>,
    device: LedgerDevice,
}

#[derive(Default)]
struct Inner {
    conn: Mutex<Option<Open>>,
    /// Bumped by close/open; an exchange started under an older value stops.
    gen: AtomicU64,
}

#[derive(Default)]
pub struct LedgerState {
    inner: Arc<Inner>,
}

/// "udev" error with the one-line fix for the JS side.
pub(crate) fn udev_err() -> String {
    crate::err_data(
        "udev",
        "Rare Wallet needs permission to use your hardware wallet",
        serde_json::json!({ "fix": crate::udev::fix_command() }),
    )
}

fn exchange_sync(inner: &Inner, apdu: &[u8]) -> Result<Vec<u8>, String> {
    if apdu.len() > u16::MAX as usize {
        return Err(err("error", "APDU too long"));
    }
    let gen = inner.gen.load(Ordering::SeqCst);
    let mut guard = match inner.conn.try_lock() {
        Ok(g) => g,
        Err(TryLockError::WouldBlock) => return Err(err("busy", "Your Ledger is busy with another request")),
        Err(TryLockError::Poisoned(p)) => p.into_inner(),
    };
    let Some(open) = guard.as_mut() else {
        return Err(err("disconnected", "No Ledger open"));
    };
    let live = || inner.gen.load(Ordering::SeqCst) == gen;
    let res = exchange_on(open.io.as_mut(), apdu, &live, EXCHANGE_LIMIT);
    if let Err(e) = &res {
        // A dead link stays dead: the next call reports it right away.
        if e.starts_with("disconnected|") {
            log::info!("ledger {} closed: link lost", open.device.transport);
            *guard = None;
        }
    }
    res
}

/// The open Ledger as an APDU link for the Sui app protocol (`crate::ledger_sui`).
impl crate::ledger_sui::Apdu for &LedgerState {
    fn exchange(&mut self, apdu: &[u8]) -> Result<Vec<u8>, crate::ledger_sui::LedgerError> {
        self.exchange_blocking(apdu).map_err(|e| crate::ledger_sui::LedgerError::from_native(&e))
    }
}

impl LedgerState {
    /// USB devices now; with `ble`, also scans Bluetooth for `scan_ms` (Nano X, Stax, Flex).
    pub async fn list(ble: bool, scan_ms: u64) -> Result<Vec<LedgerDevice>, String> {
        let mut out = tokio::task::spawn_blocking(usb::list).await.map_err(|_| err("error", "USB scan failed"))??;
        if ble {
            // Bluetooth problems should not hide plugged-in devices.
            match ble::list(scan_ms.clamp(500, 30_000)).await {
                Ok(found) => out.extend(found),
                Err(e) if out.is_empty() => return Err(e),
                Err(e) => log::warn!("ledger ble scan: {}", e.split('|').next().unwrap_or("")),
            }
        }
        Ok(out)
    }

    /// Opens `id` ("usb:<path>" or "ble:<id>"), closing (and cancelling) any open device first.
    /// BLE pairing on Linux sends `WalletEvent::LedgerPairing` through `sink`.
    pub async fn open(&self, sink: &Sink, id: &str) -> Result<LedgerDevice, String> {
        let inner = self.inner.clone();
        // Cancel a waiting exchange and drop the old device before opening the new one.
        inner.gen.fetch_add(1, Ordering::SeqCst);
        let old = inner.clone();
        tokio::task::spawn_blocking(move || old.conn.lock().map(|mut c| c.take()).ok().flatten()).await.ok();

        let (io, device): (Box<dyn FrameIo>, LedgerDevice) = if let Some(path) = id.strip_prefix("usb:") {
            let path = path.to_string();
            let (io, dev) = tokio::task::spawn_blocking(move || usb::open(&path)).await.map_err(|_| err("error", "USB open failed"))??;
            (Box::new(io), dev)
        } else if id.starts_with("ble:") {
            let (io, dev) = ble::open(sink, id).await?;
            (Box::new(io), dev)
        } else {
            return Err(err("notfound", "Connect your Ledger and unlock it"));
        };
        let ret = device.clone();
        tokio::task::spawn_blocking(move || {
            if let Ok(mut c) = inner.conn.lock() {
                *c = Some(Open { io, device });
            }
        })
        .await
        .ok();
        Ok(ret)
    }

    /// One APDU in, the response (data + 2-byte status word) out. Waits as long as the person
    /// needs on the device; `close` or another `open` cancels.
    pub async fn exchange(&self, apdu: Vec<u8>) -> Result<Vec<u8>, String> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || exchange_sync(&inner, &apdu)).await.map_err(|_| err("error", "Ledger task failed"))?
    }

    /// As `exchange`, on the calling thread (the native UI's wallet worker).
    pub fn exchange_blocking(&self, apdu: &[u8]) -> Result<Vec<u8>, String> {
        exchange_sync(&self.inner, apdu)
    }

    /// As `exchange`, base64 in and out (the JS bridge's shape).
    pub async fn exchange_b64(&self, apdu: &str) -> Result<String, String> {
        let b64 = base64::engine::general_purpose::STANDARD;
        let apdu = b64.decode(apdu.trim()).map_err(|_| err("error", "Bad APDU"))?;
        Ok(b64.encode(self.exchange(apdu).await?))
    }

    pub async fn close(&self) {
        let inner = self.inner.clone();
        inner.gen.fetch_add(1, Ordering::SeqCst);
        tokio::task::spawn_blocking(move || {
            let taken = inner.conn.lock().map(|mut c| c.take()).ok().flatten();
            drop(taken);
        })
        .await
        .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicBool;

    /// Scripted device: records written frames; once a whole APDU arrived, queues the reply
    /// frames, optionally after some empty read ticks (the person confirming on the device).
    struct FakeLedger {
        ble: Option<usize>,
        written: Vec<Vec<u8>>,
        rx: Reassembler,
        replies: VecDeque<Vec<u8>>,
        outbox: VecDeque<Option<Vec<u8>>>,
        idle_ticks: usize,
        apdus: Vec<Vec<u8>>,
        fail_write: bool,
    }

    impl FakeLedger {
        fn new(ble: Option<usize>, replies: Vec<Vec<u8>>, idle_ticks: usize) -> Self {
            let rx = if ble.is_some() { Reassembler::ble() } else { Reassembler::hid() };
            Self { ble, written: vec![], rx, replies: replies.into(), outbox: VecDeque::new(), idle_ticks, apdus: vec![], fail_write: false }
        }
    }

    impl FrameIo for FakeLedger {
        fn write(&mut self, frame: &[u8]) -> Result<(), String> {
            if self.fail_write {
                return Err(err("disconnected", "gone"));
            }
            self.written.push(frame.to_vec());
            if let Some(apdu) = self.rx.push(frame).unwrap() {
                self.rx = if self.ble.is_some() { Reassembler::ble() } else { Reassembler::hid() };
                self.apdus.push(apdu);
                let reply = self.replies.pop_front().unwrap();
                for _ in 0..self.idle_ticks {
                    self.outbox.push_back(None);
                }
                let frames = match self.ble {
                    Some(n) => ble_frames(&reply, n),
                    None => hid_frames(&reply),
                };
                self.outbox.extend(frames.into_iter().map(Some));
            }
            Ok(())
        }
        fn read(&mut self, _wait: Duration) -> Result<Option<Vec<u8>>, String> {
            match self.outbox.pop_front() {
                Some(f) => Ok(f),
                None => Err(err("disconnected", "nothing scripted")),
            }
        }
        fn ble_frame(&self) -> Option<usize> {
            self.ble
        }
    }

    fn sign_apdu(n: usize) -> Vec<u8> {
        let mut a = vec![0x00, 0x03, 0x00, 0x00, n.min(255) as u8];
        a.extend((0..n).map(|i| i as u8));
        a
    }

    #[test]
    fn exchange_hid_multi_frame_with_wait() {
        let resp: Vec<u8> = (0..200u32).map(|i| (i * 3) as u8).chain([0x90, 0x00]).collect();
        let mut dev = FakeLedger::new(None, vec![resp.clone()], 5);
        let apdu = sign_apdu(250);
        let got = exchange_on(&mut dev, &apdu, &|| true, EXCHANGE_LIMIT).unwrap();
        assert_eq!(got, resp);
        assert_eq!(dev.apdus, vec![apdu]);
        assert_eq!(dev.written.len(), 5);
        assert!(dev.written.iter().all(|f| f.len() == 64));
    }

    #[test]
    fn exchange_ble_skips_late_mtu_answer() {
        let resp = vec![0x01, 0x02, 0x90, 0x00];
        let mut dev = FakeLedger::new(Some(23), vec![resp.clone()], 0);
        dev.outbox.push_back(Some(vec![0x08, 0, 0, 0, 1, 153]));
        let apdu = sign_apdu(100);
        assert_eq!(exchange_on(&mut dev, &apdu, &|| true, EXCHANGE_LIMIT).unwrap(), resp);
        assert!(dev.written.iter().all(|f| f.len() <= 23));
        assert_eq!(dev.apdus, vec![apdu]);
    }

    #[test]
    fn exchange_status_word_only() {
        let mut dev = FakeLedger::new(None, vec![vec![0x6e, 0x00]], 0);
        assert_eq!(exchange_on(&mut dev, &[0xe0, 1, 0, 0, 0], &|| true, EXCHANGE_LIMIT).unwrap(), vec![0x6e, 0x00]);
    }

    #[test]
    fn exchange_cancel_stops_waiting() {
        let mut dev = FakeLedger::new(None, vec![vec![0x90, 0x00]], 1000);
        let flag = AtomicBool::new(true);
        let reads = std::cell::Cell::new(0);
        let live = || {
            reads.set(reads.get() + 1);
            if reads.get() > 10 {
                flag.store(false, Ordering::SeqCst);
            }
            flag.load(Ordering::SeqCst)
        };
        let e = exchange_on(&mut dev, &[0xe0, 1, 0, 0, 0], &live, EXCHANGE_LIMIT).unwrap_err();
        assert!(e.starts_with("cancelled|"), "{e}");
    }

    #[test]
    fn exchange_limit_times_out() {
        let mut dev = FakeLedger::new(None, vec![vec![0x90, 0x00]], 1000);
        let e = exchange_on(&mut dev, &[0xe0, 1, 0, 0, 0], &|| true, Duration::ZERO).unwrap_err();
        assert!(e.starts_with("timeout|"), "{e}");
    }

    #[test]
    fn exchange_write_failure_is_disconnected() {
        let mut dev = FakeLedger::new(None, vec![], 0);
        dev.fail_write = true;
        let e = exchange_on(&mut dev, &[0xe0, 1, 0, 0, 0], &|| true, EXCHANGE_LIMIT).unwrap_err();
        assert!(e.starts_with("disconnected|"), "{e}");
    }

    #[test]
    fn exchange_bad_frame_is_disconnected() {
        struct Garbage;
        impl FrameIo for Garbage {
            fn write(&mut self, _: &[u8]) -> Result<(), String> {
                Ok(())
            }
            fn read(&mut self, _: Duration) -> Result<Option<Vec<u8>>, String> {
                Ok(Some(vec![0x02, 0x02, 0x05, 0, 0, 0, 2, 0x90, 0]))
            }
            fn ble_frame(&self) -> Option<usize> {
                None
            }
        }
        let e = exchange_on(&mut Garbage, &[0xe0, 1, 0, 0, 0], &|| true, EXCHANGE_LIMIT).unwrap_err();
        assert!(e.starts_with("disconnected|"), "{e}");
    }

    #[test]
    fn models_from_product_ids() {
        assert_eq!(model_from_pid(0x1011).0, "nanoS");
        assert_eq!(model_from_pid(0x0001).0, "nanoS");
        assert_eq!(model_from_pid(0x4011).0, "nanoX");
        assert_eq!(model_from_pid(0x0004).0, "nanoX");
        assert_eq!(model_from_pid(0x5011), ("nanoSP", "Nano S Plus"));
        assert_eq!(model_from_pid(0x0005).0, "nanoSP");
        assert_eq!(model_from_pid(0x6011).0, "stax");
        assert_eq!(model_from_pid(0x6015).0, "stax");
        assert_eq!(model_from_pid(0x7011).0, "flex");
        assert_eq!(model_from_pid(0x0007).0, "flex");
        assert_eq!(model_from_pid(0x8011).0, "unknown");
        assert_eq!(model_from_pid(0x2011).0, "unknown");
    }

    #[test]
    fn device_json_shape() {
        let d = LedgerDevice { id: "usb:/dev/hidraw3".into(), transport: "usb", name: "Nano S Plus".into(), model: "nanoSP", paired: None };
        assert_eq!(serde_json::to_value(&d).unwrap(), serde_json::json!({"id":"usb:/dev/hidraw3","transport":"usb","name":"Nano S Plus","model":"nanoSP"}));
        let d = LedgerDevice { paired: Some(true), transport: "ble", ..d };
        assert_eq!(serde_json::to_value(&d).unwrap()["paired"], true);
    }
}
