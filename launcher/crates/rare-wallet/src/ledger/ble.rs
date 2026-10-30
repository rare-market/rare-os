//! Ledger over Bluetooth LE (btleplug: BlueZ on Linux, CoreBluetooth on macOS).
//!
//! GATT layout from @ledgerhq/devices `bluetoothSpec`: one service per model with a notify
//! characteristic (device to host), a write characteristic and a write-without-response one.
//! After subscribing, the host asks for the frame size (MTU query) and then frames APDUs with
//! framing::ble_frames.

use super::framing::{parse_mtu_reply, BLE_DEFAULT_FRAME, MTU_REQUEST};
use super::{FrameIo, LedgerDevice};
use crate::{err, Sink};
use btleplug::api::{Central, CentralState, Characteristic, Manager as _, Peripheral as _, PeripheralProperties, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub struct Spec {
    pub model: &'static str,
    pub name: &'static str,
    /// third UUID group: 0004 Nano X, 6004 Stax, 3004 Flex, 8004 Nano Gen5
    group: u16,
}

pub const SPECS: [Spec; 4] = [
    Spec { model: "nanoX", name: "Nano X", group: 0x0004 },
    Spec { model: "stax", name: "Stax", group: 0x6004 },
    Spec { model: "flex", name: "Flex", group: 0x3004 },
    Spec { model: "unknown", name: "Nano Gen5", group: 0x8004 },
];

impl Spec {
    /// 13d63400-2c97-GGGG-KKKK-4c6564676572 ("Ledger" in ASCII): K 0 service, 1 notify,
    /// 2 write, 3 write without response.
    fn uuid(&self, kind: u16) -> Uuid {
        let k = kind.to_be_bytes();
        Uuid::from_fields(0x13d6_3400, 0x2c97, self.group, &[k[0], k[1], 0x4c, 0x65, 0x64, 0x67, 0x65, 0x72])
    }
    pub fn service(&self) -> Uuid {
        self.uuid(0)
    }
    pub fn notify(&self) -> Uuid {
        self.uuid(1)
    }
    pub fn write(&self) -> Uuid {
        self.uuid(2)
    }
}

fn spec_for(props: &PeripheralProperties) -> Option<&'static Spec> {
    if let Some(s) = SPECS.iter().find(|s| props.services.contains(&s.service())) {
        return Some(s);
    }
    // Bonded devices BlueZ remembers may not list services until connected.
    let name = props.local_name.as_deref()?.trim_start_matches("Ledger ");
    SPECS.iter().find(|s| s.model != "unknown" && name.starts_with(s.name))
}

pub(crate) fn pairing_err() -> String {
    err("ble-pairing", "Pair your Ledger in Bluetooth settings first")
}

fn ble_off() -> String {
    err("ble-off", "Turn on Bluetooth")
}

fn map_err(e: btleplug::Error) -> String {
    if let btleplug::Error::TimedOut(_) = e {
        return err("timeout", "Your Ledger did not answer over Bluetooth");
    }
    if let btleplug::Error::PermissionDenied = e {
        return err("ble-off", "Allow Bluetooth for Rare Launcher");
    }
    let m = e.to_string();
    let l = m.to_lowercase();
    if ["authenticat", "not paired", "notpaired", "insufficient", "encrypt"].iter().any(|k| l.contains(k)) {
        return pairing_err();
    }
    if l.contains("notready") || l.contains("not ready") || l.contains("powered off") || l.contains("not powered") {
        return ble_off();
    }
    err("disconnected", format!("Bluetooth connection to your Ledger failed, try again ({m})"))
}

static ADAPTER: tokio::sync::OnceCell<Adapter> = tokio::sync::OnceCell::const_new();
/// Peripherals from the last scan, by device id.
static SEEN: Mutex<Option<HashMap<String, Peripheral>>> = Mutex::new(None);

async fn adapter() -> Result<Adapter, String> {
    let a = ADAPTER
        .get_or_try_init(|| async {
            let m = Manager::new().await.map_err(|_| ble_off())?;
            m.adapters().await.map_err(|_| ble_off())?.into_iter().next().ok_or_else(ble_off)
        })
        .await?
        .clone();
    if let Ok(CentralState::PoweredOff) = a.adapter_state().await {
        return Err(ble_off());
    }
    Ok(a)
}

/// "ble:<address>" on Linux; macOS hides addresses, so its peripheral UUID there.
fn ble_id(p: &Peripheral) -> String {
    if cfg!(target_os = "linux") {
        format!("ble:{}", p.address())
    } else {
        format!("ble:{}", p.id())
    }
}

fn paired_map() -> HashMap<String, bool> {
    #[cfg(target_os = "linux")]
    {
        super::bluez_agent::paired_by_address()
    }
    #[cfg(not(target_os = "linux"))]
    {
        HashMap::new()
    }
}

fn remember(id: &str, p: &Peripheral) {
    if let Ok(mut s) = SEEN.lock() {
        s.get_or_insert_with(HashMap::new).insert(id.to_string(), p.clone());
    }
}

async fn describe(p: &Peripheral, paired: &HashMap<String, bool>) -> Option<(LedgerDevice, &'static Spec)> {
    let props = p.properties().await.ok().flatten()?;
    let spec = spec_for(&props)?;
    let id = ble_id(p);
    let name = props.local_name.clone().unwrap_or_else(|| spec.name.to_string());
    let paired = if cfg!(target_os = "linux") { Some(paired.get(&props.address.to_string().to_uppercase()).copied().unwrap_or(false)) } else { None };
    Some((LedgerDevice { id, transport: "ble", name, model: spec.model, paired }, spec))
}

async fn scan(a: &Adapter, ms: u64) -> Result<(), String> {
    let services = SPECS.iter().map(Spec::service).collect();
    a.start_scan(ScanFilter { services }).await.map_err(map_err)?;
    tokio::time::sleep(Duration::from_millis(ms)).await;
    let _ = a.stop_scan().await;
    Ok(())
}

/// Scans for `ms` and returns the Ledgers in range (and ones BlueZ already knows).
pub async fn list(ms: u64) -> Result<Vec<LedgerDevice>, String> {
    let a = adapter().await?;
    scan(&a, ms).await?;
    let paired = tokio::task::spawn_blocking(paired_map).await.unwrap_or_default();
    let mut out = Vec::new();
    for p in a.peripherals().await.map_err(map_err)? {
        if let Some((d, _)) = describe(&p, &paired).await {
            remember(&d.id, &p);
            out.push(d);
        }
    }
    Ok(out)
}

async fn find(a: &Adapter, id: &str) -> Result<Option<Peripheral>, String> {
    if let Some(p) = SEEN.lock().ok().and_then(|s| s.as_ref().and_then(|m| m.get(id).cloned())) {
        return Ok(Some(p));
    }
    Ok(a.peripherals().await.map_err(map_err)?.into_iter().find(|p| ble_id(p) == id))
}

async fn timed<T>(secs: u64, f: impl std::future::Future<Output = btleplug::Result<T>>) -> Result<T, String> {
    match tokio::time::timeout(Duration::from_secs(secs), f).await {
        Ok(r) => r.map_err(map_err),
        Err(_) => Err(err("timeout", "Your Ledger did not answer over Bluetooth. Unlock it and keep it close")),
    }
}

/// Linux: bonds an unpaired Ledger through our BlueZ agent first, sending the comparison code
/// to the UI as event "ledger-pairing" {passkey: "123456"}.
#[cfg(target_os = "linux")]
async fn ensure_paired(sink: &Sink, p: &Peripheral, device: &LedgerDevice) -> Result<(), String> {
    if device.paired != Some(false) {
        return Ok(());
    }
    let sink = sink.clone();
    let addr = p.address().to_string();
    tokio::task::spawn_blocking(move || {
        super::bluez_agent::pair(&addr, &|code| {
            sink(crate::WalletEvent::LedgerPairing { passkey: format!("{code:06}") });
        })
    })
    .await
    .map_err(|_| pairing_err())?
}

/// macOS pairs on the first encrypted write and shows its own prompt.
#[cfg(not(target_os = "linux"))]
async fn ensure_paired(_sink: &Sink, _p: &Peripheral, _device: &LedgerDevice) -> Result<(), String> {
    Ok(())
}

pub async fn open(sink: &Sink, id: &str) -> Result<(BleIo, LedgerDevice), String> {
    let a = adapter().await?;
    let p = match find(&a, id).await? {
        Some(p) => p,
        None => {
            scan(&a, 4000).await?;
            find(&a, id).await?.ok_or_else(|| err("notfound", "Turn on your Ledger, unlock it and keep it close"))?
        }
    };
    let paired = tokio::task::spawn_blocking(paired_map).await.unwrap_or_default();
    let (device, spec) = describe(&p, &paired).await.ok_or_else(|| err("notfound", "This is not a Ledger"))?;

    ensure_paired(sink, &p, &device).await?;
    if !p.is_connected().await.unwrap_or(false) {
        timed(20, p.connect()).await?;
    }
    timed(15, p.discover_services()).await?;
    let chars = p.characteristics();
    let pick = |u: Uuid| -> Result<Characteristic, String> {
        chars.iter().find(|c| c.uuid == u).cloned().ok_or_else(|| err("disconnected", "Your Ledger's Bluetooth service is missing, update it in Ledger Live"))
    };
    let notify = pick(spec.notify())?;
    let write = pick(spec.write())?;
    let mut stream = p.notifications().await.map_err(map_err)?;
    timed(10, p.subscribe(&notify)).await?;

    let (tx, rx) = mpsc::channel();
    let notify_uuid = notify.uuid;
    let pump = tokio::spawn(async move {
        while let Some(n) = stream.next().await {
            if n.uuid == notify_uuid && tx.send(n.value).is_err() {
                break;
            }
        }
    });
    let io = BleIo { p, write, rx, frame: BLE_DEFAULT_FRAME, handle: tokio::runtime::Handle::current(), pump: Some(pump) };
    // MTU query; the Ledger answers on the notify characteristic.
    let io = tokio::task::spawn_blocking(move || {
        let mut io = io;
        io.write(&MTU_REQUEST)?;
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            if let Some(f) = io.read(Duration::from_millis(250))? {
                if let Some(size) = parse_mtu_reply(&f) {
                    io.frame = size;
                    break;
                }
            }
        }
        Ok::<_, String>(io)
    })
    .await
    .map_err(|_| err("error", "Bluetooth task failed"))??;
    remember(&device.id, &io.p);
    Ok((io, LedgerDevice { paired: device.paired.map(|_| true), ..device }))
}

pub struct BleIo {
    p: Peripheral,
    write: Characteristic,
    rx: mpsc::Receiver<Vec<u8>>,
    frame: usize,
    handle: tokio::runtime::Handle,
    pump: Option<tokio::task::JoinHandle<()>>,
}

impl FrameIo for BleIo {
    // Runs on a blocking thread (see ledger_exchange), so block_on is allowed here.
    fn write(&mut self, frame: &[u8]) -> Result<(), String> {
        let fut = self.p.write(&self.write, frame, WriteType::WithResponse);
        match self.handle.block_on(async { tokio::time::timeout(Duration::from_secs(15), fut).await }) {
            Ok(r) => r.map_err(map_err),
            Err(_) => Err(err("timeout", "Your Ledger did not answer over Bluetooth")),
        }
    }

    fn read(&mut self, wait: Duration) -> Result<Option<Vec<u8>>, String> {
        match self.rx.recv_timeout(wait) {
            Ok(f) => Ok(Some(f)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // BlueZ does not always end the notification stream on a drop; ask.
                let p = self.p.clone();
                if !self.handle.block_on(async move { p.is_connected().await.unwrap_or(false) }) {
                    return Err(err("disconnected", "Your Ledger disconnected"));
                }
                Ok(None)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(err("disconnected", "Your Ledger disconnected")),
        }
    }

    fn ble_frame(&self) -> Option<usize> {
        Some(self.frame)
    }
}

impl Drop for BleIo {
    fn drop(&mut self) {
        if let Some(pump) = self.pump.take() {
            pump.abort();
        }
        let p = self.p.clone();
        self.handle.spawn(async move {
            let _ = p.disconnect().await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_match_ledger_bluetooth_spec() {
        let s = &SPECS[0];
        assert_eq!(s.service().to_string(), "13d63400-2c97-0004-0000-4c6564676572");
        assert_eq!(s.notify().to_string(), "13d63400-2c97-0004-0001-4c6564676572");
        assert_eq!(s.write().to_string(), "13d63400-2c97-0004-0002-4c6564676572");
        assert_eq!(SPECS[1].service().to_string(), "13d63400-2c97-6004-0000-4c6564676572");
        assert_eq!(SPECS[2].service().to_string(), "13d63400-2c97-3004-0000-4c6564676572");
        assert_eq!(SPECS[3].service().to_string(), "13d63400-2c97-8004-0000-4c6564676572");
    }

    #[test]
    fn spec_from_services_or_name() {
        let p = PeripheralProperties { services: vec![SPECS[1].service()], ..Default::default() };
        assert_eq!(spec_for(&p).unwrap().model, "stax");
        let mut p = PeripheralProperties { local_name: Some("Nano X 4F2A".into()), ..Default::default() };
        assert_eq!(spec_for(&p).unwrap().model, "nanoX");
        p.local_name = Some("Ledger Flex 12AB".into());
        assert_eq!(spec_for(&p).unwrap().model, "flex");
        p.local_name = Some("Headphones".into());
        assert!(spec_for(&p).is_none());
    }
}
