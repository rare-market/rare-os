//! org.rare.Fan on the system bus: /org/rare/Fan (org.rare.Fan) and /org/rare/Battery
//! (org.rare.Battery).
//!
//! org.rare.Fan methods (polkit action org.rare.fan.set; root always, the active local session
//! without a prompt, nobody else):
//!   SetPreset(s preset)            "default", "cool" or "quiet"
//!   SetCurve(a(uu) points)         custom curve, (edge C, duty %) points
//! Properties (read-only, anyone):
//!   Preset s, Curve a(uu), Mode s ("ec", "curve", "emergency", "failsafe"),
//!   Temperature d (C, -1 unknown), Sensor s ("edge" or "tctl"), Duty i (%, -1 unknown),
//!   FanSpeed i (rpm, -1 unknown)
//!
//! org.rare.Battery (polkit action org.rare.battery.set, same rule):
//!   SetCare(b on)                  hold the charge just under the limit on AC
//!   SetLimit(u percent)            the limit, 60..=95 (80 by default); it charges again 5 under it
//! Properties (read-only, anyone):
//!   Care b, Available b (BAT0 has charge_behaviour), State s ("off", "unavailable",
//!   "unplugged", "low", "hot", "unknown", "charging", "holding"), Health d (%, -1 unknown),
//!   Limit u (%), Cycles d (charge cycles counted by rare-fand, -1 unknown)

use crate::curve::Preset;
use crate::daemon::Daemon;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use zbus::message::Header;
use zbus::names::BusName;
use zbus::zvariant::Value;
use zbus::{fdo, interface, Connection};

pub const NAME: &str = "org.rare.Fan";
pub const PATH: &str = "/org/rare/Fan";
pub const ACTION: &str = "org.rare.fan.set";
pub const BATTERY_PATH: &str = "/org/rare/Battery";
pub const BATTERY_ACTION: &str = "org.rare.battery.set";

struct FanIface {
    d: Arc<Mutex<Daemon>>,
}

impl FanIface {
    fn lock(&self) -> MutexGuard<'_, Daemon> {
        self.d.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Root, or whatever polkit says for the action without user interaction (no prompts: the rule
/// in 50-rare-fan.rules lets the active local session through).
async fn authorize(conn: &Connection, hdr: &Header<'_>, action: &str) -> fdo::Result<()> {
    let sender = hdr.sender().ok_or_else(|| fdo::Error::AccessDenied("no sender".into()))?.to_owned();
    let dbus = fdo::DBusProxy::new(conn).await?;
    if dbus.get_connection_unix_user(BusName::from(sender.clone())).await? == 0 {
        return Ok(());
    }
    let mut subject: HashMap<&str, Value<'_>> = HashMap::new();
    subject.insert("name", Value::from(sender.as_str()));
    let details: HashMap<&str, &str> = HashMap::new();
    let reply = conn
        .call_method(
            Some("org.freedesktop.PolicyKit1"),
            "/org/freedesktop/PolicyKit1/Authority",
            Some("org.freedesktop.PolicyKit1.Authority"),
            "CheckAuthorization",
            &(("system-bus-name", subject), action, details, 0u32, ""),
        )
        .await
        .map_err(|e| fdo::Error::AccessDenied(format!("polkit: {e}")))?;
    let (ok, _challenge, _details): (bool, bool, HashMap<String, String>) =
        reply.body().deserialize().map_err(|e| fdo::Error::AccessDenied(format!("polkit reply: {e}")))?;
    if ok {
        Ok(())
    } else {
        Err(fdo::Error::AccessDenied("only the active local session can change this".into()))
    }
}

#[interface(name = "org.rare.Fan")]
impl FanIface {
    async fn set_preset(&self, #[zbus(header)] hdr: Header<'_>, #[zbus(connection)] conn: &Connection, preset: String) -> fdo::Result<()> {
        authorize(conn, &hdr, ACTION).await?;
        let p = Preset::parse(&preset).ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown preset {preset:?}: use default, cool or quiet")))?;
        self.lock().set_preset(p).map_err(fdo::Error::InvalidArgs)
    }

    async fn set_curve(&self, #[zbus(header)] hdr: Header<'_>, #[zbus(connection)] conn: &Connection, points: Vec<(u32, u32)>) -> fdo::Result<()> {
        authorize(conn, &hdr, ACTION).await?;
        self.lock().set_preset(Preset::Custom(points)).map_err(fdo::Error::InvalidArgs)
    }

    #[zbus(property)]
    fn preset(&self) -> String {
        self.lock().preset().name().to_string()
    }

    #[zbus(property)]
    fn curve(&self) -> Vec<(u32, u32)> {
        self.lock().preset().curve().map(<[_]>::to_vec).unwrap_or_default()
    }

    #[zbus(property)]
    fn mode(&self) -> String {
        self.lock().status.mode.map_or("starting", |m| m.name()).to_string()
    }

    #[zbus(property)]
    fn temperature(&self) -> f64 {
        self.lock().status.temp.unwrap_or(-1.0)
    }

    #[zbus(property)]
    fn sensor(&self) -> String {
        self.lock().status.sensor.to_string()
    }

    #[zbus(property)]
    fn duty(&self) -> i32 {
        self.lock().status.duty.map_or(-1, |d| d as i32)
    }

    #[zbus(property)]
    fn fan_speed(&self) -> i32 {
        self.lock().status.rpm.map_or(-1, |r| r as i32)
    }
}

struct BatteryIface {
    d: Arc<Mutex<Daemon>>,
}

impl BatteryIface {
    fn lock(&self) -> MutexGuard<'_, Daemon> {
        self.d.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[interface(name = "org.rare.Battery")]
impl BatteryIface {
    async fn set_care(&self, #[zbus(header)] hdr: Header<'_>, #[zbus(connection)] conn: &Connection, on: bool) -> fdo::Result<()> {
        authorize(conn, &hdr, BATTERY_ACTION).await?;
        self.lock().set_care(on);
        Ok(())
    }

    async fn set_limit(&self, #[zbus(header)] hdr: Header<'_>, #[zbus(connection)] conn: &Connection, percent: u32) -> fdo::Result<()> {
        authorize(conn, &hdr, BATTERY_ACTION).await?;
        let p = u8::try_from(percent).map_err(|_| fdo::Error::InvalidArgs(format!("limit {percent} % is too big")))?;
        self.lock().set_limit(p).map_err(fdo::Error::InvalidArgs)
    }

    #[zbus(property)]
    fn limit(&self) -> u32 {
        self.lock().battery_status.limit as u32
    }

    #[zbus(property)]
    fn cycles(&self) -> f64 {
        self.lock().battery_status.cycles.unwrap_or(-1.0)
    }

    #[zbus(property)]
    fn care(&self) -> bool {
        self.lock().battery_status.care
    }

    #[zbus(property)]
    fn available(&self) -> bool {
        self.lock().battery_status.available
    }

    #[zbus(property)]
    fn state(&self) -> String {
        self.lock().battery_status.state.name().to_string()
    }

    #[zbus(property)]
    fn health(&self) -> f64 {
        self.lock().battery_status.health.unwrap_or(-1.0)
    }
}

/// Owns org.rare.Fan and serves /org/rare/Fan and /org/rare/Battery; keep the connection alive
/// while running.
pub fn serve(d: Arc<Mutex<Daemon>>) -> zbus::Result<zbus::blocking::Connection> {
    zbus::blocking::connection::Builder::system()?
        .name(NAME)?
        .serve_at(PATH, FanIface { d: d.clone() })?
        .serve_at(BATTERY_PATH, BatteryIface { d })?
        .build()
}
