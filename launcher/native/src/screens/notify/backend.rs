//! What the notify screens need from the outside, as traits. rare-notify (NotifyStream, api,
//! Notifier) implements them later; the fixtures here stand in until then. Names follow the crate:
//! `NewAlert` is `rare_notify::api::NewAlert`, `StreamEvent` mirrors `rare_notify::StreamEvent`.

use super::data::store::Status;
use super::data::types::{AlertDir, Notice, PriceAlert};

#[derive(Clone, Debug, PartialEq)]
pub struct NewAlert {
    pub coin_type: String,
    pub symbol: String,
    pub dir: AlertDir,
    pub target: f64,
    pub addr: Option<String>,
}

/// api.ts alerts: list, add and delete price alerts for this device.
pub trait AlertsApi {
    fn list(&mut self) -> Result<Vec<PriceAlert>, String>;
    fn add(&mut self, a: &NewAlert) -> Result<String, String>;
    fn remove(&mut self, id: &str) -> Result<(), String>;
}

/// The notify stream as the UI sees it (stream.ts): the host drains these on its UI thread.
#[derive(Clone, Debug, PartialEq)]
pub enum StreamEvent {
    Status(Status),
    Notice(Box<Notice>),
    /// `recent` answered, newest first.
    Recent(Vec<Notice>),
}

pub trait NoticeStream {
    fn drain(&mut self) -> Vec<StreamEvent>;
}

/// Battery readings (store.device.battery), for the BatteryNotice watcher.
pub trait BatterySource {
    fn reading(&mut self) -> Option<super::data::battery::BatteryReading>;
}

/// Alerts held in memory: the web mock's one starting alert.
pub struct MemAlerts {
    pub list: Vec<PriceAlert>,
    pub fail: bool,
    seq: u32,
}

impl MemAlerts {
    pub fn new(list: Vec<PriceAlert>) -> MemAlerts {
        MemAlerts { list, fail: false, seq: 0 }
    }
}

impl AlertsApi for MemAlerts {
    fn list(&mut self) -> Result<Vec<PriceAlert>, String> {
        if self.fail {
            return Err("rare.market 503".into());
        }
        Ok(self.list.clone())
    }
    fn add(&mut self, a: &NewAlert) -> Result<String, String> {
        if self.fail {
            return Err("rare.market 503".into());
        }
        self.seq += 1;
        let id = format!("a{}", self.seq);
        self.list.push(PriceAlert { id: id.clone(), coin_type: a.coin_type.clone(), symbol: a.symbol.clone(), dir: a.dir, target: a.target, created_at: super::data::types::now_ms(), fired_at: None, active: true });
        Ok(id)
    }
    fn remove(&mut self, id: &str) -> Result<(), String> {
        self.list.retain(|x| x.id != id);
        Ok(())
    }
}

/// alerts.ts as plain state: the list (None until loaded) and the last error.
#[derive(Default)]
pub struct AlertsState {
    pub list: Option<Vec<PriceAlert>>,
    pub error: Option<String>,
}

impl AlertsState {
    pub fn load(&mut self, api: &mut dyn AlertsApi) {
        match api.list() {
            Ok(l) => {
                self.list = Some(l);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
    pub fn add(&mut self, api: &mut dyn AlertsApi, a: &NewAlert) -> Result<(), String> {
        api.add(a)?;
        self.load(api);
        Ok(())
    }
    /// Optimistic: the row goes first, then the server, then the list is read again.
    pub fn remove(&mut self, api: &mut dyn AlertsApi, id: &str) {
        if let Some(l) = self.list.as_mut() {
            l.retain(|x| x.id != id);
        }
        let _ = api.remove(id);
        self.load(api);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(id: &str, active: bool) -> PriceAlert {
        PriceAlert { id: id.into(), coin_type: "c".into(), symbol: "SUI".into(), dir: AlertDir::Above, target: 1.1, created_at: 1, fired_at: None, active }
    }

    #[test]
    fn add_then_the_list_is_read_again() {
        let mut api = MemAlerts::new(vec![alert("a0", true)]);
        let mut st = AlertsState::default();
        st.load(&mut api);
        assert_eq!(st.list.as_ref().unwrap().len(), 1);
        st.add(&mut api, &NewAlert { coin_type: "c".into(), symbol: "SUI".into(), dir: AlertDir::Below, target: 0.9, addr: None }).unwrap();
        assert_eq!(st.list.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn a_failed_load_keeps_the_error_and_no_list() {
        let mut api = MemAlerts::new(vec![]);
        api.fail = true;
        let mut st = AlertsState::default();
        st.load(&mut api);
        assert!(st.list.is_none());
        assert!(st.error.is_some());
    }

    #[test]
    fn remove_drops_the_row_even_when_the_server_is_slow() {
        let mut api = MemAlerts::new(vec![alert("a0", true), alert("a1", false)]);
        let mut st = AlertsState::default();
        st.load(&mut api);
        st.remove(&mut api, "a0");
        assert_eq!(st.list.as_ref().unwrap().iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["a1"]);
    }
}
