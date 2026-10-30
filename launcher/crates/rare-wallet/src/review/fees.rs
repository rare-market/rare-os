//! Fee recipients Rare itself uses (ports `review/fees.ts`). Transfers to them are shown as a
//! named fee line ("Rare fee 0.2%") instead of "Sends coins to 0x..., not you" plus a
//! first-time-recipient warning, which reads like a scam alert on every swap. Unknown recipients
//! keep the full warning.
//!
//! - the swap router's fee wallet: the 0.2% swap fee on Rare's Trade tab (the wallet the Trade
//!   API's swap builds pay).
//!
//! rare.market's own marketplace fees go to its Config treasury through the contract, not as a
//! plain coin transfer, so there is nothing to list for it until that changes.

use serde::{Deserialize, Serialize};

use super::analyze::{big_str, norm_type, CoinFlow, OtherReceive};
use super::packages::norm_id;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct KnownFeeRecipient {
    pub label: &'static str,
}

/// (address, label), addresses normalized.
pub const KNOWN_FEE_RECIPIENTS: &[(&str, KnownFeeRecipient)] = &[("0x1dd4889f1ba614d0b44bd0fda31dedbb734d4edf8f25254f7d6b69671903ff06", KnownFeeRecipient { label: "Rare fee" })];

/// Rare's own fee wallet at this address (any spelling), else None.
pub fn known_fee(address: &str) -> Option<KnownFeeRecipient> {
    let a = norm_id(address);
    KNOWN_FEE_RECIPIENTS.iter().find(|(k, _)| norm_id(k) == a).map(|(_, v)| *v)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeLine {
    pub address: String,
    pub label: String,
    pub coin_type: String,
    #[serde(with = "big_str")]
    pub amount: i128,
    /// The fee as a share of what you send of that coin, when known, e.g. "0.2%".
    pub pct: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitFees {
    pub fees: Vec<FeeLine>,
    pub others: Vec<OtherReceive>,
}

/// Splits coin transfers to other addresses into Rare's own fee lines and everything else.
pub fn split_fees(others_receive: &[OtherReceive], send: &[CoinFlow]) -> SplitFees {
    let mut out = SplitFees::default();
    for o in others_receive {
        let Some(k) = known_fee(&o.address) else {
            out.others.push(o.clone());
            continue;
        };
        let ct = norm_type(&o.coin_type);
        let sent = send.iter().find(|s| norm_type(&s.coin_type) == ct).map_or(0, |s| s.amount);
        // the fee is part of what leaves the wallet: share = fee / (amount sent)
        let pct = (sent > 0).then(|| {
            let bps = (o.amount * 10_000 + sent / 2) / sent; // basis points, rounded
            if bps % 10 == 0 {
                let s = format!("{}.{}", bps / 100, (bps % 100) / 10);
                format!("{}%", s.strip_suffix(".0").unwrap_or(&s))
            } else {
                format!("{}.{:02}%", bps / 100, bps % 100)
            }
        });
        out.fees.push(FeeLine { address: o.address.clone(), label: k.label.to_string(), coin_type: o.coin_type.clone(), amount: o.amount, pct });
    }
    out
}
