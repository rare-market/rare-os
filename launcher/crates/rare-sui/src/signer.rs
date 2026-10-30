//! The one signing door the Sui features use (`CloudSigner` in src/sui/signer.ts). In the
//! launcher it is Rare Wallet (rare-wallet implements it: every request goes through the wallet's
//! review sheet, nothing is signed without the player approving it). This crate never holds a key.

use crate::ptb::Tx;
use crate::BoxFut;

pub trait TxSigner: Send + Sync {
    /// The account that signs (and owns what the transactions create).
    fn address(&self) -> String;
    /// Signs a personal message; answers the base64 serialized signature.
    fn sign_personal_message(&self, message: &[u8], title: &str) -> BoxFut<'_, String>;
    /// Builds, reviews, signs and executes `tx` (sender set to [`TxSigner::address`]); resolves
    /// with the digest once the network has the transaction.
    fn sign_and_execute(&self, tx: Tx, title: &str) -> BoxFut<'_, String>;
}
