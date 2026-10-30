//! deriveSecret building blocks: 32 secret bytes, the same for a given account and context on
//! any device (cloud saves take their key from here). Port of src/wallet/secret.ts.
//!
//! - phrase or imported key: HKDF-SHA256 over the private key (`Keystore::derive_from_key`)
//! - passkey: the WebAuthn PRF output for `prf_input(context)`, then `finish`
//! - Ledger / Keystone: the device signs `derive_message` (Ed25519 is deterministic), then
//!   `finish(ed25519_signature_bytes(sig))`
//!
//! Every path runs the same HKDF with the account address, so contexts and accounts never share
//! a secret. Changing any of these changes every derived key: bump `DERIVE_SALT` instead.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// HKDF salt shared by every derivation.
pub const DERIVE_SALT: &[u8] = b"rare/derive/v1";

/// HKDF info: the context and the account.
pub fn derive_info(context: &str, address: &str) -> Vec<u8> {
    format!("rare/{context}\u{0}{}", address.to_lowercase()).into_bytes()
}

/// The WebAuthn PRF input for a context (32 bytes).
pub fn prf_input(context: &str) -> [u8; 32] {
    Sha256::digest(format!("rare/{context}").as_bytes()).into()
}

/// The message a Ledger or Keystone signs to derive a secret. Never changes for a version.
pub fn derive_message(context: &str, address: &str) -> Vec<u8> {
    format!(
        "Rare key\n\nSign to unlock \"{context}\" on this device. This is not a transaction and costs nothing.\n\nAccount: {}\nVersion: 1",
        address.to_lowercase()
    )
    .into_bytes()
}

/// Final step for every kind: 32 bytes from raw key material, bound to context and account.
pub fn finish(ikm: &[u8], context: &str, address: &str) -> Zeroizing<[u8; 32]> {
    let mut out = Zeroizing::new([0u8; 32]);
    hkdf::Hkdf::<Sha256>::new(Some(DERIVE_SALT), ikm).expand(&derive_info(context, address), &mut out[..]).expect("32 bytes");
    out
}

/// A context the API accepts: 1 to 64 UTF-16 units, like secret.ts.
pub fn valid_context(context: &str) -> bool {
    let n = context.encode_utf16().count();
    (1..=64).contains(&n)
}

/// An Ed25519 Sui signature (flag 0x00 | sig 64 | pk 32), base64 -> its 64 signature bytes.
pub fn ed25519_signature_bytes(serialized: &str) -> Result<[u8; 64], &'static str> {
    let b = B64.decode(serialized).map_err(|_| "This account's signatures can't derive a key")?;
    if b.len() != 97 || b[0] != 0x00 {
        return Err("This account's signatures can't derive a key");
    }
    Ok(b[1..65].try_into().expect("64 bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{hex, Keypair};

    const PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    const ADDR: &str = "0x5e93a736d04fbb25737aa40bee40171ef79f65fae833749e3c089fe7cc2161f1";

    // Pinned in secret.test.ts: if these change, every key derived on a device changes.
    #[test]
    fn prf_input_is_sha256_of_rare_context() {
        assert_eq!(hex(&prf_input("cloud-saves")), "e98ccacee8c68d9008a2fe288bdf97b5ffe3e2c855bc02de82f261e45060ecbe");
    }

    #[test]
    fn ledger_path_is_deterministic_and_pinned() {
        let kp = Keypair::from_mnemonic(PHRASE, 0).unwrap();
        let sign = || kp.sign_personal_message(&derive_message("cloud-saves", ADDR));
        let (x, y) = (sign(), sign());
        let s = finish(&ed25519_signature_bytes(&x).unwrap(), "cloud-saves", ADDR);
        assert_eq!(*s, *finish(&ed25519_signature_bytes(&y).unwrap(), "cloud-saves", ADDR));
        assert_eq!(hex(&s[..]), "e5c10994260a631eea86a4252e3984ac69de5c45e9c6c486d4f1db687b3e1bf3");
    }

    #[test]
    fn refuses_non_ed25519_signatures() {
        let mut b = vec![0x02u8];
        b.extend([0u8; 96]);
        assert!(ed25519_signature_bytes(&B64.encode(b)).is_err());
        assert!(ed25519_signature_bytes("!").is_err());
    }

    #[test]
    fn contexts() {
        assert!(valid_context("cloud-saves"));
        assert!(!valid_context(""));
        assert!(!valid_context(&"x".repeat(65)));
        assert!(valid_context(&"x".repeat(64)));
    }
}

/// deriveSecret: 32 bytes, deterministic per account and context, asking for what the account
/// needs. Native accounts need the vault unlocked first (the UI asks for the PIN on `Locked`);
/// passkeys evaluate PRF (one tap); Ledger and Keystone sign `derive_message` through the review
/// sheet, so this blocks until the person approves there: call it off the Approve thread.
#[cfg(feature = "chain")]
pub mod derive {
    use super::*;
    use crate::keystore::{AccountKind, AccountStore, KeystoreError};
    use crate::passkey_account::{self, Authenticator, PasskeyError, Via};
    use crate::sign::{Chain, SignError, SignOptions, SignOrigin, Signing};
    use crate::sponsor::SponsorApi;

    #[derive(Debug, Clone, PartialEq)]
    pub enum DeriveError {
        /// The account can't derive one here, with the words to show.
        Unavailable(String),
        Keystore(KeystoreError),
        Passkey(PasskeyError),
        Sign(SignError),
    }

    impl std::fmt::Display for DeriveError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                DeriveError::Unavailable(m) => f.write_str(m),
                DeriveError::Keystore(e) => write!(f, "{e}"),
                DeriveError::Passkey(e) => write!(f, "{e}"),
                DeriveError::Sign(e) => write!(f, "{e}"),
            }
        }
    }

    impl std::error::Error for DeriveError {}

    pub fn derive_secret<S: AccountStore, C: Chain, A: SponsorApi>(
        signing: &Signing<S, C, A>,
        passkeys: &mut dyn Authenticator,
        via: Via,
        context: &str,
        account: Option<&str>,
        title: Option<&str>,
    ) -> Result<Zeroizing<[u8; 32]>, DeriveError> {
        if !valid_context(context) {
            return Err(DeriveError::Unavailable("Invalid context".into()));
        }
        let ks = &signing.keystore;
        let acc = match account {
            Some(a) => ks.list_accounts().map_err(DeriveError::Keystore)?.into_iter().find(|x| x.address == a),
            None => ks.current().map_err(DeriveError::Keystore)?,
        }
        .ok_or_else(|| DeriveError::Unavailable("Add a Rare Wallet account first".into()))?;
        match acc.kind {
            AccountKind::Watch => Err(DeriveError::Unavailable("This account is watch only. Use an account you can sign with.".into())),
            AccountKind::Native => ks.derive_from_key(&acc.address, DERIVE_SALT, &derive_info(context, &acc.address)).map_err(DeriveError::Keystore),
            AccountKind::Passkey => {
                let out = passkey_account::passkey_prf(&acc, passkeys, via, &prf_input(context)).map_err(DeriveError::Passkey)?;
                Ok(finish(&out, context, &acc.address))
            }
            AccountKind::Ledger | AccountKind::Keystone => {
                let o = SignOptions { account: Some(acc.address.clone()), origin: Some(SignOrigin::wallet()), title: Some(title.unwrap_or("Unlock with your device").into()), ..Default::default() };
                let r = signing.sign_personal_message(derive_message(context, &acc.address), o).map_err(DeriveError::Sign)?.wait().map_err(DeriveError::Sign)?;
                let sig = ed25519_signature_bytes(&r.signature).map_err(|m| DeriveError::Unavailable(m.into()))?;
                Ok(finish(&sig, context, &acc.address))
            }
            AccountKind::Social => Err(DeriveError::Unavailable("This account can't derive a key".into())),
        }
    }
}
