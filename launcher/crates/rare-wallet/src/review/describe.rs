//! Display rows for the review sheet (ports `review/describe.ts`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::analyze::{big_str, norm_type, Insights, ObjectFlow, ObjectFlowKind, SUI_TYPE};
use super::packages::{object_label, short_id};
use super::types::DisplayInfo;
use crate::amount::format_amount;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinMeta {
    pub symbol: String,
    pub decimals: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Coin metadata by coin type (any spelling of the address), or a lookup function.
#[derive(Clone, Copy, Default)]
pub enum CoinMetaLookup<'a> {
    #[default]
    None,
    Map(&'a BTreeMap<String, CoinMeta>),
    Fn(&'a dyn Fn(&str) -> Option<CoinMeta>),
}

impl<'a> From<&'a BTreeMap<String, CoinMeta>> for CoinMetaLookup<'a> {
    fn from(m: &'a BTreeMap<String, CoinMeta>) -> Self {
        CoinMetaLookup::Map(m)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RowKind {
    Send,
    Receive,
    Fee,
}

pub const LABEL_SEND: &str = "You send";
pub const LABEL_RECEIVE: &str = "You receive";
pub const LABEL_FEE: &str = "Network fee";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRow {
    pub kind: RowKind,
    /// "You send" / "You receive" / "Network fee".
    pub label: String,
    /// "1.5 SUI", "Popkins #8475", "0.0019 SUI".
    pub text: String,
    /// Small second line: where an object goes, "Paid by someone else".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coin_type: Option<String>,
    /// Absolute base units for coin rows; the net fee (negative = refund) for the fee row.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "big_str::opt")]
    pub amount: Option<i128>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

impl ReviewRow {
    fn new(kind: RowKind, text: String) -> Self {
        let label = match kind {
            RowKind::Send => LABEL_SEND,
            RowKind::Receive => LABEL_RECEIVE,
            RowKind::Fee => LABEL_FEE,
        };
        ReviewRow { kind, label: label.into(), text, sub: None, coin_type: None, amount: None, icon_url: None, object_id: None, object_type: None, image: None }
    }
}

/// Extras for [`describe_review`].
#[derive(Clone, Debug, Default)]
pub struct DescribeExtras<'a> {
    pub display: Option<&'a BTreeMap<String, DisplayInfo>>,
    pub originals: Option<&'a BTreeMap<String, String>>,
    /// The gas owner is Rare's fee sponsor (rare.market).
    pub covered_by_rare: bool,
    /// Who that sponsorship is paid by: Rare, or a game's gas budget ("Network fee: Paid by <game>").
    pub paid_by: Option<String>,
}

/// Coin metadata for a coin type: exact hit, else SUI's, else the type name and 9 decimals.
pub fn lookup_coin(meta: CoinMetaLookup<'_>, coin_type: &str) -> CoinMeta {
    let t = norm_type(coin_type);
    let hit = match meta {
        CoinMetaLookup::None => None,
        CoinMetaLookup::Fn(f) => f(&t).or_else(|| f(coin_type)),
        CoinMetaLookup::Map(m) => m.get(&t).or_else(|| m.get(coin_type)).or_else(|| m.get(&short_outer(&t))).cloned(),
    };
    if let Some(h) = hit {
        return h;
    }
    if t == SUI_TYPE {
        return CoinMeta { symbol: "SUI".into(), decimals: 9, icon_url: None };
    }
    CoinMeta { symbol: t.rsplit("::").next().unwrap_or("?").to_string(), decimals: 9, icon_url: None }
}

/// `t.replace(/^0x0*/, '0x')`
fn short_outer(t: &str) -> String {
    match t.strip_prefix("0x") {
        Some(r) => format!("0x{}", r.trim_start_matches('0')),
        None => t.to_string(),
    }
}

fn object_row(kind: RowKind, o: &ObjectFlow, extras: &DescribeExtras) -> ReviewRow {
    let d = extras.display.and_then(|m| m.get(&o.object_id));
    let title = d.and_then(|d| d.name.clone()).unwrap_or_else(|| object_label(o.object_type.as_deref(), extras.originals).title);
    let sub = match o.kind {
        ObjectFlowKind::Sent if o.to.is_some() => Some(format!("To {}", short_id(o.to.as_deref().unwrap_or_default()))),
        ObjectFlowKind::IntoObject => Some("Into another object".into()),
        ObjectFlowKind::Wrapped => Some("Held by the app".into()),
        ObjectFlowKind::Deleted => Some("Destroyed".into()),
        ObjectFlowKind::Shared => Some("Made public".into()),
        ObjectFlowKind::Frozen => Some("Frozen".into()),
        _ if o.created == Some(true) => Some("New".into()),
        _ => None,
    };
    ReviewRow { sub, object_id: Some(o.object_id.clone()), object_type: o.object_type.clone(), image: d.and_then(|d| d.image.clone()), ..ReviewRow::new(kind, title) }
}

/// Display rows for the review sheet: what leaves, what arrives, and the network fee.
pub fn describe_review(ins: &Insights, coin_meta: CoinMetaLookup<'_>, extras: &DescribeExtras) -> Vec<ReviewRow> {
    let coin = |kind: RowKind, coin_type: &str, amount: i128| {
        let m = lookup_coin(coin_meta, coin_type);
        ReviewRow {
            coin_type: Some(coin_type.to_string()),
            amount: Some(amount),
            icon_url: m.icon_url.clone(),
            ..ReviewRow::new(kind, format!("{} {}", format_amount(amount, m.decimals), m.symbol))
        }
    };
    let mut rows: Vec<ReviewRow> = vec![];
    rows.extend(ins.send.iter().map(|f| coin(RowKind::Send, &f.coin_type, f.amount)));
    rows.extend(ins.objects_out.iter().map(|o| object_row(RowKind::Send, o, extras)));
    rows.extend(ins.receive.iter().map(|f| coin(RowKind::Receive, &f.coin_type, f.amount)));
    rows.extend(ins.objects_in.iter().map(|o| object_row(RowKind::Receive, o, extras)));
    let sui = lookup_coin(coin_meta, SUI_TYPE);
    let net = ins.fee.net;
    let fee = |text: String, sub: Option<&str>, amount: i128| ReviewRow { sub: sub.map(str::to_string), coin_type: Some(SUI_TYPE.into()), amount: Some(amount), ..ReviewRow::new(RowKind::Fee, text) };
    rows.push(if !ins.fee_paid_by_signer && extras.covered_by_rare {
        fee(format!("Paid by {}", extras.paid_by.as_deref().filter(|p| !p.is_empty()).unwrap_or("Rare")), None, 0)
    } else if !ins.fee_paid_by_signer {
        fee(format!("0 {}", sui.symbol), Some("Paid by someone else"), 0)
    } else if net < 0 {
        fee(format!("{} {} back", format_amount(-net, sui.decimals), sui.symbol), Some("Storage refund is larger than the fee"), net)
    } else {
        fee(format!("{} {}", format_amount(net, sui.decimals), sui.symbol), None, net)
    });
    rows
}
