//! Small things Trade keeps on the device (the swap API key, followed traders, mirror sizes, recent
//! coins): what the web launcher put in localStorage. The host gives a [`Kv`] backed by its own
//! storage; [`MemKv`] serves tests and private sessions.

use std::collections::HashMap;
use std::sync::Mutex;

pub trait Kv: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
    fn remove(&self, key: &str);
}

#[derive(Default)]
pub struct MemKv(Mutex<HashMap<String, String>>);

impl MemKv {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Kv for MemKv {
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().get(key).cloned()
    }
    fn set(&self, key: &str, value: &str) {
        self.0.lock().unwrap().insert(key.to_string(), value.to_string());
    }
    fn remove(&self, key: &str) {
        self.0.lock().unwrap().remove(key);
    }
}
