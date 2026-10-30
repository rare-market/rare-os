//! The TPM side, over tpm2-tools (shipped in Rare OS) on the kernel's resource manager
//! (/dev/tpmrm0). Every object is transient: the EK is recreated from the standard template
//! (TCG L-1, the same key as the factory EK at 0x81010001), the AK is a fresh restricted signing
//! key, and both vanish when the tools exit. Nothing is written to the TPM's NV or persistent
//! handles. The working files live in a 0700 directory under $XDG_RUNTIME_DIR that is removed when
//! the [`Session`] drops.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DEVICE: &str = "/dev/tpmrm0";

/// Whether this device can attest right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TpmStatus {
    Ready,
    /// No TPM 2.0 (or the kernel's resource manager is missing).
    NoTpm,
    /// The TPM is there but this user can't open it (not in the `tss` group).
    NoAccess,
    /// tpm2-tools is not installed.
    NoTools,
}

fn tcti() -> String {
    std::env::var("RARE_TPM_TCTI").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("device:{DEVICE}"))
}

pub fn status() -> TpmStatus {
    let swtpm = std::env::var("RARE_TPM_TCTI").is_ok_and(|s| !s.is_empty());
    if !swtpm {
        if !Path::new(DEVICE).exists() {
            return TpmStatus::NoTpm;
        }
        if fs::OpenOptions::new().read(true).write(true).open(DEVICE).is_err() {
            return TpmStatus::NoAccess;
        }
    }
    match Command::new("tpm2_createek").arg("--version").output() {
        Ok(o) if o.status.success() => TpmStatus::Ready,
        _ => TpmStatus::NoTools,
    }
}

pub struct Session {
    dir: PathBuf,
}

impl Drop for Session {
    fn drop(&mut self) {
        // Belt and braces: the resource manager already flushes a closed connection's objects.
        let _ = Command::new("tpm2_flushcontext").arg("-t").env("TPM2TOOLS_TCTI", tcti()).output();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn work_dir() -> std::io::Result<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let dir = base.join(format!("rare-attest-{}-{nanos}", std::process::id()));
    fs::create_dir(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

impl Session {
    pub fn new() -> Result<Session, String> {
        Ok(Session { dir: work_dir().map_err(|e| format!("could not make a work folder: {e}"))? })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new(args[0]).args(&args[1..]).current_dir(&self.dir).env("TPM2TOOLS_TCTI", tcti()).output().map_err(|e| format!("{}: {e}", args[0]))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("failed").trim().to_string();
            return Err(format!("{}: {last}", args[0]));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Creates the transient EK and an AK under it. Returns (EK TPM2B_PUBLIC, AK TPM2B_PUBLIC).
    pub fn keys(&self) -> Result<(Vec<u8>, Vec<u8>), String> {
        self.run(&["tpm2_createek", "-c", "ek.ctx", "-G", "rsa", "-u", "ek.pub"])?;
        self.run(&["tpm2_createak", "-C", "ek.ctx", "-c", "ak.ctx", "-G", "rsa", "-s", "rsassa", "-g", "sha256", "-u", "ak.pub", "-n", "ak.name", "-f", "tss"])?;
        let ek = fs::read(self.path("ek.pub")).map_err(|e| e.to_string())?;
        let ak = fs::read(self.path("ak.pub")).map_err(|e| e.to_string())?;
        Ok((ek, ak))
    }

    /// The EK certificate from NV, when the TPM keeps one (AMD fTPMs don't: the attestor fetches it).
    pub fn ek_cert(&self) -> Option<Vec<u8>> {
        self.run(&["tpm2_nvread", "0x01c00002", "-o", "ekcert.der"]).ok()?;
        fs::read(self.path("ekcert.der")).ok().filter(|b| !b.is_empty())
    }

    /// TPM2_ActivateCredential with the EK (PolicySecret(endorsement) session) for the AK.
    pub fn activate(&self, credential_file: &[u8]) -> Result<Vec<u8>, String> {
        fs::write(self.path("cred.bin"), credential_file).map_err(|e| e.to_string())?;
        self.run(&["tpm2_startauthsession", "--policy-session", "-S", "s.ctx"])?;
        let r = self.run(&["tpm2_policysecret", "-S", "s.ctx", "-c", "e"]).and_then(|_| self.run(&["tpm2_activatecredential", "-c", "ak.ctx", "-C", "ek.ctx", "-i", "cred.bin", "-o", "secret.bin", "-P", "session:s.ctx"]));
        let _ = self.run(&["tpm2_flushcontext", "s.ctx"]);
        r?;
        fs::read(self.path("secret.bin")).map_err(|e| e.to_string())
    }

    /// Quotes `pcrs` (SHA-256 bank) over `nonce`. Returns (TPMS_ATTEST, TPMT_SIGNATURE, PCR values).
    pub fn quote(&self, pcrs: &[u32], nonce_hex: &str) -> Result<Quote, String> {
        if nonce_hex.len() > 128 || !nonce_hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("bad nonce".into());
        }
        let sel = format!("sha256:{}", pcrs.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(","));
        let out = self.run(&["tpm2_quote", "-c", "ak.ctx", "-l", &sel, "-q", nonce_hex, "-m", "q.msg", "-s", "q.sig", "-o", "q.pcrs", "-g", "sha256"])?;
        let values = parse_pcrs(&out);
        if pcrs.iter().any(|p| !values.iter().any(|(i, _)| i == p)) {
            return Err("tpm2_quote did not report every PCR".into());
        }
        Ok(Quote { attest: fs::read(self.path("q.msg")).map_err(|e| e.to_string())?, signature: fs::read(self.path("q.sig")).map_err(|e| e.to_string())?, pcrs: values })
    }
}

pub struct Quote {
    pub attest: Vec<u8>,
    pub signature: Vec<u8>,
    pub pcrs: Vec<(u32, String)>,
}

/// The `pcrs: sha256: N : 0x...` block tpm2_quote prints.
pub fn parse_pcrs(yaml: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut in_sha256 = false;
    for line in yaml.lines() {
        let t = line.trim();
        if t.ends_with(':') && !t.contains(' ') {
            in_sha256 = t == "sha256:";
            continue;
        }
        if !in_sha256 {
            continue;
        }
        if let Some((i, v)) = t.split_once(':') {
            let v = v.trim().trim_start_matches("0x").to_ascii_lowercase();
            if let Ok(i) = i.trim().parse::<u32>() {
                if v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()) {
                    out.push((i, v));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcrs_from_quote_output() {
        let y = "quoted: ff54\nsignature:\n  alg: rsassa\n  sig: 8366\npcrs:\n  sha256:\n    0 : 0xA98374F93455A10A48BC1F2F8ABB937DB3376B3FEFF19240215D2ED8BA766768\n    4 : 0xBAFFE4DE15D115CA2E35C5DDEA047AC98E66C4EBEE7E59E97E8939A0CA468BE0\n    7 : 0xB926225AC488E9C50EF2FA815AA7104B385A06907093BFB1DC62EEB7ABECDDF1\ncalcDigest: 5512\n";
        let p = parse_pcrs(y);
        assert_eq!(p.len(), 3);
        assert_eq!(p[0], (0, "a98374f93455a10a48bc1f2f8abb937db3376b3feff19240215d2ed8ba766768".to_string()));
        assert_eq!(p[2].0, 7);
    }
}
