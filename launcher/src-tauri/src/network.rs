//! Thin wrapper: the code lives in the rare-system crate (launcher/crates/rare-system).

use crate::config::Config;

pub fn is_testnet(c: &Config) -> bool {
    rare_system::network::is_testnet(c)
}

pub fn market_url(c: &Config, path: &str) -> String {
    rare_system::network::market_url(c, path)
}

pub fn sponsor_url(c: &Config, path: &str) -> String {
    rare_system::network::sponsor_url(c, path)
}
