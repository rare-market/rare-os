//! What the review sheet's Details and fee lines name: well-known packages (review/packages.ts
//! KNOWN, `knownPackage`, `callLabel`) and Rare's own fee wallets (review/fees.ts `splitFees`).
//! Same tables as TS; when rare-wallet lands its review module, these move there.

use crate::screens::wallet::fmt::{norm_id, norm_type, short_id};
use std::collections::BTreeMap;

/// Original (v1) package ids of well-known protocols (packages.ts, checked on mainnet 2026-10-24).
const KNOWN: &[(&str, &str)] = &[
    ("0x1", "Move stdlib"),
    ("0x2", "Sui"),
    ("0x3", "Sui system"),
    // Rare's own contracts (rare.market's review names them the same way)
    ("0x465753839605ea682f147cfe5db736c7e8718798d64cd8f0fe7bd60869d64b03", "Rare"),
    ("0x94b8e1cf51652ea35680a55cb8a7b1a16978f2ba1dc15e0c93ed7536d7659475", "Rare"),
    ("0x9252bf93053dcd0cd5ad0562303e66d6a5526d6022453b88c00dc463a012a7bf", "Rare"),
    ("0xda09fa9db4a54d4b0c337fb7ee41311f4679a7f6049e58372a4951968a32f73c", "Rare"),
    ("0xdee9", "DeepBook"),
    ("0x2c8d603bc51326b8c13cef9dd07031a408a48dddb541963357661df5d3204809", "DeepBook"),
    ("0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb", "Cetus"),
    ("0x91bfbc386a41afcfd9b2533058d7e915a1d3829089cc268ff4333d54d6339ca1", "Turbos"),
    ("0x3492c874c1e3b3e2984e8c41b589e642d4d0a5d6459e5a9cfc2d52fd7c89c267", "Bluefin"),
    ("0xefe170ec0be4d762196bedecd7a065816576198a6527c99282a2551aaa7da38c", "Aftermath"),
    ("0xa0eba10b173538c8fecca1dff298e488402cc9ff374f8a12ca7758eebe830b66", "Kriya"),
    ("0xba153169476e8c3114962261d1edc70de5ad9781b83cc617ecc8c1923191cae0", "FlowX"),
    ("0xf95b06141ed4a174f239417323bde3f209b972f5930d8521ea38a52aff3a6ddf", "Suilend"),
    ("0xefe8b36d5b2e43728cc323298626b83177803521d195cfb11e15b910e892fddf", "Scallop"),
    ("0xd899cf7d2b5db716bd2cf55599fb0d5ee38a3061e7b6bb6eebf73fa5bc4c81ca", "NAVI"),
    ("0xd22b24490e0bae52676651b4f56660a5ff8022a2576e0089f79b3c88d44e08f0", "SuiNS"),
    ("0x434b5bd8f6a7b05fede0ff46c6e511d71ea326ed38056e3bcd681d2d7c2a7879", "Kiosk"),
    ("0xdfb4f1d4e43e0c3ad834dcd369f0d39005c872e118c9dc1c5da9765bb93ee5f3", "Kiosk"),
    ("0xb42dbb7413b79394e1a0175af6ae22b69a5c7cc5df259cd78072b6818217c027", "Tradeport"),
    ("0xd85a131b82ef9f4aa5eb99a0ea758a3f209a8771eba7e9402e8181273734bf9a", "Tradeport"),
    ("0xec175e537be9e48f75fa6929291de6454d2502f1091feb22c0d26a22821bbf28", "Tradeport"),
    ("0xafc035fc8e321da96a2cb1bb88cec64ef6bf7c83483653c259184312b3c0bb96", "Tradeport"),
    ("0xf938f27ce9460838643ac04fac05254f73329caf59643b7843c883bb4fa7402f", "Tradeport"),
    ("0x53134eb544c5a0b5085e99efaf7eab13b28ad123de35d61f941f8c8c40b72033", "Tradeport"),
    ("0xe2680c552bd3a5540c2f2986e92927ef2ebb6308e41ae395a1cec24e51d2d951", "Tradeport"),
    ("0xff2251ea99230ed1cbe3a347a209352711c6723fcdcd9286e16636e65bb55cab", "Tradeport"),
    ("0x81e16ff388eacc2908f4e8248f54d140eb6d7834ff01dc7a86042a3b701b68f6", "Tradeport"),
    ("0x06cefebb11191abd347654200a6fd8ae47fa97b54506e4e680db64dae54d0b12", "Tradeport"),
    ("0xe50387176587c707c428c537b7b8fb530c3eaeadd16a45b1d975369f372dbc0a", "Drip"),
    ("0x7dcbc405496f4f6ecb70921b571ea9bfca09db7004efddd3dad33a524e7489e4", "Drip"),
    ("0x0cb4bcc0560340eb1a1b929cabe56b33fc6449820ec8c1980d69bb98b649b802", "Kiosk"),
    ("0x34cc6762780f4f6f153c924c0680cfe2a1fb4601e7d33cc28a92297b62de1e0e", "Kiosk"),
];

fn by_id(n: &str) -> Option<&'static str> {
    KNOWN.iter().find(|(k, _)| norm_id(k) == n).map(|(_, v)| *v)
}

/// `knownPackage`: the protocol name for a package (or its original id), else None.
pub fn known_package(id: &str, originals: &BTreeMap<String, String>) -> Option<&'static str> {
    let n = norm_id(id);
    let orig = originals.get(&n).or_else(|| originals.get(id));
    by_id(&n).or_else(|| orig.and_then(|o| by_id(&norm_id(o))))
}

#[derive(Clone, Debug, PartialEq)]
pub struct MoveCallRef {
    pub package: String,
    pub module: String,
    pub function: String,
}

/// `callLabel`: `module::function` over the protocol name or the short package id.
pub fn call_label(c: &MoveCallRef, originals: &BTreeMap<String, String>) -> (String, String) {
    let sub = known_package(&c.package, originals).map(str::to_string).unwrap_or_else(|| short_id(&c.package));
    (format!("{}::{}", c.module, c.function), sub)
}

/// Rare's own fee wallets: transfers to them are a named fee line, not a "Goes to" recipient.
/// the Trade API's router fee wallet takes the 0.2% swap fee on Rare's Trade tab.
const KNOWN_FEE_RECIPIENTS: &[(&str, &str)] = &[("0x1dd4889f1ba614d0b44bd0fda31dedbb734d4edf8f25254f7d6b69671903ff06", "Rare fee")];

pub fn known_fee(address: &str) -> Option<&'static str> {
    let n = norm_id(address);
    KNOWN_FEE_RECIPIENTS.iter().find(|(a, _)| norm_id(a) == n).map(|(_, l)| *l)
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoinFlow {
    pub coin_type: String,
    /// absolute base units
    pub amount: i128,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OtherReceive {
    pub address: String,
    pub coin_type: String,
    pub amount: i128,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FeeLine {
    pub address: String,
    pub label: String,
    pub coin_type: String,
    pub amount: i128,
    /// the fee as a share of what you send of that coin, e.g. "0.2%"
    pub pct: Option<String>,
}

/// `splitFees`: coin transfers to other addresses split into Rare's fee lines and everything else.
pub fn split_fees(others_receive: &[OtherReceive], send: &[CoinFlow]) -> (Vec<FeeLine>, Vec<OtherReceive>) {
    let mut fees = Vec::new();
    let mut others = Vec::new();
    for o in others_receive {
        let Some(label) = known_fee(&o.address) else {
            others.push(o.clone());
            continue;
        };
        let sent = send.iter().find(|s| norm_type(&s.coin_type) == norm_type(&o.coin_type)).map(|s| s.amount).unwrap_or(0);
        let pct = (sent > 0).then(|| {
            let bps = (o.amount * 10_000 + sent / 2) / sent;
            let s = if bps % 10 == 0 { format!("{}.{}", bps / 100, (bps / 10) % 10) } else { format!("{}.{:02}", bps / 100, bps % 100) };
            format!("{}%", s.strip_suffix(".0").unwrap_or(&s))
        });
        fees.push(FeeLine { address: o.address.clone(), label: label.into(), coin_type: o.coin_type.clone(), amount: o.amount, pct });
    }
    (fees, others)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::fmt::sui_type;

    const FEE: &str = "0x1dd4889f1ba614d0b44bd0fda31dedbb734d4edf8f25254f7d6b69671903ff06";
    const BOB: &str = "0x00000000000000000000000000000000000000000000000000000000000000b0";

    // review/fees.test.ts
    #[test]
    fn fee_lines() {
        let send = [CoinFlow { coin_type: sui_type(), amount: 1_000_000_000 }];
        let (fees, others) = split_fees(
            &[OtherReceive { address: FEE.into(), coin_type: "0x2::sui::SUI".into(), amount: 2_000_000 }, OtherReceive { address: BOB.into(), coin_type: sui_type(), amount: 998_000_000 }],
            &send,
        );
        assert_eq!(fees.len(), 1);
        assert_eq!(fees[0].label, "Rare fee");
        assert_eq!(fees[0].pct.as_deref(), Some("0.2%"));
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].address, BOB);
        let (fees, _) = split_fees(&[OtherReceive { address: FEE.into(), coin_type: sui_type(), amount: 10_000_000 }], &send);
        assert_eq!(fees[0].pct.as_deref(), Some("1%"));
        let (fees, _) = split_fees(&[OtherReceive { address: FEE.into(), coin_type: sui_type(), amount: 2_500_000 }], &send);
        assert_eq!(fees[0].pct.as_deref(), Some("0.25%"));
        let (fees, _) = split_fees(&[OtherReceive { address: FEE.into(), coin_type: sui_type(), amount: 1 }], &[]);
        assert_eq!(fees[0].pct, None);
    }

    #[test]
    fn calls() {
        let o = BTreeMap::new();
        let c = MoveCallRef { package: "0x2".into(), module: "coin".into(), function: "split".into() };
        assert_eq!(call_label(&c, &o), ("coin::split".to_string(), "Sui".to_string()));
        let c = MoveCallRef { package: "0xabcdef0123456789".into(), module: "pool".into(), function: "swap".into() };
        assert_eq!(call_label(&c, &o).1, "0x0000…6789");
        let mut o = BTreeMap::new();
        o.insert(norm_id("0xabcdef0123456789"), "0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb".to_string());
        assert_eq!(call_label(&c, &o).1, "Cetus");
    }
}
