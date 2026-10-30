//! The unsigned transactions Trade hands to Rare Wallet. The Trade API answers with ready-made
//! transactions (a router's transaction kind, or the perps API's transaction JSON), so nothing here
//! builds a PTB of its own: these carry the API's answer, with the sender, to `rare-sui` (gas,
//! object versions, BCS) and `rare-wallet` (review, sign, submit).

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum UnsignedTx {
    /// A router's transaction kind (BCS `TransactionKind`, base64) for `sender`: what
    /// `Transaction.fromKind(kind)` then `setSender(sender)` made in the web launcher. When
    /// `gas_from_spent_sui` is set the builder runs [`crate::chain::gas_from_spent_sui`] on the
    /// transaction's JSON before resolving gas (a wallet holding one SUI coin has nothing else to pay
    /// gas with); a sponsored swap never touches the gas coin, so it is left off there.
    Kind { kind: String, sender: String, gas_from_spent_sui: bool },
    /// Transaction JSON v2 (`Transaction.toJSON()`), sender set.
    Json(Value),
    /// Transaction bytes (base64) as `Transaction.from(bytes)` takes them; the builder sets `sender`
    /// when the bytes carry none.
    Bytes { b64: String, sender: String },
}

impl UnsignedTx {
    /// The sender the transaction is for.
    pub fn sender(&self) -> Option<&str> {
        match self {
            UnsignedTx::Kind { sender, .. } | UnsignedTx::Bytes { sender, .. } => Some(sender),
            UnsignedTx::Json(v) => v.get("sender").and_then(Value::as_str),
        }
    }

    /// `Transaction.from(text)` then `setSenderIfNotSet(sender)`: JSON when the text is JSON, else
    /// base64 bytes.
    pub fn from_api(text: &str, sender: &str) -> UnsignedTx {
        match serde_json::from_str::<Value>(text) {
            Ok(mut v @ Value::Object(_)) => {
                if v.get("sender").map_or(true, Value::is_null) {
                    v["sender"] = Value::String(rare_market::ptb::norm_addr(sender));
                }
                UnsignedTx::Json(v)
            }
            _ => UnsignedTx::Bytes { b64: text.to_string(), sender: sender.to_string() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn from_api_sets_a_missing_sender_only() {
        let me = format!("0x{}", "b2".repeat(32));
        let t = UnsignedTx::from_api(&json!({"version": 2, "sender": null, "commands": []}).to_string(), &me);
        assert_eq!(t.sender(), Some(me.as_str()));
        let t = UnsignedTx::from_api(&json!({"version": 2, "sender": "0x1", "commands": []}).to_string(), &me);
        assert_eq!(t.sender(), Some("0x1"));
        assert_eq!(UnsignedTx::from_api("AAEC", &me), UnsignedTx::Bytes { b64: "AAEC".into(), sender: me.clone() });
    }
}
