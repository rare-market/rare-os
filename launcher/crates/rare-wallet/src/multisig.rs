//! Passkey accounts. A Rare passkey account is a 1-of-2 Sui multisig:
//!   member 0: the person's passkey (secp256r1, Sui's passkey scheme, flag 0x06), weight 1
//!   member 1: an Ed25519 key kept on the device, sealed in the PIN vault, weight 1
//! threshold 1. Either member alone signs. The member order and weights are part of the address,
//! so they never change. Port of src/wallet/core/passkey/{multisig,signer}.ts.
//!
//! A passkey signs a WebAuthn assertion whose challenge is the 32-byte intent digest;
//! `passkey_signature` turns the assertion into Sui's passkey signature (low-S, like the TS SDK).

use crate::keys::{personal_message_digest, tx_digest, Keypair, FLAG_ED25519, FLAG_PASSKEY};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use sui_sdk_types::{
    Ed25519PublicKey, MultisigAggregatedSignature, MultisigCommittee, MultisigMember, MultisigMemberPublicKey, MultisigMemberSignature,
    PasskeyAuthenticator, PasskeyPublicKey, Secp256r1PublicKey, Secp256r1Signature, SimpleSignature, UserSignature,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultisigError(pub String);

impl std::fmt::Display for MultisigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MultisigError {}

fn e(m: impl Into<String>) -> MultisigError {
    MultisigError(m.into())
}

/// The account's committee from the passkey's compressed P-256 key (33 bytes) and the device's
/// Ed25519 key (32 bytes).
pub fn passkey_multisig(passkey_public_key: &[u8], device_public_key: &[u8]) -> Result<MultisigCommittee, MultisigError> {
    rare_committee(&[passkey_public_key], device_public_key)
}

/// Most passkeys one account takes (Sui allows 10 members: the device key and 9 passkeys).
pub const MAX_PASSKEYS: usize = 9;

/// A Rare account with one or more passkeys (a phone, a security key): passkeys[0], then the
/// device key, then passkeys[1..], every weight 1, threshold 1. One passkey is the original
/// 1-of-2 layout, so those addresses never change.
pub fn rare_committee(passkeys: &[&[u8]], device_public_key: &[u8]) -> Result<MultisigCommittee, MultisigError> {
    if passkeys.is_empty() || passkeys.len() > MAX_PASSKEYS {
        return Err(e("A Rare account takes 1 to 9 passkeys"));
    }
    let dk: [u8; 32] = device_public_key.try_into().map_err(|_| e("Expected an Ed25519 device key (32 bytes)"))?;
    let member = |k: &[u8]| -> Result<MultisigMember, MultisigError> {
        let pk: [u8; 33] = k.try_into().map_err(|_| e("Expected a compressed secp256r1 passkey key (33 bytes)"))?;
        Ok(MultisigMember::new(MultisigMemberPublicKey::Passkey(PasskeyPublicKey::new(Secp256r1PublicKey::new(pk))), 1))
    };
    let mut members = vec![member(passkeys[0])?, MultisigMember::new(MultisigMemberPublicKey::Ed25519(Ed25519PublicKey::new(dk)), 1)];
    for k in &passkeys[1..] {
        let m = member(k)?;
        if members.iter().any(|x| x.public_key() == m.public_key()) {
            return Err(e("The same passkey twice"));
        }
        members.push(m);
    }
    Ok(MultisigCommittee::new(members, 1))
}

/// `rare_committee` from base64 keys.
pub fn rare_committee_b64(passkeys: &[&str], device_public_key: &str) -> Result<MultisigCommittee, MultisigError> {
    let pks = passkeys.iter().map(|p| B64.decode(p).map_err(|_| e("Invalid passkey public key"))).collect::<Result<Vec<_>, _>>()?;
    let dk = B64.decode(device_public_key).map_err(|_| e("Invalid device key"))?;
    rare_committee(&pks.iter().map(|v| v.as_slice()).collect::<Vec<_>>(), &dk)
}

/// Committee from an account's base64 keys.
pub fn passkey_multisig_b64(passkey_public_key: &str, device_public_key: &str) -> Result<MultisigCommittee, MultisigError> {
    let pk = B64.decode(passkey_public_key).map_err(|_| e("Invalid passkey public key"))?;
    let dk = B64.decode(device_public_key).map_err(|_| e("Invalid device key"))?;
    passkey_multisig(&pk, &dk)
}

pub fn multisig_address(c: &MultisigCommittee) -> String {
    c.derive_address().to_string()
}

/// The committee as TS `MultiSigPublicKey.toRawBytes()`: flag-less BCS, what the account
/// metadata stores as the account's public key.
pub fn multisig_public_key_bytes(c: &MultisigCommittee) -> Vec<u8> {
    bcs::to_bytes(c).expect("committee serializes")
}

/// The address of a passkey on its own (flag 0x06).
pub fn passkey_address(passkey_public_key: &[u8]) -> Result<String, MultisigError> {
    let pk: [u8; 33] = passkey_public_key.try_into().map_err(|_| e("Invalid passkey public key"))?;
    Ok(PasskeyPublicKey::new(Secp256r1PublicKey::new(pk)).derive_address().to_string())
}

fn parse_user_signature(serialized: &[u8]) -> Result<UserSignature, MultisigError> {
    UserSignature::from_bytes(serialized).map_err(|_| e("Unreadable member signature"))
}

/// Wraps one member's serialized signature (base64) into the account's multisig signature
/// (base64), TS `combinePartialSignatures([sig])`.
pub fn combine_member(c: &MultisigCommittee, member_signature_b64: &str) -> Result<String, MultisigError> {
    let raw = B64.decode(member_signature_b64).map_err(|_| e("Unreadable member signature"))?;
    let (public_key, signature) = match parse_user_signature(&raw)? {
        UserSignature::Simple(SimpleSignature::Ed25519 { signature, public_key }) => (MultisigMemberPublicKey::Ed25519(public_key), MultisigMemberSignature::Ed25519(signature)),
        UserSignature::Simple(SimpleSignature::Secp256k1 { signature, public_key }) => (MultisigMemberPublicKey::Secp256k1(public_key), MultisigMemberSignature::Secp256k1(signature)),
        UserSignature::Simple(SimpleSignature::Secp256r1 { signature, public_key }) => (MultisigMemberPublicKey::Secp256r1(public_key), MultisigMemberSignature::Secp256r1(signature)),
        UserSignature::Passkey(p) => (MultisigMemberPublicKey::Passkey(p.public_key()), MultisigMemberSignature::Passkey(p)),
        _ => return Err(e("Unsupported member signature")),
    };
    let idx = c.members().iter().position(|m| m.public_key() == &public_key).ok_or_else(|| e("Received signature from unknown public key"))?;
    let agg = MultisigAggregatedSignature::new(c.clone(), vec![signature], 1u16 << idx);
    Ok(UserSignature::Multisig(agg).to_base64())
}

/// A WebAuthn assertion as the authenticator returned it.
#[derive(Debug, Clone)]
pub struct AssertionParts<'a> {
    pub authenticator_data: &'a [u8],
    /// the exact clientDataJSON bytes the authenticator signed over
    pub client_data_json: &'a [u8],
    /// DER ECDSA signature
    pub signature_der: &'a [u8],
}

/// Sui's passkey signature (flag 0x06 || BCS PasskeyAuthenticator), base64, for an assertion
/// over `digest` by the passkey whose compressed key is `passkey_public_key`. Refuses an
/// assertion whose challenge isn't `digest` or whose signature doesn't verify.
pub fn passkey_signature(passkey_public_key: &[u8], digest: &[u8; 32], a: &AssertionParts<'_>) -> Result<String, MultisigError> {
    let pk: [u8; 33] = passkey_public_key.try_into().map_err(|_| e("Invalid signature or public key length"))?;
    let sig = p256::ecdsa::Signature::from_der(a.signature_der).map_err(|_| e("Invalid passkey signature"))?;
    let sig = sig.normalize_s().unwrap_or(sig);
    let cdj = std::str::from_utf8(a.client_data_json).map_err(|_| e("Invalid clientDataJSON"))?.to_string();
    let simple = SimpleSignature::Secp256r1 { signature: Secp256r1Signature::new(sig.to_bytes().into()), public_key: Secp256r1PublicKey::new(pk) };
    let auth = PasskeyAuthenticator::new(a.authenticator_data.to_vec(), cdj, simple).ok_or_else(|| e("Invalid passkey assertion"))?;
    let user = UserSignature::Passkey(auth);
    use sui_crypto::Verifier as _;
    sui_crypto::passkey::PasskeyVerifier::new().verify(digest, &user).map_err(|_| e("The passkey signed something else"))?;
    debug_assert_eq!(user.to_bytes()[0], FLAG_PASSKEY);
    Ok(user.to_base64())
}

/// Signs as a passkey account with its device member (no phone needed).
pub struct MultisigDeviceSigner {
    pub committee: MultisigCommittee,
    pub device: Keypair,
}

impl MultisigDeviceSigner {
    pub fn new(committee: MultisigCommittee, device: Keypair) -> Result<Self, MultisigError> {
        if device.flag() != FLAG_ED25519 {
            return Err(e("The device key must be Ed25519"));
        }
        let mine = MultisigMemberPublicKey::Ed25519(Ed25519PublicKey::new(device.public_key().try_into().expect("32 bytes")));
        if !committee.members().iter().any(|m| m.public_key() == &mine) {
            return Err(e("Device key does not match"));
        }
        Ok(Self { committee, device })
    }

    pub fn address(&self) -> String {
        multisig_address(&self.committee)
    }

    pub fn sign_transaction(&self, tx_bytes: &[u8]) -> Result<String, MultisigError> {
        combine_member(&self.committee, &B64.encode(self.device.sign_digest(&tx_digest(tx_bytes))))
    }

    pub fn sign_personal_message(&self, message: &[u8]) -> Result<String, MultisigError> {
        combine_member(&self.committee, &B64.encode(self.device.sign_digest(&personal_message_digest(message))))
    }
}
