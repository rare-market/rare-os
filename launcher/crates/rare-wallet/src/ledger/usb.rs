//! Ledger over USB HID (hidapi; hidraw on Linux, IOKit on macOS).

use super::{model_from_pid, FrameIo, LedgerDevice};
use crate::err;
use hidapi::{HidApi, HidDevice};
use std::ffi::CString;
use std::time::Duration;

pub const LEDGER_VID: u16 = 0x2c97;
/// The APDU interface. Interface 1 is FIDO (usage page 0xf1d0) and must not be picked.
const APDU_USAGE_PAGE: u16 = 0xffa0;

fn api() -> Result<HidApi, String> {
    HidApi::new().map_err(|e| err("error", format!("USB is not available: {e}")))
}

/// Ledger APDU interfaces, one per device.
fn ledgers(api: &HidApi) -> Vec<(String, u16, Option<String>)> {
    let mut out: Vec<(String, u16, Option<String>)> = Vec::new();
    for d in api.device_list() {
        if d.vendor_id() != LEDGER_VID {
            continue;
        }
        // hidraw may not report the usage page; interface 0 is the APDU one on every model.
        if d.usage_page() != APDU_USAGE_PAGE && d.interface_number() != 0 {
            continue;
        }
        let path = d.path().to_string_lossy().into_owned();
        if out.iter().any(|(p, _, _)| *p == path) {
            continue;
        }
        out.push((path, d.product_id(), d.product_string().map(str::to_string)));
    }
    out
}

/// True when this user may not open the hidraw node (missing udev rule).
#[cfg(target_os = "linux")]
fn permission_denied(path: &str) -> bool {
    match std::fs::OpenOptions::new().read(true).write(true).open(path) {
        Ok(_) => false,
        Err(e) => e.kind() == std::io::ErrorKind::PermissionDenied,
    }
}

#[cfg(not(target_os = "linux"))]
fn permission_denied(_path: &str) -> bool {
    false
}

fn device(path: &str, pid: u16) -> LedgerDevice {
    let (model, name) = model_from_pid(pid);
    LedgerDevice { id: format!("usb:{path}"), transport: "usb", name: name.to_string(), model, paired: None }
}

/// Plugged-in Ledgers. Err "udev" when one is plugged in but this user cannot open it.
pub fn list() -> Result<Vec<LedgerDevice>, String> {
    let api = api()?;
    let found = ledgers(&api);
    if !found.is_empty() && found.iter().all(|(p, _, _)| permission_denied(p)) {
        return Err(super::udev_err());
    }
    Ok(found.into_iter().filter(|(p, _, _)| !permission_denied(p)).map(|(p, pid, _)| device(&p, pid)).collect())
}

pub fn open(path: &str) -> Result<(UsbIo, LedgerDevice), String> {
    let api = api()?;
    let Some((path, pid, _)) = ledgers(&api).into_iter().find(|(p, _, _)| p == path) else {
        return Err(err("notfound", "Connect your Ledger and unlock it"));
    };
    if permission_denied(&path) {
        return Err(super::udev_err());
    }
    let c = CString::new(path.clone()).map_err(|_| err("notfound", "Connect your Ledger and unlock it"))?;
    let dev = api.open_path(&c).map_err(|e| {
        let m = e.to_string();
        if m.contains("Permission denied") {
            super::udev_err()
        } else if m.contains("xclusive") || m.contains("E00002C5") || m.contains("busy") {
            // macOS opens HID devices exclusively: Ledger Live or a browser holds it.
            err("busy", "Another app is using your Ledger. Close it and try again")
        } else {
            err("notfound", "Connect your Ledger and unlock it")
        }
    })?;
    Ok((UsbIo { dev }, device(&path, pid)))
}

pub struct UsbIo {
    dev: HidDevice,
}

impl FrameIo for UsbIo {
    fn write(&mut self, frame: &[u8]) -> Result<(), String> {
        // Report id 0 first, as @ledgerhq/hw-transport-node-hid writes it.
        let mut buf = Vec::with_capacity(frame.len() + 1);
        buf.push(0);
        buf.extend_from_slice(frame);
        self.dev.write(&buf).map(|_| ()).map_err(|_| err("disconnected", "Your Ledger was disconnected"))
    }

    fn read(&mut self, wait: Duration) -> Result<Option<Vec<u8>>, String> {
        let mut buf = [0u8; 64];
        match self.dev.read_timeout(&mut buf, wait.as_millis() as i32) {
            Ok(0) => Ok(None),
            Ok(n) => Ok(Some(buf[..n].to_vec())),
            Err(_) => Err(err("disconnected", "Your Ledger was disconnected")),
        }
    }

    fn ble_frame(&self) -> Option<usize> {
        None
    }
}
