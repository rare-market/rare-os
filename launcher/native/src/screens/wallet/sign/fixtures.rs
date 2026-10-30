//! Review sheet fixtures: the requests and reviews the web sheet is shot with (dev hook
//! `__rareWalletDev`: a 0.1 SUI send, the Rare Arcade sign-in message, device waiting panels) plus
//! the states the TS review tests build (a failing call, a swap with Rare's fee, a sponsored game
//! transaction, an NFT buy, a lookalike recipient that needs the extra tick). Golden images and
//! the parity tests render these.

use super::packages::{CoinFlow, MoveCallRef, OtherReceive};
use super::review::*;
use super::site::Risk;
use crate::screens::wallet::fmt::{norm_type, sui_type};
use std::collections::BTreeMap;

pub const ME: &str = "0x7d20dcdb2bca4f508ea9613994683eb4e76e9c4ed371169677c1be02aaf0b58e";
pub const OTHER: &str = "0xabababababababababababababababababababababababababababababababab";
pub const RARE_FEE: &str = "0x1dd4889f1ba614d0b44bd0fda31dedbb734d4edf8f25254f7d6b69671903ff06";
pub const USDC: &str = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";

pub fn accounts() -> Vec<Account> {
    vec![
        Account::new(ME, "Main", "passkey"),
        Account::new("0x5ab1e8f0a9c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a1b0c9d8e7f6a5b4c3d2e1", "Ledger", "ledger"),
    ]
}

pub fn wallet_origin() -> Origin {
    Origin { name: "Rare Wallet".into(), url: None, icon: None, kind: OriginKind::Wallet }
}

pub fn arcade_origin() -> Origin {
    Origin { name: "Rare Arcade".into(), url: Some("https://arcade.rare.market".into()), icon: Some("https://arcade.rare.market/icon.png".into()), kind: OriginKind::Game }
}

pub fn send_request() -> SignRequest {
    SignRequest { id: 1, kind: ReqKind::Transaction, account: ME.into(), origin: wallet_origin(), execute: true, dry_run: false, title: Some("Send 0.1 SUI".into()), fee_note: None, message: None }
}

pub fn message_request() -> SignRequest {
    SignRequest {
        id: 2,
        kind: ReqKind::Message,
        account: ME.into(),
        origin: arcade_origin(),
        execute: false,
        dry_run: false,
        title: None,
        fee_note: None,
        message: Some(b"Sign in to Rare Arcade\nNonce: 7f3a91c2".to_vec()),
    }
}

pub fn sui_coin(price: Option<f64>) -> ReviewCoin {
    ReviewCoin { symbol: "SUI".into(), decimals: 9, icon_url: None, price_usd: price }
}

fn coins(list: &[(&str, ReviewCoin)]) -> BTreeMap<String, ReviewCoin> {
    list.iter().map(|(t, c)| (norm_type(t), c.clone())).collect()
}

fn fee_row(net: i128) -> ReviewRow {
    use crate::screens::wallet::fmt::format_amount;
    if net < 0 {
        ReviewRow { kind: RowKind::Fee, label: "Network fee".into(), text: format!("{} SUI back", format_amount(-net, 9)), sub: Some("Storage refund is larger than the fee".into()), coin_type: Some(sui_type()), amount: Some(net), icon_url: None, object_id: None, image: None }
    } else {
        ReviewRow { kind: RowKind::Fee, label: "Network fee".into(), text: format!("{} SUI", format_amount(net, 9)), sub: None, coin_type: Some(sui_type()), amount: Some(net), icon_url: None, object_id: None, image: None }
    }
}

fn coin_out(kind: RowKind, coin_type: &str, amount: i128, symbol: &str, decimals: u32) -> ReviewRow {
    use crate::screens::wallet::fmt::format_amount;
    ReviewRow {
        kind,
        label: if kind == RowKind::Send { "You send" } else { "You receive" }.into(),
        text: format!("{} {symbol}", format_amount(amount, decimals)),
        sub: None,
        coin_type: Some(coin_type.into()),
        amount: Some(amount),
        icon_url: None,
        object_id: None,
        image: None,
    }
}

fn base() -> Review {
    Review {
        ok: true,
        reason: None,
        risks: vec![],
        rows: vec![],
        coins: coins(&[("0x2::sui::SUI", sui_coin(Some(0.906)))]),
        send: vec![],
        others_receive: vec![],
        calls: vec![],
        shared: vec![],
        originals: BTreeMap::new(),
        bytes: Some(vec![1, 2, 3]),
    }
}

/// The dev hook's `sendDemo`: 0.1 SUI to another address the wallet never sent to.
pub fn send_review() -> Review {
    Review {
        rows: vec![coin_out(RowKind::Send, &sui_type(), 100_000_000, "SUI", 9), fee_row(1_997_880)],
        send: vec![CoinFlow { coin_type: sui_type(), amount: 100_000_000 }],
        others_receive: vec![OtherReceive { address: OTHER.into(), coin_type: sui_type(), amount: 100_000_000 }],
        risks: vec![Risk { id: "new-recipient".into(), text: format!("You have not sent to {} before.", crate::screens::wallet::fmt::addr_short(OTHER)), danger: false, requires_confirm: false }],
        calls: vec![],
        ..base()
    }
}

/// A swap on the Trade tab: SUI out, USDC in, Rare's 0.2% fee line, a Cetus call.
pub fn swap_review() -> Review {
    Review {
        rows: vec![coin_out(RowKind::Send, &sui_type(), 10_000_000_000, "SUI", 9), coin_out(RowKind::Receive, USDC, 9_041_200, "USDC", 6), fee_row(3_120_400)],
        coins: coins(&[("0x2::sui::SUI", sui_coin(Some(0.906))), (USDC, ReviewCoin { symbol: "USDC".into(), decimals: 6, icon_url: Some("https://circle.com/usdc.png".into()), price_usd: Some(1.0) })]),
        send: vec![CoinFlow { coin_type: sui_type(), amount: 10_000_000_000 }],
        others_receive: vec![OtherReceive { address: RARE_FEE.into(), coin_type: sui_type(), amount: 20_000_000 }],
        calls: vec![MoveCallRef { package: "0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb".into(), module: "pool_script_v2".into(), function: "swap_a2b".into() }],
        shared: vec![SharedTouch { object_id: "0xcf994611fd4c48e277ce3ffd4d4364c914af2c3cbb05f7bf6facd371de688630".into(), label: "Cetus Pool".into() }],
        ..base()
    }
}

/// A call that aborts: the banner, Approve off, Details open.
pub fn failed_review() -> Review {
    Review {
        ok: false,
        reason: Some("Not enough balance for this.".into()),
        risks: vec![Risk { id: "failed".into(), text: "This transaction would fail.".into(), danger: true, requires_confirm: false }],
        rows: vec![fee_row(1_000_000)],
        calls: vec![MoveCallRef { package: "0x2".into(), module: "balance".into(), function: "split".into() }],
        ..base()
    }
}

/// A lookalike recipient: the danger note and the extra tick before Approve.
pub fn lookalike_review() -> Review {
    let mut r = send_review();
    r.risks = vec![
        Risk { id: "new-recipient".into(), text: "You have not sent to 0xabab...abab before.".into(), danger: false, requires_confirm: false },
        Risk { id: "lookalike".into(), text: "This address looks like one you sent to before, but it is different. Check every character.".into(), danger: true, requires_confirm: true },
    ];
    r
}

/// A game's sponsored transaction: an item arrives, the fee is paid by the game.
pub fn sponsored_review() -> Review {
    Review {
        rows: vec![
            ReviewRow {
                kind: RowKind::Receive,
                label: "You receive".into(),
                text: "Rusty Blade".into(),
                sub: Some("New".into()),
                coin_type: None,
                amount: None,
                icon_url: None,
                object_id: Some("0x77".into()),
                image: Some("https://act.game/items/rusty-blade.png".into()),
            },
            ReviewRow { kind: RowKind::Fee, label: "Network fee".into(), text: "Paid by ACT".into(), sub: None, coin_type: Some(sui_type()), amount: Some(0), icon_url: None, object_id: None, image: None },
        ],
        ..base()
    }
}

/// Nothing moves (a profile update): one sentence instead of sections.
pub fn nothing_review() -> Review {
    Review { rows: vec![fee_row(-120_000)], ..base() }
}
