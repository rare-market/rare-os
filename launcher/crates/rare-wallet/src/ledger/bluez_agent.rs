//! BlueZ pairing for the Ledger BLE transport (Linux only).
//!
//! Ledgers need a bonded link. In game mode no desktop Bluetooth agent may be running, so
//! implicit pairing on the first encrypted write would just fail. Instead, before connecting to
//! an unpaired Ledger, we register our own org.bluez.Agent1 (DisplayYesNo, not the default agent)
//! and call Device1.Pair from the same D-Bus connection, so BlueZ asks us. The six-digit code is
//! sent to the UI ("ledger-pairing") to compare with the Ledger, and the confirmation is accepted
//! here; the person still approves on the Ledger. Requests for any other device are rejected.

use dbus::arg::{prop_cast, PropMap};
use dbus::blocking::Connection;
use dbus::channel::{BusType, Channel};
use dbus::message::MessageType;
use dbus::strings::ErrorName;
use dbus::{Message, Path};
use std::collections::HashMap;
use std::ffi::CString;
use std::time::{Duration, Instant};

const AGENT_PATH: &str = "/one/rare/launcher/ledger_agent";
const PAIR_TIMEOUT: Duration = Duration::from_secs(90);

type Objects = HashMap<Path<'static>, HashMap<String, PropMap>>;

fn devices() -> Vec<(Path<'static>, String, bool)> {
    let Ok(conn) = Connection::new_system() else { return vec![] };
    let proxy = conn.with_proxy("org.bluez", "/", Duration::from_secs(5));
    let Ok((objs,)): Result<(Objects,), _> = proxy.method_call("org.freedesktop.DBus.ObjectManager", "GetManagedObjects", ()) else {
        return vec![];
    };
    objs.into_iter()
        .filter_map(|(path, ifaces)| {
            let dev = ifaces.get("org.bluez.Device1")?;
            let addr = prop_cast::<String>(dev, "Address")?.to_uppercase();
            let paired = prop_cast::<bool>(dev, "Paired").copied().unwrap_or(false);
            Some((path, addr, paired))
        })
        .collect()
}

/// Bluetooth address (upper case) to BlueZ's Paired flag.
pub fn paired_by_address() -> HashMap<String, bool> {
    devices().into_iter().map(|(_, a, p)| (a, p)).collect()
}

/// Pairs (bonds) the device with this address, reporting the comparison code through `on_code`.
/// Ok when already paired or unknown to BlueZ (then the connect attempt reports the problem).
pub fn pair(address: &str, on_code: &dyn Fn(u32)) -> Result<(), String> {
    let address = address.to_uppercase();
    let Some((dev, _, paired)) = devices().into_iter().find(|(_, a, _)| *a == address) else {
        return Ok(());
    };
    if paired {
        return Ok(());
    }
    let ch = Channel::get_private(BusType::System).map_err(|_| super::ble::pairing_err())?;
    let agent = Path::new(AGENT_PATH).expect("static path");
    let reg = Message::new_method_call("org.bluez", "/org/bluez", "org.bluez.AgentManager1", "RegisterAgent")
        .expect("static names")
        .append2(agent.clone(), "DisplayYesNo");
    if let Err(name) = call(&ch, reg, Duration::from_secs(5), &dev, on_code) {
        if name != "org.bluez.Error.AlreadyExists" {
            log::warn!("bluez agent: register failed: {name}");
            return Err(super::ble::pairing_err());
        }
    }
    let pair = Message::new_method_call("org.bluez", dev.clone(), "org.bluez.Device1", "Pair").expect("static names");
    let res = call(&ch, pair, PAIR_TIMEOUT, &dev, on_code);
    let unreg = Message::new_method_call("org.bluez", "/org/bluez", "org.bluez.AgentManager1", "UnregisterAgent")
        .expect("static names")
        .append1(agent);
    let _ = call(&ch, unreg, Duration::from_secs(5), &dev, on_code);
    match res {
        Ok(()) => Ok(()),
        Err(name) if name == "org.bluez.Error.AlreadyExists" => Ok(()),
        Err(name) => {
            log::info!("ledger pairing failed: {name}");
            if name.contains("Authentication") || name.contains("Timeout") {
                Err(crate::err("ble-pairing", "Pairing did not finish. Check the code on your Ledger and try again"))
            } else {
                Err(super::ble::pairing_err())
            }
        }
    }
}

/// Sends `msg` and serves agent calls until its reply arrives. Err carries the D-Bus error name.
fn call(ch: &Channel, msg: Message, timeout: Duration, dev: &Path, on_code: &dyn Fn(u32)) -> Result<(), String> {
    let serial = ch.send(msg).map_err(|_| "send failed".to_string())?;
    let deadline = Instant::now() + timeout;
    loop {
        while let Some(mut m) = ch.pop_message() {
            if m.get_reply_serial() == Some(serial) {
                if m.msg_type() == MessageType::Error {
                    let name = m.as_result().err().and_then(|e| e.name().map(str::to_string)).unwrap_or_default();
                    return Err(name);
                }
                return Ok(());
            }
            if m.msg_type() == MessageType::MethodCall {
                if let Some(reply) = agent_reply(&m, dev, on_code) {
                    let _ = ch.send(reply);
                }
            }
        }
        if Instant::now() > deadline {
            return Err("org.bluez.Error.Timeout".into());
        }
        ch.read_write(Some(Duration::from_millis(200))).map_err(|_| "bus closed".to_string())?;
    }
}

fn reject(m: &Message) -> Message {
    m.error(&ErrorName::from("org.bluez.Error.Rejected"), &CString::new("Rejected").expect("static"))
}

fn agent_reply(m: &Message, dev: &Path, on_code: &dyn Fn(u32)) -> Option<Message> {
    if m.path().as_deref() != Some(AGENT_PATH) {
        return None;
    }
    if m.interface().as_deref() != Some("org.bluez.Agent1") {
        return Some(reject(m));
    }
    let member = m.member()?.to_string();
    let ours = || m.read1::<Path>().map(|p| p == *dev).unwrap_or(false);
    Some(match member.as_str() {
        "RequestConfirmation" => match m.read2::<Path, u32>() {
            Ok((p, code)) if p == *dev => {
                on_code(code);
                m.method_return()
            }
            _ => reject(m),
        },
        "DisplayPasskey" => {
            if let Ok((p, code, _)) = m.read3::<Path, u32, u16>() {
                if p == *dev {
                    on_code(code);
                }
            }
            m.method_return()
        }
        "RequestAuthorization" | "AuthorizeService" if ours() => m.method_return(),
        "Release" | "Cancel" | "DisplayPinCode" => m.method_return(),
        _ => reject(m),
    })
}
