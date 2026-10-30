//! USB security keys show up as /dev/hidraw* with the FIDO usage page (0xF1D0). The CTAP library
//! silently skips devices it cannot open, so we look ourselves: a FIDO hidraw node that exists
//! but cannot be opened means a missing udev rule, and the UI shows the one-line fix.

/// Usage Page (0xF1D0) as a 2-byte HID short item: 0x06 0xD0 0xF1.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn is_fido_descriptor(desc: &[u8]) -> bool {
    desc.windows(3).any(|w| w == [0x06, 0xD0, 0xF1])
}

/// FIDO hidraw nodes this user cannot open (empty when all are fine or none exist).
#[cfg(target_os = "linux")]
pub fn blocked_fido_nodes() -> Vec<String> {
    let mut out = vec![];
    let Ok(dir) = std::fs::read_dir("/sys/class/hidraw") else { return out };
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        // the report descriptor in sysfs is world-readable, unlike the device node
        let Ok(desc) = std::fs::read(e.path().join("device/report_descriptor")) else { continue };
        if !is_fido_descriptor(&desc) {
            continue;
        }
        let node = format!("/dev/{name}");
        if let Err(err) = std::fs::OpenOptions::new().read(true).write(true).open(&node) {
            if err.kind() == std::io::ErrorKind::PermissionDenied {
                out.push(node);
            }
        }
    }
    out
}

#[cfg(not(target_os = "linux"))]
pub fn blocked_fido_nodes() -> Vec<String> {
    vec![]
}

/// The one-line fix for security key access. systemd 252+ tags FIDO keys for the logged-in user
/// on its own; this rule does the same on older systems.
pub fn udev_fix() -> String {
    r#"echo 'KERNEL=="hidraw*", SUBSYSTEM=="hidraw", ENV{ID_FIDO_TOKEN}=="1", TAG+="uaccess"' | sudo tee /etc/udev/rules.d/70-rare-fido.rules && sudo udevadm control --reload-rules && sudo udevadm trigger"#.to_string()
}
