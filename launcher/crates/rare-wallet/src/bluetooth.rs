//! Turning the Bluetooth adapter on from the launcher, for the phone-passkey (hybrid/caBLE) and
//! Ledger BLE flows. On the SuiPlay the adapter often starts powered off. BlueZ's default D-Bus
//! policy allows the session user to send to org.bluez, so setting Adapter1.Powered = true over
//! the system bus works without sudo (verified on the device). rfkill is tried first only when the
//! adapter is soft-blocked; the Powered state is separate from the rfkill switch.
//!
//! No-op with Ok on macOS, where these flows are not used in a dev build.

use serde::Serialize;

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct BtState {
    /// a Bluetooth adapter exists
    pub present: bool,
    /// the adapter is powered on
    pub powered: bool,
    /// an rfkill soft block is set (airplane mode and the like)
    pub blocked: bool,
}

pub fn state_now() -> Result<BtState, String> {
    imp::state()
}

pub async fn state() -> Result<BtState, String> {
    tokio::task::spawn_blocking(imp::state).await.map_err(|e| e.to_string())?
}

/// Powers the adapter on and returns the state afterwards. Error codes for the JS side:
/// "no-adapter", "bt-blocked" (a hardware/airplane switch we cannot clear), "bt-denied"
/// (the system refused without privilege), "bt-failed".
pub async fn power_on() -> Result<BtState, String> {
    tokio::task::spawn_blocking(imp::power_on).await.map_err(|e| e.to_string())?
}

/// Powers the adapter on or off (Quick Settings' Bluetooth tile). On needs the same steps as
/// `bluetooth_power_on`; off sets Adapter1.Powered = false.
pub fn set_powered(on: bool) -> Result<BtState, String> {
    if on {
        imp::power_on()
    } else {
        imp::power_off()
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::BtState;
    use crate::err;
    use dbus::arg::{prop_cast, PropMap, Variant};
    use dbus::blocking::Connection;
    use dbus::Path;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    type Objects = HashMap<Path<'static>, HashMap<String, PropMap>>;
    const T: Duration = Duration::from_secs(5);

    /// The first org.bluez.Adapter1 object path and its current Powered value.
    fn adapter(conn: &Connection) -> Option<(Path<'static>, bool)> {
        let proxy = conn.with_proxy("org.bluez", "/", T);
        let (objs,): (Objects,) = proxy.method_call("org.freedesktop.DBus.ObjectManager", "GetManagedObjects", ()).ok()?;
        objs.into_iter().find_map(|(path, ifaces)| {
            let a = ifaces.get("org.bluez.Adapter1")?;
            Some((path, prop_cast::<bool>(a, "Powered").copied().unwrap_or(false)))
        })
    }

    pub fn state() -> Result<BtState, String> {
        let Ok(conn) = Connection::new_system() else {
            return Ok(BtState { present: false, powered: false, blocked: false });
        };
        match adapter(&conn) {
            Some((_, powered)) => Ok(BtState { present: true, powered, blocked: rfkill_soft_blocked() }),
            None => Ok(BtState { present: false, powered: false, blocked: false }),
        }
    }

    pub fn power_on() -> Result<BtState, String> {
        let conn = Connection::new_system().map_err(|_| err("bt-failed", "Bluetooth is not available"))?;
        let Some((path, powered)) = adapter(&conn) else {
            return Err(err("no-adapter", "No Bluetooth adapter found"));
        };
        if powered {
            return Ok(BtState { present: true, powered: true, blocked: false });
        }
        // Clear a soft rfkill block first; a hard (physical switch) block we cannot clear.
        if rfkill_soft_blocked() {
            let _ = rfkill_unblock();
        }
        let proxy = conn.with_proxy("org.bluez", &path, T);
        let set: Result<(), dbus::Error> =
            proxy.method_call("org.freedesktop.DBus.Properties", "Set", ("org.bluez.Adapter1", "Powered", Variant(true)));
        if let Err(e) = set {
            let name = e.name().unwrap_or("");
            if name.contains("AccessDenied") || name.contains("AuthorizationRequired") {
                return Err(err("bt-denied", "Turn Bluetooth on in Settings, then come back"));
            }
            if rfkill_hard_blocked() {
                return Err(err("bt-blocked", "Bluetooth is blocked by a hardware switch"));
            }
            return Err(err("bt-failed", "Could not turn Bluetooth on"));
        }
        // BlueZ brings the controller up asynchronously; wait briefly for Powered to flip.
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            if let Some((_, true)) = adapter(&conn) {
                return Ok(BtState { present: true, powered: true, blocked: false });
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        if rfkill_hard_blocked() {
            return Err(err("bt-blocked", "Bluetooth is blocked by a hardware switch"));
        }
        Ok(BtState { present: true, powered: false, blocked: rfkill_soft_blocked() })
    }

    pub fn power_off() -> Result<BtState, String> {
        let conn = Connection::new_system().map_err(|_| err("bt-failed", "Bluetooth is not available"))?;
        let Some((path, powered)) = adapter(&conn) else {
            return Err(err("no-adapter", "No Bluetooth adapter found"));
        };
        if powered {
            let proxy = conn.with_proxy("org.bluez", &path, T);
            let set: Result<(), dbus::Error> =
                proxy.method_call("org.freedesktop.DBus.Properties", "Set", ("org.bluez.Adapter1", "Powered", Variant(false)));
            if set.is_err() {
                return Err(err("bt-failed", "Could not turn Bluetooth off"));
            }
        }
        Ok(BtState { present: true, powered: false, blocked: rfkill_soft_blocked() })
    }

    // rfkill via sysfs (readable) and /dev/rfkill (uaccess for the session user on Bazzite).
    fn rfkill_type(idx: &str) -> String {
        std::fs::read_to_string(format!("/sys/class/rfkill/{idx}/type")).unwrap_or_default().trim().to_string()
    }
    fn bt_rfkill() -> Option<String> {
        std::fs::read_dir("/sys/class/rfkill").ok()?.flatten().find_map(|e| {
            let name = e.file_name().into_string().ok()?;
            (rfkill_type(&name) == "bluetooth").then_some(name)
        })
    }
    fn soft_of(idx: &str) -> bool {
        std::fs::read_to_string(format!("/sys/class/rfkill/{idx}/soft")).map(|s| s.trim() == "1").unwrap_or(false)
    }
    fn hard_of(idx: &str) -> bool {
        std::fs::read_to_string(format!("/sys/class/rfkill/{idx}/hard")).map(|s| s.trim() == "1").unwrap_or(false)
    }
    fn rfkill_soft_blocked() -> bool {
        bt_rfkill().map(|i| soft_of(&i)).unwrap_or(false)
    }
    fn rfkill_hard_blocked() -> bool {
        bt_rfkill().map(|i| hard_of(&i)).unwrap_or(false)
    }
    /// struct rfkill_event (packed): u32 idx, u8 type, u8 op, u8 soft, u8 hard.
    /// op 3 = RFKILL_OP_CHANGE_ALL, type 2 = RFKILL_TYPE_BLUETOOTH, soft 0 = unblock.
    const UNBLOCK_BT_EVENT: [u8; 8] = [0, 0, 0, 0, 2, 3, 0, 0];

    /// Clears a soft block by writing an RFKILL_OP_CHANGE_ALL unblock event to /dev/rfkill.
    fn rfkill_unblock() -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).open("/dev/rfkill")?;
        f.write_all(&UNBLOCK_BT_EVENT)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn rfkill_unblock_event_layout() {
            // idx 0, type=bluetooth(2), op=change_all(3), soft=0 (unblock), hard=0
            assert_eq!(UNBLOCK_BT_EVENT, [0u8, 0, 0, 0, 2, 3, 0, 0]);
            assert_eq!(UNBLOCK_BT_EVENT[4], 2, "RFKILL_TYPE_BLUETOOTH");
            assert_eq!(UNBLOCK_BT_EVENT[5], 3, "RFKILL_OP_CHANGE_ALL");
            assert_eq!(UNBLOCK_BT_EVENT[6], 0, "soft = unblock");
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::BtState;
    pub fn state() -> Result<BtState, String> {
        Ok(BtState { present: true, powered: true, blocked: false })
    }
    pub fn power_on() -> Result<BtState, String> {
        Ok(BtState { present: true, powered: true, blocked: false })
    }
    pub fn power_off() -> Result<BtState, String> {
        Ok(BtState { present: true, powered: false, blocked: false })
    }
}
