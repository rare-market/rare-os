//! Rare usernames (server: market api/src/usernames). Free, unique, picked by the player.
//! Set with the friends device key when it is linked, else with one wallet signature (the review
//! sheet shows the message). Availability is checked live while typing.

use crate::api::{send, send_empty, Error, Result, SignPrompt, WalletSigner};
use crate::sign::{iso_time, DeviceKey};
use crate::types::Person;
use crate::username_rules::normalize_username;
use rare_market::net::{Method, Net};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Unavailable {
    Invalid,
    Reserved,
    Profane,
    Taken,
    Held,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Availability {
    pub username: String,
    pub available: bool,
    #[serde(default)]
    pub reason: Option<Unavailable>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yours: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Mine {
    pub username: String,
    pub claimed_at: i64,
    pub changed_at: i64,
    #[serde(default)]
    pub next_change_at: Option<i64>,
}

/// The text a wallet signs to set its username (server claim.ts claimText; it rebuilds it to compare).
pub fn claim_text(address: &str, username: &str, issued: &str) -> String {
    [
        "Rare username",
        "",
        &format!("Set the Rare username @{username} for this wallet."),
        "",
        &format!("Address: {address}"),
        &format!("Username: {username}"),
        &format!("Issued: {issued}"),
        "",
        "This does not move funds, send a transaction or cost gas.",
    ]
    .join("\n")
}

/// JavaScript's encodeURIComponent: everything but `A-Z a-z 0-9 - _ . ! ~ * ' ( )` is escaped.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ---------- calls (the Remote API uses these) ----------

pub fn check(net: &dyn Net, raw: &str, address: Option<&str>, now: i64) -> Result<Availability> {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    q.append_pair("u", &normalize_username(raw));
    if let Some(a) = address.filter(|a| !a.is_empty()) {
        q.append_pair("address", a);
    }
    send_empty(net, None, Method::Get, &format!("/v1/usernames/available?{}", q.finish()), now)
}

/// Sets (claims or changes) the username for `address`. `key` is the linked device key, if any.
pub fn set(net: &dyn Net, wallet: &dyn WalletSigner, address: &str, raw: &str, key: Option<&DeviceKey>, now: i64) -> Result<Mine> {
    let username = normalize_username(raw);
    if let Some(k) = key {
        return send(net, Some(k), Method::Put, "/v1/usernames/me", Some(&serde_json::json!({ "username": username })), now);
    }
    let message = claim_text(address, &username, &iso_time(now));
    let prompt = SignPrompt { account: address.to_string(), origin: "Rare username".into(), title: "Pick your username".into() };
    let signature = wallet.sign_personal_message(message.as_bytes(), &prompt)?;
    send(net, None, Method::Post, "/v1/usernames/claim", Some(&serde_json::json!({ "address": address, "message": message, "signature": signature })), now)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct User {
    address: String,
    username: String,
    #[serde(default)]
    avatar_url: Option<String>,
    #[serde(default)]
    suins: Option<String>,
}

/// Who has this username (None when nobody).
pub fn lookup(net: &dyn Net, raw: &str, now: i64) -> Result<Option<Person>> {
    let u = normalize_username(raw);
    match send_empty::<User>(net, None, Method::Get, &format!("/v1/users/{}", encode_uri_component(&u)), now) {
        Ok(r) => Ok(Some(Person { address: r.address, name: Some(r.username.clone()), username: Some(r.username), suins: r.suins, avatar_url: r.avatar_url })),
        Err(Error::Api(e)) if e.status == 404 => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_market::net::MockNet;

    #[test]
    fn checks_lookups_and_claims() {
        let net = MockNet::new();
        net.on_call(Method::Get, "https://rare.market/v1/usernames/available?u=mira_k&address=0xab", 200, r#"{"username":"mira_k","available":false,"reason":"taken","message":"Someone already has that name."}"#);
        net.on_call(Method::Get, "https://rare.market/v1/users/nobody", 404, r#"{"error":{"code":"not_found","message":"No such user"}}"#);
        net.on_call(Method::Get, "https://rare.market/v1/users/mira", 200, r#"{"address":"0x1","username":"mira","avatarUrl":null,"suins":"mira.sui"}"#);
        let a = check(&net, " @Mira_K", Some("0xab"), 0).unwrap();
        assert_eq!(a.reason, Some(Unavailable::Taken));
        assert_eq!(lookup(&net, "nobody", 0).unwrap(), None);
        let p = lookup(&net, "@Mira", 0).unwrap().unwrap();
        assert_eq!((p.address.as_str(), p.suins.as_deref()), ("0x1", Some("mira.sui")));

        struct W;
        impl WalletSigner for W {
            fn sign_personal_message(&self, m: &[u8], p: &SignPrompt) -> Result<String> {
                assert_eq!(p.title, "Pick your username");
                Ok(format!("sig:{}", m.len()))
            }
        }
        net.on_call(Method::Post, "https://rare.market/v1/usernames/claim", 200, r#"{"username":"mira","claimedAt":1,"changedAt":1,"nextChangeAt":null}"#);
        let m = set(&net, &W, "0xab", "Mira", None, 1_792_972_800_000).unwrap();
        assert_eq!(m.username, "mira");
        let body: serde_json::Value = serde_json::from_str(net.calls().last().unwrap().body.as_deref().unwrap()).unwrap();
        assert_eq!(body["message"], claim_text("0xab", "mira", "2026-10-26T00:00:00.000Z"));
    }

    #[test]
    fn escapes_like_encode_uri_component() {
        assert_eq!(encode_uri_component("a.b_c-d"), "a.b_c-d");
        assert_eq!(encode_uri_component("a b/c?é"), "a%20b%2Fc%3F%C3%A9");
    }
}
