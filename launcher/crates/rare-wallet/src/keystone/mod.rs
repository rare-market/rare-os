//! Keystone 3 Pro over USB (nusb: usbfs on Linux, IOKit on macOS; no libusb).
//!
//! JS sends a command (1 echo, 2 resolve UR, 3 lock status, 5 device info) with a string, Rust
//! frames it as EAPDU packets on interface 0 (bulk OUT 0x03 / IN 0x83, 64-byte packets) and
//! returns the device's JSON answer text. Like @keystonehq/hw-transport-webusb, the interface is
//! claimed for one request and released afterwards.

pub mod eapdu;

use crate::err;
use eapdu::{encode, payload_text, status_error, Collector, MAX_PACKETS};
use serde::Serialize;
use std::future::Future;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

pub const KEYSTONE_VID: u16 = 0x1209;
pub const KEYSTONE_PID: u16 = 0x3001;
const INTERFACE: u8 = 0;
const EP_OUT: u8 = 0x03;
const EP_IN: u8 = 0x83;
const PACKET: usize = 64;

/// Serializes requests: the Keystone handles one at a time.
#[derive(Default)]
pub struct KeystoneState {
    lock: tokio::sync::Mutex<()>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KeystoneDevice {
    pub id: String,
    pub name: String,
}

/// Packet-level link. nusb in the app; a scripted fake in tests.
pub(crate) trait PacketIo: Send {
    fn write(&mut self, packet: Vec<u8>) -> impl Future<Output = Result<(), String>> + Send;
    /// Next IN packet (waits until one arrives).
    fn read(&mut self) -> impl Future<Output = Result<Vec<u8>, String>> + Send;
}

fn not_found() -> String {
    err("notfound", "Connect your Keystone, unlock it and turn on USB in Settings > Connection")
}

/// One request and its answer. Ok is the JSON text of a success response.
pub(crate) async fn transact<I: PacketIo>(io: &mut I, command: u16, request_id: u16, data: &[u8], timeout: Duration) -> Result<String, String> {
    let packets = encode(command, request_id, data);
    if packets.len() > MAX_PACKETS {
        return Err(err("error", "Request too large for Keystone"));
    }
    let work = async {
        for p in packets {
            io.write(p).await?;
        }
        let mut c = Collector::new(command, request_id);
        loop {
            let raw = io.read().await?;
            if let Some((status, body)) = c.push(&raw) {
                let text = String::from_utf8_lossy(&body).into_owned();
                if status == eapdu::status::SUCCESS {
                    return Ok(text);
                }
                return Err(status_error(status, payload_text(&text).as_deref()));
            }
        }
    };
    match tokio::time::timeout(timeout, work).await {
        Ok(r) => r,
        Err(_) => Err(err("timeout", "No answer from your Keystone")),
    }
}

fn devices() -> Result<Vec<nusb::DeviceInfo>, String> {
    let list = nusb::list_devices().map_err(|e| err("error", format!("USB is not available: {e}")))?;
    Ok(list.filter(|d| d.vendor_id() == KEYSTONE_VID && d.product_id() == KEYSTONE_PID).collect())
}

fn describe(i: usize, d: &nusb::DeviceInfo) -> KeystoneDevice {
    let id = match d.serial_number() {
        Some(s) if !s.is_empty() => format!("usb:{s}"),
        _ => format!("usb:{i}"),
    };
    KeystoneDevice { id, name: d.product_string().filter(|s| !s.is_empty()).unwrap_or("Keystone 3 Pro").to_string() }
}

struct UsbIo {
    iface: nusb::Interface,
    inq: nusb::transfer::Queue<nusb::transfer::RequestBuffer>,
}

fn open_usb() -> Result<UsbIo, String> {
    let info = devices()?.into_iter().next().ok_or_else(not_found)?;
    let dev = info.open().map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => crate::ledger::udev_err(),
        _ => not_found(),
    })?;
    let claimed = dev.claim_interface(INTERFACE);
    #[cfg(target_os = "linux")]
    let claimed = claimed.or_else(|_| dev.detach_and_claim_interface(INTERFACE));
    let iface = claimed.map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => crate::ledger::udev_err(),
        _ => err("busy", "Another app is using your Keystone. Close it and try again"),
    })?;
    let inq = iface.bulk_in_queue(EP_IN);
    Ok(UsbIo { iface, inq })
}

fn usb_lost() -> String {
    err("disconnected", "Your Keystone was disconnected")
}

impl PacketIo for UsbIo {
    fn write(&mut self, packet: Vec<u8>) -> impl Future<Output = Result<(), String>> + Send {
        let fut = self.iface.bulk_out(EP_OUT, packet);
        async move { fut.await.status.map_err(|_| usb_lost()) }
    }

    async fn read(&mut self) -> Result<Vec<u8>, String> {
        loop {
            if self.inq.pending() == 0 {
                self.inq.submit(nusb::transfer::RequestBuffer::new(PACKET));
            }
            let c = self.inq.next_complete().await;
            c.status.map_err(|_| usb_lost())?;
            if !c.data.is_empty() {
                return Ok(c.data);
            }
        }
    }
}

impl Drop for UsbIo {
    fn drop(&mut self) {
        // Pending IN transfers would otherwise outlive the request.
        self.inq.cancel_all();
    }
}

fn next_request_id() -> u16 {
    static N: AtomicU16 = AtomicU16::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    (t as u16) ^ N.fetch_add(0x9e37, Ordering::Relaxed)
}

impl KeystoneState {
    pub async fn list() -> Result<Vec<KeystoneDevice>, String> {
        let found = tokio::task::spawn_blocking(devices).await.map_err(|_| err("error", "USB scan failed"))??;
        Ok(found.iter().enumerate().map(|(i, d)| describe(i, d)).collect())
    }

    /// command: 1 echo, 2 resolve UR, 3 lock status, 5 version. Returns the answer's JSON text.
    pub async fn send(&self, command: u16, data: &str, timeout_ms: u64) -> Result<String, String> {
        let _one = self.lock.lock().await;
        let mut io = tokio::task::spawn_blocking(open_usb).await.map_err(|_| err("error", "USB open failed"))??;
        let timeout = Duration::from_millis(timeout_ms.clamp(1_000, 30 * 60 * 1000));
        transact(&mut io, command, next_request_id(), data.as_bytes(), timeout).await
    }
}

/// The Keystone over USB as a `KeystoneLink` for the signing layer (`crate::keystone_sui`), run
/// from a plain worker thread on `rt`.
pub struct KeystoneUsb<'a> {
    pub state: &'a KeystoneState,
    pub rt: tokio::runtime::Handle,
}

impl crate::keystone_sui::KeystoneLink for KeystoneUsb<'_> {
    fn send(&mut self, command: u16, data: &str, timeout_ms: u64) -> Result<String, crate::keystone_sui::KeystoneError> {
        self.rt.block_on(self.state.send(command, data, timeout_ms)).map_err(|e| crate::keystone_sui::KeystoneError::from_native(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::eapdu::tests::respond;
    use super::*;
    use std::collections::VecDeque;

    /// Scripted Keystone: collects request packets; once the last one arrives, queues `reply`
    /// (status, body) packets after optional noise packets.
    struct FakeKeystone {
        sent: Vec<Vec<u8>>,
        reply: Option<(u16, String)>,
        noise: Vec<Vec<u8>>,
        inbox: VecDeque<Vec<u8>>,
    }

    impl PacketIo for FakeKeystone {
        fn write(&mut self, packet: Vec<u8>) -> impl Future<Output = Result<(), String>> + Send {
            let p = eapdu::parse(&[packet.clone(), vec![0, 0]].concat()).unwrap();
            self.sent.push(packet);
            if p.index + 1 == p.total {
                self.inbox.extend(self.noise.drain(..));
                if let Some((st, body)) = self.reply.take() {
                    self.inbox.extend(respond(p.command, p.request_id, st, body.as_bytes()));
                }
            }
            async { Ok(()) }
        }
        fn read(&mut self) -> impl Future<Output = Result<Vec<u8>, String>> + Send {
            let next = self.inbox.pop_front();
            async move {
                match next {
                    Some(p) => Ok(p),
                    // Nothing more: hang like a device waiting for the person.
                    None => std::future::pending().await,
                }
            }
        }
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap()
    }

    #[test]
    fn transact_multi_packet_success() {
        let ur = "UR:SUI-SIGN-REQUEST/".to_string() + &"A".repeat(400);
        let answer = format!("{{\"payload\":\"UR:SUI-SIGNATURE/{}\"}}", "B".repeat(200));
        let mut dev = FakeKeystone {
            sent: vec![],
            reply: Some((0, answer.clone())),
            noise: respond(2, 0x0bad, 0, b"{\"payload\":\"stale\"}"),
            inbox: VecDeque::new(),
        };
        let out = rt().block_on(transact(&mut dev, 2, 0x1234, ur.as_bytes(), Duration::from_secs(5))).unwrap();
        assert_eq!(out, answer);
        assert_eq!(dev.sent.len(), ur.len().div_ceil(55));
        assert!(dev.sent.iter().all(|p| p.len() <= 64));
    }

    #[test]
    fn transact_maps_device_errors() {
        let run = |st: u16, msg: &str| {
            let mut dev = FakeKeystone { sent: vec![], reply: Some((st, format!("{{\"payload\":\"{msg}\"}}"))), noise: vec![], inbox: VecDeque::new() };
            rt().block_on(transact(&mut dev, 2, 7, b"UR:X/Y", Duration::from_secs(5))).unwrap_err()
        };
        assert!(run(6, "Device is locked").starts_with("locked|"));
        assert!(run(4, "Rejected").starts_with("rejected|"));
        assert!(run(6, "Export address is just allowed on specific pages").starts_with("usb-off|"));
    }

    #[test]
    fn transact_times_out() {
        let mut dev = FakeKeystone { sent: vec![], reply: None, noise: vec![], inbox: VecDeque::new() };
        let e = rt().block_on(transact(&mut dev, 3, 1, b"", Duration::from_millis(50))).unwrap_err();
        assert!(e.starts_with("timeout|"), "{e}");
        assert_eq!(dev.sent, vec![vec![0, 0, 3, 0, 1, 0, 0, 0, 1]]);
    }

    #[test]
    fn device_json_shape() {
        let d = KeystoneDevice { id: "usb:ABC".into(), name: "Keystone 3 Pro".into() };
        assert_eq!(serde_json::to_value(&d).unwrap(), serde_json::json!({"id":"usb:ABC","name":"Keystone 3 Pro"}));
    }
}
