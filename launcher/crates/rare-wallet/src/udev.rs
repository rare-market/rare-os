//! udev rules that let the logged-in user reach hardware wallets without sudo. Rare OS ships
//! them in image/system_files/usr/lib/udev/rules.d; on a plain Bazzite install the Ledger and
//! Keystone transports return error "udev" with `{"fix": fix_command()}` for the owner to paste
//! into Konsole. The texts here must stay identical to the image files (a test checks).

pub const LEDGER_RULES_FILE: &str = "70-rare-ledger.rules";
pub const LEDGER_RULES: &str = r#"# Rare OS: Ledger hardware wallets (USB vendor 2c97, every model) for Rare Wallet.
# uaccess gives the logged-in user access without sudo. Sorted before 73-seat-late.rules,
# which applies the uaccess tag.
SUBSYSTEM=="usb", ATTRS{idVendor}=="2c97", TAG+="uaccess"
KERNEL=="hidraw*", SUBSYSTEM=="hidraw", ATTRS{idVendor}=="2c97", TAG+="uaccess"
"#;

pub const KEYSTONE_RULES_FILE: &str = "70-rare-keystone.rules";
pub const KEYSTONE_RULES: &str = r#"# Rare OS: Keystone 3 Pro (USB 1209:3001) for Rare Wallet.
# uaccess gives the logged-in user access without sudo. Sorted before 73-seat-late.rules,
# which applies the uaccess tag.
# FIDO security keys need no rule here: systemd 60-fido-id.rules already tags them uaccess.
SUBSYSTEM=="usb", ATTRS{idVendor}=="1209", ATTRS{idProduct}=="3001", TAG+="uaccess"
"#;

/// The rules without their comment lines: what the pasted command writes (short enough to read
/// on screen; the image ships the commented files).
fn bare(text: &str) -> String {
    text.lines().filter(|l| !l.starts_with('#')).map(|l| format!("{l}\n")).collect()
}

/// `printf '%s\n' 'line' ... | <sudo>tee <dir>/<name> >/dev/null`. Every rules line is single
/// quoted, which is safe because the texts contain no single quote (a test checks).
fn write_cmd(dir: &str, sudo: &str, name: &str, text: &str) -> String {
    let lines: Vec<String> = bare(text).lines().map(|l| format!("'{l}'")).collect();
    format!("printf '%s\\n' {} | {sudo}tee {dir}/{name} >/dev/null", lines.join(" "))
}

fn write_both(dir: &str, sudo: &str) -> String {
    format!(
        "{sudo}mkdir -p {dir} && {} && {}",
        write_cmd(dir, sudo, LEDGER_RULES_FILE, LEDGER_RULES),
        write_cmd(dir, sudo, KEYSTONE_RULES_FILE, KEYSTONE_RULES)
    )
}

/// One line to paste into Konsole on a non-Rare-OS Bazzite install: writes both rule files to
/// /etc/udev/rules.d and reloads udev. Unplug and replug the device afterwards.
pub fn fix_command() -> String {
    format!("{} && sudo udevadm control --reload-rules && sudo udevadm trigger", write_both("/etc/udev/rules.d", "sudo "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn image_rules(name: &str) -> Option<String> {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../image/system_files/usr/lib/udev/rules.d").join(name);
        std::fs::read_to_string(p).ok()
    }

    #[test]
    fn rules_match_image_files() {
        // The image tree is absent when only launcher/ is copied (the image's build stage).
        if let Some(f) = image_rules(LEDGER_RULES_FILE) {
            assert_eq!(f, LEDGER_RULES);
        }
        if let Some(f) = image_rules(KEYSTONE_RULES_FILE) {
            assert_eq!(f, KEYSTONE_RULES);
        }
    }

    #[test]
    fn rules_have_uaccess_and_ids() {
        assert!(LEDGER_RULES.contains(r#"SUBSYSTEM=="usb", ATTRS{idVendor}=="2c97", TAG+="uaccess""#));
        assert!(LEDGER_RULES.contains(r#"SUBSYSTEM=="hidraw", ATTRS{idVendor}=="2c97", TAG+="uaccess""#));
        assert!(KEYSTONE_RULES.contains(r#"ATTRS{idVendor}=="1209", ATTRS{idProduct}=="3001", TAG+="uaccess""#));
        // uaccess only works from rules sorted before 73-seat-late.rules
        assert!(LEDGER_RULES_FILE < "73" && KEYSTONE_RULES_FILE < "73");
    }

    #[test]
    fn fix_command_is_one_pasteable_line() {
        let c = fix_command();
        assert!(!c.contains('\n'));
        assert!(!LEDGER_RULES.contains('\'') && !KEYSTONE_RULES.contains('\''));
        assert!(c.contains("sudo tee /etc/udev/rules.d/70-rare-ledger.rules >/dev/null"));
        assert!(c.contains("sudo tee /etc/udev/rules.d/70-rare-keystone.rules >/dev/null"));
        assert!(c.ends_with("sudo udevadm control --reload-rules && sudo udevadm trigger"));
    }

    #[cfg(unix)]
    #[test]
    fn fix_command_writes_exact_files() {
        // Same command without sudo, into a temp dir, through a real shell.
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("rules.d");
        let cmd = write_both(dir.to_str().unwrap(), "");
        let st = std::process::Command::new("sh").arg("-c").arg(&cmd).status().unwrap();
        assert!(st.success());
        assert_eq!(std::fs::read_to_string(dir.join(LEDGER_RULES_FILE)).unwrap(), bare(LEDGER_RULES));
        assert_eq!(std::fs::read_to_string(dir.join(KEYSTONE_RULES_FILE)).unwrap(), bare(KEYSTONE_RULES));
        assert!(fix_command().len() < 600, "fix command should stay readable on screen");
    }
}
