//! Price alerts: device scoped on rare.market (POST /v1/notify/alerts with this launcher's device
//! id). Set from a coin's page in Trade, listed and deleted in Settings > Notifications.
//!
//! The network methods block; call them off the UI thread. The stream watches [`Alerts::has_active`]:
//! a device with alerts but no accounts still connects for the alerts themselves.

use rare_market::net::Net;

use crate::api::{self, NewAlert};
use crate::types::PriceAlert;
use crate::Result;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Alerts {
    /// None until the first load answers.
    pub list: Option<Vec<PriceAlert>>,
    pub loading: bool,
    pub error: Option<String>,
}

impl Alerts {
    pub fn has_active(&self) -> bool {
        self.list.as_ref().is_some_and(|l| l.iter().any(|a| a.active))
    }

    /// A load started.
    pub fn begin_load(&mut self) {
        self.loading = true;
    }

    /// A load answered. A failed load keeps the old list.
    pub fn finish_load(&mut self, r: Result<Vec<PriceAlert>>) {
        match r {
            Ok(list) => {
                self.list = Some(list);
                self.error = None;
            }
            Err(e) => self.error = Some(if e.0.is_empty() { "Could not load price alerts".into() } else { e.0 }),
        }
        self.loading = false;
    }

    /// Takes an alert off the list at once, before the server agrees.
    pub fn remove_local(&mut self, id: &str) {
        self.list = Some(self.list.take().unwrap_or_default().into_iter().filter(|a| a.id != id).collect());
    }

    pub fn load(&mut self, net: &dyn Net, device: &str) {
        self.begin_load();
        self.finish_load(api::list_alerts(net, device));
    }

    pub fn add(&mut self, net: &dyn Net, device: &str, a: &NewAlert) -> Result<()> {
        api::add_alert(net, device, a)?;
        self.load(net, device);
        Ok(())
    }

    /// Removes an alert; the list is reloaded whether the delete worked or not.
    pub fn remove(&mut self, net: &dyn Net, device: &str, id: &str) -> Result<()> {
        self.remove_local(id);
        let r = api::delete_alert(net, id, device);
        self.load(net, device);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AlertDir;
    use rare_market::net::{Method, MockNet};

    #[test]
    fn loads_adds_and_removes() {
        let net = MockNet::new();
        let alerts = "https://rare.market/v1/notify/alerts";
        net.on_call(Method::Get, alerts, 200, r#"{"data":[{"id":"a1","coinType":"0x2::sui::SUI","symbol":"SUI","dir":"above","target":1.1,"createdAt":1,"firedAt":null,"active":true}]}"#);
        net.on_call(Method::Get, alerts, 200, r#"{"data":[]}"#);
        net.on_call(Method::Get, alerts, 500, "");
        net.on_call(Method::Post, alerts, 200, r#"{"id":"a2"}"#);
        net.on_call(Method::Delete, alerts, 404, "");
        let mut a = Alerts::default();
        assert!(!a.has_active());
        a.load(&net, "dev");
        assert!(a.has_active());
        assert!(!a.loading);
        let r = a.remove(&net, "dev", "a1");
        assert_eq!(r.unwrap_err().0, "rare.market 404");
        assert_eq!(a.list, Some(vec![]));
        a.add(&net, "dev", &NewAlert { coin_type: "c".into(), symbol: "C".into(), dir: AlertDir::Above, target: 2.0, addr: None }).unwrap();
        assert_eq!(a.error.as_deref(), Some("rare.market 500"));
        assert_eq!(a.list, Some(vec![]));
    }
}
