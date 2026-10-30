//! Signing for the screens that trade (Market, Trade, the inventory), over Rare Wallet
//! (`screens::wallet::service`): every request goes through the wallet's review sheet.
//!
//! - [`sign_and_execute`]: a market PTB (buy, cart, sweep) as "Rare Marketplace", Rare covering the
//!   fee when the wallet is short of SUI and the sponsor offers trades.
//! - [`sign_trade`]: Trade's ready-made transactions (`rare_trade::wallet::SignRequest`).
//! - [`executor`]: rare-market's inventory executor (sell into offers, list, cancel).
//!
//! Every call here blocks until the person answers the sheet: run it on a worker thread, never the
//! UI thread.

use crate::screens::wallet::service;
use rare_market::inventory::trade::Executor;
use rare_sui::ptb::Tx;
use rare_trade::wallet::{SignOutcome, SignRequest};
use std::sync::Arc;

/// The account the market screens act for: the wallet's signing account when there is one, else
/// the selected account (watch-only for these screens).
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub address: String,
    pub label: String,
    pub signer: bool,
}

pub fn account() -> Option<Account> {
    if std::env::var("RR_FIXTURE").is_ok() {
        return None;
    }
    let (list, selected) = crate::screens::wallet::page::load_accounts();
    let pick = |addr: &str| list.iter().find(|a| a.address.eq_ignore_ascii_case(addr));
    if let Some(s) = service::tx_signer() {
        let label = pick(&s.account).map(|a| a.label.clone()).unwrap_or_else(|| "Wallet".into());
        return Some(Account { address: s.account, label, signer: true });
    }
    let cur = selected.as_deref().and_then(pick).or(list.first())?;
    Some(Account { address: cur.address.clone(), label: cur.label.clone(), signer: false })
}

pub const NO_SIGNER: &str = "Add a passkey or an account in Rare Wallet to do this here.";

/// Builds, reviews, signs and executes a marketplace transaction for `account`. Blocks. Ok(digest);
/// Err(SIGN_REJECTED) when the person declined.
pub fn sign_and_execute(tx: Tx, account: &str, title: &str) -> Result<String, String> {
    use rare_wallet::sign::{OriginKind, SignError, SignOptions, SignOrigin};
    let w = service::wallet().ok_or_else(|| "Rare Wallet is not available".to_string())?;
    let o = SignOptions { account: Some(account.to_string()), origin: Some(SignOrigin::new("Rare Marketplace", OriginKind::Market)), title: Some(title.to_string()), ..Default::default() };
    match w.signing.sign_and_execute_with_cover(tx, rare_wallet::sponsor::Category::Trade, o).and_then(|t| t.wait()) {
        Ok(r) if r.status.as_deref() == Some("failure") => Err("The network rejected this transaction.".into()),
        Ok(r) => {
            crate::data::keep::nudge();
            Ok(r.digest.unwrap_or_default())
        }
        Err(SignError::Rejected(_)) => Err(rare_market::inventory::notice::SIGN_REJECTED.into()),
        Err(e) => Err(e.to_string()),
    }
}

/// A transaction a game asked for (a Local API claim): the game's gas budget pays first
/// (`game:<slug>`), then, with `trophy_cover`, Rare's trophy cover when the wallet is short of
/// SUI, else the player. Reviewed in Rare Wallet as coming from the game. Blocks. Ok(digest);
/// Err(SIGN_REJECTED) when the person declined.
pub fn sign_for_game(tx: Tx, account: &str, slug: Option<&str>, game_name: &str, title: &str, trophy_cover: bool) -> Result<String, String> {
    use rare_wallet::sign::{OriginKind, SignError, SignOptions, SignOrigin, TxInput};
    use rare_wallet::sponsor::Category;
    let w = service::wallet().ok_or_else(|| "Rare Wallet is not available".to_string())?;
    let o = SignOptions { account: Some(account.to_string()), origin: Some(SignOrigin::new(game_name, OriginKind::Game)), title: Some(title.to_string()), ..Default::default() };
    let cover = tx.clone();
    let fallback = |o: SignOptions| w.signing.sign_and_execute_with_cover(cover.clone(), Category::Trophy, o);
    let fb: Option<&dyn Fn(SignOptions) -> Result<rare_wallet::sign::Ticket, SignError>> = if trophy_cover { Some(&fallback) } else { None };
    match w.signing.sign_and_execute_for_game(TxInput::Ptb(tx), slug, game_name, o, fb).and_then(|t| t.wait()) {
        Ok(r) if r.status.as_deref() == Some("failure") => Err("The network rejected the claim".into()),
        Ok(r) => {
            crate::data::keep::nudge();
            Ok(r.digest.unwrap_or_default())
        }
        Err(SignError::Rejected(_)) => Err(rare_market::inventory::notice::SIGN_REJECTED.into()),
        Err(e) => Err(e.to_string()),
    }
}

/// The Device Pass claim (Settings > About), reviewed as coming from Rare, Rare covering the fee
/// when the wallet is short of SUI and the sponsor offers device claims. Blocks. Ok(digest);
/// Err(SIGN_REJECTED) when the person declined.
pub fn sign_device_pass(tx: Tx, account: &str, title: &str) -> Result<String, String> {
    use rare_wallet::sign::{OriginKind, SignError, SignOptions, SignOrigin};
    let w = service::wallet().ok_or_else(|| "Rare Wallet is not available".to_string())?;
    let o = SignOptions { account: Some(account.to_string()), origin: Some(SignOrigin::new("Rare", OriginKind::Wallet)), title: Some(title.to_string()), ..Default::default() };
    match w.signing.sign_and_execute_with_cover(tx, rare_wallet::sponsor::Category::Device, o).and_then(|t| t.wait()) {
        Ok(r) if r.status.as_deref() == Some("failure") => Err("The network rejected this claim.".into()),
        Ok(r) => Ok(r.digest.unwrap_or_default()),
        Err(SignError::Rejected(_)) => Err(rare_market::inventory::notice::SIGN_REJECTED.into()),
        Err(e) => Err(e.to_string()),
    }
}

/// Hands a Trade request to Rare Wallet. Blocks.
pub fn sign_trade(req: &SignRequest, reader: Option<&dyn rare_trade::chain::ChainReader>) -> SignOutcome {
    service::trade_sign(req, reader)
}

/// rare-market's inventory executor (sell, list, cancel) over Rare Wallet.
pub fn executor() -> Arc<dyn Executor> {
    service::executor()
}
