//! The add friend box: a username (with or without @), a SuiNS name or a 0x address, to an address.
//! Usernames are looked up here; SuiNS names and addresses go to the wallet's recipient resolver
//! (`rare-wallet`, the same one the send pages use), which the host passes in.

use crate::api::{FriendsApi, Result};
use crate::display::{short_addr, shown};
use crate::username_rules::{parse_target, username_problem, Target};

/// The wallet's answer for a SuiNS name or an address (wallet/tx/recipient.ts `RecipientState`,
/// the states that matter once typing has stopped).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recipient {
    /// `suins` is the name typed, or the address's default name.
    Ready { address: String, suins: Option<String> },
    Invalid { msg: String },
    Missing { msg: String },
    Error { msg: String },
    /// Anything else (still typing, empty).
    Other,
}

/// Rare Wallet's recipient resolver: SuiNS names to addresses, addresses to their default name.
pub trait RecipientResolver: Send + Sync {
    fn resolve_recipient(&self, text: &str) -> Recipient;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    Ok { address: String, label: String, secondary: Option<String> },
    Err { msg: String },
}

/// Who the add box names. None when it is empty; errors only when rare.market cannot be asked.
pub fn resolve_target(api: &dyn FriendsApi, wallet: &dyn RecipientResolver, raw: &str) -> Result<Option<Resolved>> {
    let Some(t) = parse_target(raw) else { return Ok(None) };
    let text = match t {
        Target::Username { username } => {
            if let Some(problem) = username_problem(&username) {
                return Ok(Some(Resolved::Err { msg: problem }));
            }
            let Some(u) = api.lookup_username(&username)? else {
                return Ok(Some(Resolved::Err { msg: "No player has that username.".into() }));
            };
            let s = shown(Some(&u), "Someone");
            return Ok(Some(Resolved::Ok { address: u.address, label: s.primary, secondary: s.secondary }));
        }
        Target::Address { address } => address,
        Target::Suins { name } => name,
    };
    Ok(Some(match wallet.resolve_recipient(&text) {
        Recipient::Ready { address, suins } => Resolved::Ok { label: suins.unwrap_or_else(|| short_addr(&address)), address, secondary: None },
        Recipient::Invalid { msg } | Recipient::Missing { msg } | Recipient::Error { msg } => Resolved::Err { msg },
        Recipient::Other => Resolved::Err { msg: "Could not find that player.".into() },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{NoWallet, Remote};
    use rare_market::net::{Method, MockNet};
    use std::sync::Arc;

    struct Names;
    impl RecipientResolver for Names {
        fn resolve_recipient(&self, text: &str) -> Recipient {
            match text {
                "mira.sui" => Recipient::Ready { address: "0xabcdef0123".into(), suins: Some("mira.sui".into()) },
                "0xabcdef0123" => Recipient::Ready { address: "0xabcdef0123".into(), suins: None },
                _ => Recipient::Missing { msg: "No one has that name.".into() },
            }
        }
    }

    #[test]
    fn names_usernames_and_addresses_resolve() {
        let net = MockNet::new();
        net.on_call(Method::Get, "https://rare.market/v1/users/mira", 200, r#"{"address":"0x1","username":"mira","avatarUrl":null,"suins":"mira.sui"}"#);
        net.on_call(Method::Get, "https://rare.market/v1/users/ghost", 404, "");
        let api = Remote::new(Arc::new(net), Arc::new(NoWallet));
        assert_eq!(resolve_target(&api, &Names, " ").unwrap(), None);
        assert_eq!(resolve_target(&api, &Names, "@Mira").unwrap(), Some(Resolved::Ok { address: "0x1".into(), label: "mira".into(), secondary: Some("mira.sui".into()) }));
        assert_eq!(resolve_target(&api, &Names, "ghost").unwrap(), Some(Resolved::Err { msg: "No player has that username.".into() }));
        assert_eq!(resolve_target(&api, &Names, "a..b").unwrap(), Some(Resolved::Err { msg: "No two . or _ in a row.".into() }));
        assert_eq!(resolve_target(&api, &Names, "Mira.sui").unwrap(), Some(Resolved::Ok { address: "0xabcdef0123".into(), label: "mira.sui".into(), secondary: None }));
        assert_eq!(resolve_target(&api, &Names, "0xABCDEF0123").unwrap(), Some(Resolved::Ok { address: "0xabcdef0123".into(), label: "0xabcd...0123".into(), secondary: None }));
        assert_eq!(resolve_target(&api, &Names, "nope.sui").unwrap(), Some(Resolved::Err { msg: "No one has that name.".into() }));
    }
}
