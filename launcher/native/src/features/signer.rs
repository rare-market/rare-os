//! The wallet's signing door (`rare_sui::signer::TxSigner`) for the selected account, from Rare
//! Wallet's service (N2). Every request goes through the wallet's review sheet; call the signer
//! from a worker thread. None without a signing account: tell the player to sign in.

use rare_sui::signer::TxSigner;
use std::sync::Arc;

pub fn current() -> Option<Arc<dyn TxSigner>> {
    crate::screens::wallet::service::tx_signer().map(|s| Arc::new(s) as Arc<dyn TxSigner>)
}
