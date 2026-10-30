//! The device-bound key behind the device vault: a random 256-bit secret that opens the vault
//! holding this device's recovery-phrase accounts, so they unlock at login with no PIN.
//!
//! Where the secret is kept, best first:
//! - `tpm2`: sealed by `systemd-creds --user` (host key + TPM2, scoped to this user), so the file
//!   is useless on another machine or to another user. The 7840U fTPM is enough.
//! - `keyring`: the Secret Service item `rare-wallet/device-key`, when the default collection is
//!   unlocked with no prompt (KWallet opened at login), and the Mac keychain in development.
//! - `file`: a 0600 file, only where neither exists (tests, a dev box without a keyring).
//!
//! The secret never reaches logs, argv or error strings: systemd-creds gets it on stdin and
//! answers on stdout.

use crate::err;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use zeroize::Zeroizing;

const META: &str = "seal.json";
const CRED: &str = "key.cred";
const PLAIN: &str = "key";
const CRED_NAME: &str = "rare-wallet-device";
const KEYRING_USER: &str = "device-key";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Seal {
    Tpm2,
    Keyring,
    File,
}

impl Seal {
    pub fn as_str(self) -> &'static str {
        match self {
            Seal::Tpm2 => "tpm2",
            Seal::Keyring => "keyring",
            Seal::File => "file",
        }
    }
    /// How Settings describes it.
    pub fn describe(self) -> &'static str {
        match self {
            Seal::Tpm2 => "Locked to this SuiPlay's security chip",
            Seal::Keyring => "Locked to your login on this device",
            Seal::File => "Kept in a private file on this device",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Meta {
    seal: Seal,
}

/// The sealed secret for one directory (`<wallet dir>/device`).
pub struct DeviceSeal {
    dir: PathBuf,
    /// false in tests: file only, never systemd-creds or the keyring
    system: bool,
}

impl DeviceSeal {
    pub fn new(dir: PathBuf, system: bool) -> Self {
        DeviceSeal { dir, system }
    }

    fn meta(&self) -> Option<Meta> {
        fs::read(self.dir.join(META)).ok().and_then(|b| serde_json::from_slice(&b).ok())
    }

    /// How the secret is kept, when there is one.
    pub fn seal(&self) -> Option<Seal> {
        self.meta().map(|m| m.seal)
    }

    /// The secret, when one was made here.
    pub fn load(&self) -> Result<Option<Zeroizing<String>>, String> {
        let Some(m) = self.meta() else { return Ok(None) };
        let s = match m.seal {
            Seal::Tpm2 => tpm2_decrypt(&self.dir.join(CRED))?,
            Seal::Keyring => match crate::vault::keyring::get(KEYRING_USER) {
                Ok(Some(s)) => Zeroizing::new(s),
                Ok(None) => return Err(err("missing", "This device's key is gone from the keyring")),
                Err(()) => return Err(err("keyring", "The system keyring is locked")),
            },
            Seal::File => Zeroizing::new(fs::read_to_string(self.dir.join(PLAIN)).map_err(|e| err("error", format!("Could not read this device's key: {}", e.kind())))?),
        };
        Ok(Some(s))
    }

    /// Makes and seals a new secret, the best way this device allows. Returns it and the way.
    pub fn create(&self) -> Result<(Zeroizing<String>, Seal), String> {
        let mut raw = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(&mut raw[..]);
        let secret = Zeroizing::new(B64.encode(&raw[..]));
        fs::create_dir_all(&self.dir).map_err(|e| err("error", format!("Could not make the wallet folder: {}", e.kind())))?;
        let seal = if self.system && tpm2_encrypt(&secret, &self.dir.join(CRED)).is_ok() && tpm2_decrypt(&self.dir.join(CRED)).is_ok_and(|s| s.as_str() == secret.as_str()) {
            Seal::Tpm2
        } else if self.system && crate::vault::keyring::available() && crate::vault::keyring::set(KEYRING_USER, &secret).is_ok() && crate::vault::keyring::get(KEYRING_USER).ok().flatten().is_some_and(|s| s == *secret) {
            let _ = fs::remove_file(self.dir.join(CRED));
            Seal::Keyring
        } else {
            let _ = fs::remove_file(self.dir.join(CRED));
            crate::vault::write_atomic(&self.dir.join(PLAIN), secret.as_bytes()).map_err(|e| err("error", format!("Could not save this device's key: {}", e.kind())))?;
            Seal::File
        };
        let meta = serde_json::to_vec(&Meta { seal }).expect("plain JSON");
        crate::vault::write_atomic(&self.dir.join(META), &meta).map_err(|e| err("error", format!("Could not save this device's key: {}", e.kind())))?;
        log::info!("device vault key sealed with {}", seal.as_str());
        Ok((secret, seal))
    }

    /// Forgets the secret everywhere it may be.
    pub fn remove(&self) {
        if self.seal() == Some(Seal::Keyring) && self.system {
            let _ = crate::vault::keyring::delete(KEYRING_USER);
        }
        for f in [META, CRED, PLAIN] {
            let _ = fs::remove_file(self.dir.join(f));
        }
        // the folder too, once the vault's own files are gone (never anything else in it)
        let _ = fs::remove_dir(&self.dir);
    }
}

fn tpm2_encrypt(secret: &str, out: &std::path::Path) -> Result<(), ()> {
    seal_with(CRED_NAME, secret, out)
}

/// Seals `secret` into `out` with `systemd-creds --user` (host key + TPM2) under `name`.
pub(crate) fn seal_with(name: &str, secret: &str, out: &std::path::Path) -> Result<(), ()> {
    let mut child = Command::new("systemd-creds")
        .args(["encrypt", "--user", "--with-key=host+tpm2", &format!("--name={name}"), "-"])
        .arg(out)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| ())?;
    child.stdin.take().ok_or(())?.write_all(secret.as_bytes()).map_err(|_| ())?;
    let o = child.wait_with_output().map_err(|_| ())?;
    if !o.status.success() {
        log::info!("systemd-creds encrypt: {}", String::from_utf8_lossy(&o.stderr).trim());
        return Err(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(out, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn tpm2_decrypt(path: &std::path::Path) -> Result<Zeroizing<String>, String> {
    open_with(CRED_NAME, path)
}

/// Opens what `seal_with(name, ..)` sealed.
pub(crate) fn open_with(name: &str, path: &std::path::Path) -> Result<Zeroizing<String>, String> {
    let o = Command::new("systemd-creds")
        .args(["decrypt", "--user", &format!("--name={name}")])
        .arg(path)
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| err("error", format!("Could not run systemd-creds: {}", e.kind())))?;
    let out = Zeroizing::new(o.stdout);
    if !o.status.success() {
        return Err(err("error", format!("This device's security chip did not open the wallet ({})", String::from_utf8_lossy(&o.stderr).trim().chars().take(120).collect::<String>())));
    }
    std::str::from_utf8(&out).map(|s| Zeroizing::new(s.trim().to_string())).map_err(|_| err("error", "This device's key is damaged"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_seal_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let s = DeviceSeal::new(d.path().join("device"), false);
        assert!(s.load().unwrap().is_none());
        let (k, how) = s.create().unwrap();
        assert_eq!(how, Seal::File);
        assert_eq!(B64.decode(k.as_bytes()).unwrap().len(), 32);
        assert_eq!(s.load().unwrap().unwrap().as_str(), k.as_str());
        assert_eq!(s.seal(), Some(Seal::File));
        s.remove();
        assert!(s.load().unwrap().is_none());
    }
}
