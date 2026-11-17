#!/usr/bin/env bash
# Rare OS VM tests. Run as root on a Linux host with podman and QEMU (CI runner or the build machine).
#
#   image/vm/test.sh fresh    install the image to a disk, boot it, wait for the launcher,
#                             then run game mode under headless gamescope (gamescope-focus.sh)
#   image/vm/test.sh rebase   install stock Fedora bootc, switch it to Rare OS with
#                             tools/get-rare-os.sh, boot Rare OS, roll back, boot the old OS
#   image/vm/test.sh handheld a Bazzite handheld reachable only over SSH, switched offline from an
#                             OCI archive (RARE_ARCHIVE), checked and rolled back (see handheld())
#   image/vm/test.sh update   install the previous published release (FROM_IMAGE), then update it
#                             the way a device does (Settings > System > Update now: os-admin
#                             update-now, os-update, bootc upgrade, signature policy) to the image
#                             under test, boot it, and time a power-off (update())
#   image/vm/test.sh bootloader install stock fedora-bootc:43 (GameOS's base), switch it to Rare OS, check
#                             that rare-bootloader-update puts Rare OS's shim and GRUB on the EFI
#                             partition once, and that Rare OS and the old OS both boot on them (bootloader())
#   image/vm/test.sh usbiso   the USB installer ISO (RARE_ISO) as an owner uses it: written to a USB
#                             stick, a blank (or Windows) NVMe drive, a USB drive and an SD card that
#                             must stay untouched; install, restart, boot without the stick (usbiso())
#
# Env: IMAGE (default localhost/rare-os:test), WORK (disks, default /var/tmp/rare-vm),
#      STORAGE (host container storage, default /var/lib/containers/storage),
#      BASE_IMAGE (stock OS for the rebase test), OUT (screenshots and logs),
#      SOURCE_IMGREF (install from another source, e.g. oci:/output/rare-oci:test after
#      `podman push $IMAGE oci:$WORK/rare-oci:test`; needed when the host's podman is older
#      than the image's and bootc can't read the host storage, as with Ubuntu 24.04's 4.9).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
IMAGE="${IMAGE:-localhost/rare-os:test}"
WORK="${WORK:-/var/tmp/rare-vm}"
STORAGE="${STORAGE:-/var/lib/containers/storage}"
OUT="${OUT:-$WORK/out}"
BASE_IMAGE="${BASE_IMAGE:-quay.io/fedora/fedora-bootc:44}"
export RARE_VM_STATE="$WORK/state"
export RARE_VM_KEY="$RARE_VM_STATE/id_ed25519"
H="python3 $HERE/harness.py"
mkdir -p "$WORK" "$OUT" "$RARE_VM_STATE"
[[ -f "$RARE_VM_KEY" ]] || ssh-keygen -q -t ed25519 -N "" -f "$RARE_VM_KEY"
cp "$RARE_VM_KEY.pub" "$WORK/id.pub"

KARGS=(--karg systemd.wants=sshd.service --karg console=tty0 --karg console=ttyS0,115200)
INSTALL_ARGS=()

# Rare OS starts with SSH off (rare-keep-ssh turns sshd off on the first start, as on a device), so
# the fresh and rebase tests reach the guest over a vsock instead: QEMU gets a vhost-vsock device and
# systemd's ssh generator in the guest answers on it (sshd-vsock.socket, only in a VM with a vsock),
# with root's key from the install (--root-ssh-authorized-keys) and as a systemd credential. Nothing
# in the image changes: on a device there is no vsock, and sshd stays off until the owner turns it on.
use_vsock() {
    [[ -e /dev/vhost-vsock ]] || modprobe vhost_vsock 2>/dev/null || true
    [[ -e /dev/vhost-vsock ]] || { echo "no /dev/vhost-vsock on this host (modprobe vhost_vsock): the VM tests need it" >&2; exit 1; }
    command -v socat >/dev/null || { echo "socat is missing on this host: the VM tests need it" >&2; exit 1; }
    export RARE_VM_VSOCK="${RARE_VM_VSOCK:-42}" RARE_VM_ROOT_KEY="$RARE_VM_KEY.pub"
}
# scp to the guest as root (over the vsock when it is set up). Args: local-file guest-path
to_guest() {
    local via=(-P "${RARE_VM_SSH_PORT:-2222}") host=root@127.0.0.1
    [[ -n "${RARE_VM_VSOCK:-}" ]] && { via=(-o "ProxyCommand=socat - VSOCK-CONNECT:$RARE_VM_VSOCK:22"); host=root@vm; }
    scp -i "$RARE_VM_KEY" "${via[@]}" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR "$1" "$host:$2"
}

install_disk() { # image disk-file size filesystem
    local img="$1" disk="$2" size="$3" fs="$4"
    rm -f "$disk"; truncate -s "$size" "$disk"
    podman run --rm --privileged --pid=host --security-opt label=type:unconfined_t \
        -v /dev:/dev -v "$STORAGE:/var/lib/containers/storage" -v "$WORK:/output" \
        "$img" bootc install to-disk ${SOURCE_IMGREF:+--source-imgref "$SOURCE_IMGREF"} "${INSTALL_ARGS[@]}" --via-loopback --generic-image --wipe --skip-fetch-check \
        --filesystem "$fs" --root-ssh-authorized-keys /output/id.pub "${KARGS[@]}" \
        "/output/$(basename "$disk")"
}

fresh() {
    use_vsock
    echo "::group::install Rare OS to a disk"
    install_disk "$IMAGE" "$WORK/rare.raw" 48G btrfs
    echo "::endgroup::"
    echo "::group::boot Rare OS"
    local ok=0
    $H boot --disk "$WORK/rare.raw" --name fresh --out "$OUT" --wait-launcher "${WAIT:-2700}" || ok=$?
    echo "::endgroup::"
    echo "::group::Rare OS game mode under gamescope (headless backend)"
    # The VM boots the launcher in cage (no real GPU); this runs it under gamescope --steam too.
    if [[ $ok == 0 ]]; then
        local gs=0
        # SSH stays off on the first start, as it does on a device (the harness came in over the vsock).
        $H ssh --cmd "echo state=\$(cat /var/lib/rare/ssh/state 2>/dev/null); echo sshd=\$(systemctl is-active sshd.service)" | tee "$OUT/fresh-ssh-off.txt"
        if grep -q '^state=off' "$OUT/fresh-ssh-off.txt" && grep -q '^sshd=inactive' "$OUT/fresh-ssh-off.txt"; then
            echo "CHECK SSH is off by default: PASS"
        else
            echo "CHECK SSH is off by default: FAIL"; ok=1
        fi
        to_guest "$HERE/gamescope-focus.sh" /root/gamescope-focus.sh
        $H ssh --timeout 1200 --cmd "bash /root/gamescope-focus.sh" | tee "$OUT/fresh-gamescope-focus.txt" || gs=$?
        $H ssh --cmd "cat /var/tmp/rare-gamescope-focus.log" > "$OUT/fresh-gamescope-session.log" 2>&1 || true
        case $gs in
            0) ;;
            77) echo "gamescope focus check SKIPPED in this VM (see above): not a pass" ;;
            *) ok=1 ;;
        esac
    fi
    $H stop
    cp "$RARE_VM_STATE"/serial-rare.raw.log "$OUT/fresh-serial.log" 2>/dev/null || true
    rm -f "$WORK/rare.raw"
    echo "::endgroup::"
    return $ok
}

rebase() {
    use_vsock
    echo "::group::stock OS"
    podman pull "$BASE_IMAGE" || { BASE_IMAGE=quay.io/fedora/fedora-bootc:43; podman pull "$BASE_IMAGE"; }
    # A stock bootc disk has no separate /boot, so systemd-gpt-auto mounts the ESP over /boot and
    # ostree can't finalize the staged switch at shutdown ("Remounting /boot read-write: Invalid
    # argument"); the old OS then boots again. Real devices (GameOS, Bazzite) have a /boot partition.
    KARGS+=(--karg systemd.gpt_auto=0)
    install_disk "$BASE_IMAGE" "$WORK/base.raw" 48G xfs
    $H boot --disk "$WORK/base.raw" --name rebase-0-stock --out "$OUT" --wait-ssh 1200
    $H ssh --cmd "bootc status; cat /etc/os-release | head -3"
    # Keep the journal across reboots, so a failed switch can be read from the next boot.
    $H ssh --cmd "mkdir -p /var/log/journal && systemd-tmpfiles --create --prefix /var/log/journal && journalctl --flush"
    echo "::endgroup::"

    echo "::group::serve Rare OS from a local registry"
    podman rm -f rare-registry >/dev/null 2>&1 || true
    mkdir -p "$WORK/registry"
    # Loopback only: the VM reaches it as 10.0.2.2, and nothing outside the host can.
    podman run -d --name rare-registry -p 127.0.0.1:5000:5000 -v "$WORK/registry:/var/lib/registry" docker.io/library/registry:2
    sleep 3
    podman push --tls-verify=false --compression-format zstd --compression-level 1 "$IMAGE" localhost:5000/rare-os:test
    # The registry has a copy now; free the local one (CI runners are tight on disk).
    if [[ "${FREE_AFTER_PUSH:-0}" == 1 ]]; then podman rmi -f "$IMAGE" || true; podman image prune -af || true; fi
    df -h "$WORK" || true
    echo "::endgroup::"

    echo "::group::switch with get-rare-os.sh"
    to_guest "$REPO/tools/get-rare-os.sh" /root/get-rare-os.sh
    # The phone route (rare.market/os, as a SuiPlay owner runs it): a systemd-run job with SELinux enforcing. bootc
    # then runs in the job's domain, not install_t as from a terminal, so it can't set labels the stock OS's policy
    # doesn't know unless the script handles it (GitHub issue 2, "fsetxattr(security.selinux): Invalid argument").
    $H ssh --cmd "getenforce"
    $H ssh --timeout 5400 --cmd "systemd-run --quiet --wait --pipe --collect --unit=rare-os-get -E RARE_IMAGE=10.0.2.2:5000/rare-os:test -E RARE_YES=1 -E RARE_INSECURE_REGISTRY=1 bash /root/get-rare-os.sh 2>&1 | tail -n 40"
    $H ssh --cmd "bootc status; echo SELinux: \$(getenforce)" | tee "$OUT/rebase-0-switch.txt"
    podman rm -f rare-registry; rm -rf "$WORK/registry"
    if ! grep -q 'SELinux: Enforcing' "$OUT/rebase-0-switch.txt"; then echo "SELinux is not enforcing again after the switch"; $H stop; return 1; fi
    echo "::endgroup::"

    echo "::group::boot Rare OS after the switch"
    local ok=0
    $H reboot --name rebase-1-rare --out "$OUT" --wait-launcher "${WAIT:-2700}" || ok=$?
    $H ssh --cmd "bootc status; id -nu 1000; cat /etc/os-release | head -3; journalctl -b -1 --no-pager -u ostree-finalize-staged | tail -n 15"
    echo "::endgroup::"

    echo "::group::undo (rare-undo)"
    $H ssh --cmd "rare-undo"
    $H reboot --name rebase-2-rollback --out "$OUT" --wait-ssh 1200 || ok=$?
    $H ssh --cmd "bootc status; grep '^NAME=' /etc/os-release" | tee "$OUT/rebase-2-rollback-check.txt"
    if grep -q '^NAME=.*Rare' "$OUT/rebase-2-rollback-check.txt"; then echo "rollback did not return to the stock OS"; ok=1; fi
    $H stop
    cp "$RARE_VM_STATE"/serial-base.raw.log "$OUT/rebase-serial.log" 2>/dev/null || true
    rm -f "$WORK/base.raw"
    echo "::endgroup::"
    return $ok
}

# Rehearses the offline switch of a Bazzite handheld that is reachable only over SSH:
# stock Bazzite with user `bazzite` (uid 1000, SSH key), sshd enabled by a symlink in /etc (no
# kernel argument), a Wi-Fi profile and an older game-mode autologin; then the switch from an
# OCI archive copied to /var/tmp (`bootc switch --transport oci-archive`), with SWITCH_BOOTC to
# use the device's own bootc binary. Checks (handheld.sh check-rare):
#   a  sshd enabled and active, firewall allows ssh, key login as `bazzite` from outside
#   b  Wi-Fi profile, authorized_keys and the user survive
#   c  game mode logs `bazzite` in by itself into the Rare session, no SDDM password prompt
#   d  Rare Launcher runs; Steam starts the way the launcher starts it
#   e  rolling back to the old OS works (by index, as there may be a newer Rare OS deployment)
#   f  the old OS stays pinned through the first start's own changes and a second boot
# Env: RARE_ARCHIVE (host path of the oci-archive), BASE_IMAGE (default bazzite-deck:stable),
#      SWITCH_BOOTC (host path of a bootc binary to run the switch with), HANDHELD_FROM=switch
#      (reuse the set-up stock disk a previous run left, and start at the switch).
handheld() {
    local archive="${RARE_ARCHIVE:?set RARE_ARCHIVE to the Rare OS oci-archive}"
    local bkey="$RARE_VM_STATE/bazzite_ed25519" port="${RARE_VM_SSH_PORT:-2222}" ok=0 res
    [[ -f "$bkey" ]] || ssh-keygen -q -t ed25519 -N "" -f "$bkey"
    local sshopt=(-i "$RARE_VM_KEY" -P "$port" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)
    as_bazzite() { ssh -i "$bkey" -p "$port" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o BatchMode=yes \
                       -o ConnectTimeout=10 -o LogLevel=ERROR bazzite@127.0.0.1 "$@"; }
    guest() { # name cmd: run in the guest, keep the output, count failures
        $H ssh --timeout 900 --cmd "$2" | tee "$OUT/$1.txt" || true
        grep -q '^RESULT fails=0' "$OUT/$1.txt" || ok=1
    }
    export RARE_VM_SMBIOS="${RARE_VM_SMBIOS:-type=1,manufacturer=Mysten Labs,, Inc.,product=SuiPlay0X1}"
    # The owner's rollback (docs/INSTALL.md): rare-undo where the image has it, else the same steps
    # by hand: stop a kernel-argument change still running, drop anything staged, then make the
    # first system that is not Rare OS the default.
    local rollback_cmd='if command -v rare-undo >/dev/null; then rare-undo; else rpm-ostree cancel; rpm-ostree cleanup -p && i=$(rpm-ostree status --json | jq "[.deployments[] | (.[\"container-image-reference\"] // \"\") | test(\"rare-os\") | not] | index(true)") && echo "old OS index $i" && ostree admin set-default "$i"; fi; rpm-ostree status'
    # First-boot kernel-argument changes run in the background; under TCG they take minutes, and a
    # reboot in the middle drops them (harmless, they run again), so wait for them before rebooting.
    ostree_idle() { $H ssh --timeout 3000 --cmd 'for i in $(seq 1 280); do rpm-ostree status | grep -q "^State: idle" && { echo "rpm-ostree idle"; exit 0; }; sleep 10; done; echo "rpm-ostree still busy"; exit 1' || true; }

    if [[ "${HANDHELD_FROM:-}" == switch && -f "$WORK/handheld.raw" ]]; then
        # Resume on the disk a previous run left (KEEP_DISK=1 or a failed switch): stock OS, set up.
        echo "::group::resume: stock Bazzite from the previous run"
        $H boot --disk "$WORK/handheld.raw" --name hh-1-resume --out "$OUT" --wait-ssh 2400
        scp "${sshopt[@]}" "$HERE/handheld.sh" root@127.0.0.1:/var/tmp/handheld.sh
        if $H ssh --cmd "grep -q '^NAME=\"Rare OS\"' /etc/os-release"; then
            # The earlier run left Rare OS booted: go back with the owner's rollback first (check e).
            ostree_idle
            $H ssh --cmd "$rollback_cmd"
            $H reboot --name hh-1-rollback --out "$OUT" --wait-ssh 2400 || ok=1
            guest hh-1-check "bash /var/tmp/handheld.sh check-base"
            grep -q '^INFO booted: .*Bazzite' "$OUT/hh-1-check.txt" && echo "CHECK e PASS: the owner's rollback returned to Bazzite" \
                || { echo "CHECK e FAIL: the owner's rollback did not return to Bazzite"; ok=1; }
            res="$(as_bazzite 'echo KEYLOGIN $(id -un)' || true)"; echo "$res" | tee -a "$OUT/hh-1-check.txt"
        fi
        $H ssh --cmd "bash /var/tmp/handheld.sh prep-sddm; cat /proc/cmdline"
        echo "::endgroup::"
    else
    echo "::group::stock Bazzite, set up like the owner's"
    KARGS+=(--karg systemd.gpt_auto=0)
    install_disk "${BASE_IMAGE}" "$WORK/handheld.raw" 80G btrfs
    $H boot --disk "$WORK/handheld.raw" --name hh-0-stock --out "$OUT" --wait-ssh 2400
    scp "${sshopt[@]}" "$HERE/handheld.sh" root@127.0.0.1:/var/tmp/handheld.sh
    scp "${sshopt[@]}" "$bkey.pub" root@127.0.0.1:/var/tmp/bazzite.pub
    $H ssh --cmd "bash /var/tmp/handheld.sh prep"
    # From here sshd runs only because it is enabled, as on the device.
    $H ssh --cmd "rpm-ostree kargs --delete=systemd.wants=sshd.service"
    $H reboot --name hh-1-stock --out "$OUT" --wait-ssh 2400 || ok=1
    guest hh-1-check "bash /var/tmp/handheld.sh check-base"
    as_bazzite 'echo KEYLOGIN $(id -un)' | tee -a "$OUT/hh-1-check.txt" || ok=1
    $H ssh --cmd "bash /var/tmp/handheld.sh prep-sddm; cat /proc/cmdline"
    echo "::endgroup::"
    fi

    echo "::group::switch from the OCI archive"
    # The archive may still be building; it appears here only when complete.
    while [[ ! -f "$archive" ]]; do sleep 30; done
    local have want; want="$(sha256sum "$archive" | cut -d' ' -f1)"
    have="$($H ssh --timeout 1800 --cmd "sha256sum /var/tmp/rare-os.ociarchive 2>/dev/null | cut -d' ' -f1" || true)"
    if [[ "${have//[[:space:]]/}" != "$want" ]]; then
        scp "${sshopt[@]}" "$archive" root@127.0.0.1:/var/tmp/rare-os.ociarchive
    fi
    local bootc=bootc
    if [[ -n "${SWITCH_BOOTC:-}" ]]; then
        scp "${sshopt[@]}" "$SWITCH_BOOTC" root@127.0.0.1:/var/tmp/bootc-switch
        # Same SELinux label as /usr/bin/bootc (install_exec_t), so it runs in install_t like the
        # device's `sudo bootc`; from /var/tmp it would stay unconfined and could not write the new
        # image's labels that the old policy does not know yet.
        bootc="/var/tmp/bootc-switch"; $H ssh --cmd "chmod +x $bootc; chcon --reference=/usr/bin/bootc $bootc; ls -Z $bootc; $bootc --version"
    fi
    $H ssh --timeout 10800 --cmd "time $bootc switch --transport oci-archive /var/tmp/rare-os.ociarchive 2>&1 | tail -n 30; echo SWITCH_EXIT=\${PIPESTATUS[0]}" | tee "$OUT/hh-switch.txt"
    grep -q 'SWITCH_EXIT=0' "$OUT/hh-switch.txt" || { echo "switch failed"; $H stop; return 1; }
    $H ssh --cmd "bootc status; rpm-ostree status"
    echo "::endgroup::"

    echo "::group::first Rare OS boot"
    $H reboot --name hh-2-rare --out "$OUT" --wait-launcher "${WAIT:-3600}" || ok=1
    guest hh-2-check "bash /var/tmp/handheld.sh check-rare"
    res="$(as_bazzite 'echo KEYLOGIN $(id -un); systemctl is-enabled sshd' || true)"; echo "$res" | tee -a "$OUT/hh-2-check.txt"
    [[ "$res" == *"KEYLOGIN bazzite"* ]] || { echo "CHECK a FAIL: key login as bazzite"; ok=1; }
    echo "::endgroup::"

    echo "::group::second Rare OS boot (old OS must stay)"
    ostree_idle
    $H ssh --cmd "rpm-ostree status"
    $H reboot --name hh-3-rare --out "$OUT" --wait-launcher "${WAIT:-3600}" || ok=1
    guest hh-3-check "bash /var/tmp/handheld.sh check-rare; cat /proc/cmdline"
    res="$(as_bazzite 'echo KEYLOGIN $(id -un)' || true)"; echo "$res" | tee -a "$OUT/hh-3-check.txt"
    [[ "$res" == *"KEYLOGIN bazzite"* ]] || { echo "CHECK a FAIL: key login as bazzite"; ok=1; }
    # (f) after a second boot (and any version the first start added), the old OS is still there and pinned.
    res="$($H ssh --cmd 'rpm-ostree status --json | jq -r ".deployments[] | [(.booted|tostring), (.pinned|tostring), (.[\"container-image-reference\"] // \"\")] | join(\" \")"' || true)"
    echo "$res" | tee -a "$OUT/hh-3-check.txt"
    if [[ "$res" == *"true false "*rare-os* && "$res" == *"false true "*bazzite* ]]; then
        echo "CHECK f PASS: Rare OS booted again; the old OS is still installed and pinned"
    else
        echo "CHECK f FAIL: expected Rare OS booted and the old OS pinned"; ok=1
    fi
    echo "::endgroup::"

    echo "::group::roll back to the old OS"
    $H ssh --cmd "$rollback_cmd"
    $H reboot --name hh-4-rollback --out "$OUT" --wait-ssh 2400 || ok=1
    guest hh-4-check "bash /var/tmp/handheld.sh check-base"
    grep -q '^INFO booted: .*Bazzite' "$OUT/hh-4-check.txt" && echo "CHECK e PASS: rolled back to Bazzite" || { echo "CHECK e FAIL: did not boot Bazzite"; ok=1; }
    res="$(as_bazzite 'echo KEYLOGIN $(id -un)' || true)"; echo "$res" | tee -a "$OUT/hh-4-check.txt"
    [[ "$res" == *"KEYLOGIN bazzite"* ]] || { echo "CHECK e FAIL: key login as bazzite after rollback"; ok=1; }
    $H stop
    cp "$RARE_VM_STATE"/serial-handheld.raw.log "$OUT/handheld-serial.log" 2>/dev/null || true
    [[ "${KEEP_DISK:-0}" == 1 ]] || rm -f "$WORK/handheld.raw"
    echo "::endgroup::"
    return $ok
}

# Secure Boot on, set up like a GameOS SuiPlay where it can be (GameOS's own image is not public):
# OVMF with Secure Boot and the Microsoft keys (RARE_VM_SECUREBOOT=1), a stock Fedora bootc system
# whose signed shim/GRUB/kernel start under it, user `playtron` (uid 1000, password playtron,
# wheel), /etc/os-release of GameOS (ID=playtronos) and GameOS's update config. Then the documented
# route: as `playtron`, `sudo bash get-rare-os.sh` (Rare OS from a local registry), which must queue
# the Universal Blue key with mokutil; the harness walks the blue MokManager screen with the keys
# the guide lists (Enroll MOK, Continue, Yes, password, Reboot); Rare OS must then start
# with Secure Boot still on, the key enrolled, and reach the launcher; rare-undo goes back.
# Env: RARE_ARCHIVE (oci-archive of Rare OS) or IMAGE (local image), BASE_IMAGE, MOK_WAIT (seconds
# from the restart until the key screen is up, default 120).
secureboot() {
    export RARE_VM_SECUREBOOT=1
    local ok=0 port="${RARE_VM_SSH_PORT:-2222}" pkey="$RARE_VM_STATE/playtron_ed25519"
    [[ -f "$pkey" ]] || ssh-keygen -q -t ed25519 -N "" -f "$pkey"
    local sshopt=(-i "$RARE_VM_KEY" -P "$port" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)
    as_playtron() { ssh -i "$pkey" -p "$port" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o BatchMode=yes \
                        -o ConnectTimeout=10 -o LogLevel=ERROR playtron@127.0.0.1 "$@"; }
    check() { # name pattern file
        if grep -qE "$2" "$3"; then echo "CHECK $1 PASS"; else echo "CHECK $1 FAIL (no /$2/ in $3)"; ok=1; fi
    }

    echo "::group::stock OS with Secure Boot on, set up like GameOS"
    podman pull "$BASE_IMAGE" || { BASE_IMAGE=quay.io/fedora/fedora-bootc:43; podman pull "$BASE_IMAGE"; }
    KARGS+=(--karg systemd.gpt_auto=0)
    rm -f "$RARE_VM_STATE/vars-sb.raw.fd"
    install_disk "$BASE_IMAGE" "$WORK/sb.raw" 48G xfs
    $H boot --disk "$WORK/sb.raw" --name sb-0-stock --out "$OUT" --wait-ssh 1200 || { echo "stock OS did not start under Secure Boot"; $H stop; return 1; }
    scp "${sshopt[@]}" "$pkey.pub" root@127.0.0.1:/var/tmp/playtron.pub
    scp "${sshopt[@]}" "$REPO/tools/get-rare-os.sh" root@127.0.0.1:/var/tmp/get-rare-os.sh
    $H ssh --cmd 'set -e
        mkdir -p /var/log/journal && systemd-tmpfiles --create --prefix /var/log/journal && journalctl --flush
        useradd -u 1000 -m -G wheel playtron && echo playtron:playtron | chpasswd
        install -d -m 700 -o playtron -g playtron /var/home/playtron/.ssh
        install -m 600 -o playtron -g playtron /var/tmp/playtron.pub /var/home/playtron/.ssh/authorized_keys
        rm /etc/os-release && printf "%s\n" "NAME=\"GameOS\"" "ID=\"playtronos\"" "ID_LIKE=\"fedora\"" "VERSION_ID=1.3.2" "PRETTY_NAME=\"GameOS 1.3.2\"" > /etc/os-release
        printf "%s\n" "BASE=ostree-unverified-registry:registry.playtron.one/public/playtronos:1.3.2" "PROJECT=public" "TAG=1.3.2" > /etc/playtron-os.conf
        mokutil --sb-state; mokutil --list-enrolled | grep -E "Subject:" || true' | tee "$OUT/sb-0-check.txt"
    check "0 stock OS starts with Secure Boot on" 'SecureBoot enabled' "$OUT/sb-0-check.txt"
    echo "::endgroup::"

    echo "::group::serve Rare OS from a local registry"
    podman rm -f rare-registry >/dev/null 2>&1 || true
    mkdir -p "$WORK/registry"
    podman run -d --name rare-registry -p 127.0.0.1:5000:5000 -v "$WORK/registry:/var/lib/registry" docker.io/library/registry:2
    sleep 3
    if [[ -n "${RARE_ARCHIVE:-}" ]]; then
        skopeo copy --dest-tls-verify=false "oci-archive:$RARE_ARCHIVE" docker://localhost:5000/rare-os:test
    else
        podman push --tls-verify=false --compression-format zstd --compression-level 1 "$IMAGE" localhost:5000/rare-os:test
    fi
    echo "::endgroup::"

    echo "::group::the documented route: as playtron, sudo bash get-rare-os.sh"
    as_playtron "id; echo playtron | sudo -S -p '' env RARE_IMAGE=10.0.2.2:5000/rare-os:test RARE_YES=1 RARE_INSECURE_REGISTRY=1 bash /var/tmp/get-rare-os.sh 2>&1 | tail -n 60" | tee "$OUT/sb-1-get.txt"
    podman rm -f rare-registry; rm -rf "$WORK/registry"
    check "1 script sees Secure Boot and queues the key" 'Start-up key queued' "$OUT/sb-1-get.txt"
    check "1 script prints the key screen steps" 'Enroll MOK' "$OUT/sb-1-get.txt"
    # Rare OS starts with SSH off after a switch (keep-ssh) and never lets root in; the test needs
    # SSH, so it records the choice an owner makes in Settings > System (SSH on) beforehand, and
    # from here logs in as playtron with a key (sudo with the password).
    $H ssh --cmd 'mokutil --list-new | grep -E "Subject:|CN=" ; bootc status | head -20
        rm -f /etc/os-release && ln -s ../usr/lib/os-release /etc/os-release
        mkdir -p /var/lib/rare/ssh && echo on > /var/lib/rare/ssh/state' | tee "$OUT/sb-1-check.txt"
    check "1 key request pending" 'ublue kernel' "$OUT/sb-1-check.txt"
    echo "::endgroup::"

    echo "::group::restart: blue key screen, enrolled with the guide's keys"
    $H ssh --timeout 15 --cmd "systemctl reboot" || true
    export RARE_VM_SSH_USER=playtron RARE_VM_KEY="$pkey"
    sleep "${MOK_WAIT:-120}"
    $H shot --out "$OUT/sb-2-mok-0.png" || true
    # MokManager opens straight on its menu (mokutil --timeout -1: no countdown screen).
    # Enroll MOK; Continue; Yes; password; Reboot. (Enter on the first item, Continue boot, skips
    # it: GRUB then says "bad shim signature" for Rare OS's kernel and shows its menu.)
    $H keys --out "$OUT" --name sb-2-mok --gap 3 --keys "down ret down ret down ret text:${RARE_MOK_PASSWORD:-rare} ret"
    $H shot --out "$OUT/sb-2-mok-9-after-password.png" || true
    $H keys --out "$OUT" --name sb-2-mok-reboot --gap 3 --keys "ret"
    $H wait --name sb-3-rare --out "$OUT" --wait-launcher "${WAIT:-2700}" || ok=1
    $H ssh --cmd 'mokutil --sb-state; echo playtron | sudo -S -p "" mokutil --test-key /etc/pki/akmods/certs/akmods-ublue.der; uname -r; grep ^NAME= /etc/os-release; cat /sys/kernel/security/lockdown; id -nu 1000; systemctl --failed --no-pager' | tee "$OUT/sb-3-check.txt"
    check "3 Rare OS started with Secure Boot on" 'SecureBoot enabled' "$OUT/sb-3-check.txt"
    check "3 key enrolled" 'already enrolled' "$OUT/sb-3-check.txt"
    check "3 booted Rare OS" '^NAME=.*Rare' "$OUT/sb-3-check.txt"
    echo "::endgroup::"

    echo "::group::undo (rare-undo)"
    $H ssh --cmd 'echo playtron | sudo -S -p "" rare-undo'
    $H ssh --timeout 15 --cmd 'echo playtron | sudo -S -p "" systemctl reboot' || true
    sleep 20
    export RARE_VM_SSH_USER=root RARE_VM_KEY="$RARE_VM_STATE/id_ed25519"
    $H wait --name sb-4-rollback --out "$OUT" --wait-ssh 1200 || ok=1
    $H ssh --cmd "mokutil --sb-state; grep '^NAME=' /etc/os-release" | tee "$OUT/sb-4-check.txt"
    check "4 back on the old OS with Secure Boot on" 'NAME="?Fedora' "$OUT/sb-4-check.txt"
    $H stop
    cp "$RARE_VM_STATE"/serial-sb.raw.log "$OUT/sb-serial.log" 2>/dev/null || true
    [[ "${KEEP_DISK:-0}" == 1 ]] || rm -f "$WORK/sb.raw"
    echo "::endgroup::"
    return $ok
}

# The USB installer as an owner uses it. RARE_ISO is the released rare-os-installer.iso (no podman
# needed). UEFI, the ISO on a USB stick (usb-storage on xHCI, not a CD drive), a 64 GB NVMe internal
# drive, a 256 GB USB drive and a 128 GB SD card (both bigger than the NVMe drive, with a file
# system and a file on them). Checks:
#   disk   the installer picks the NVMe drive; the USB drive and the SD card are byte-for-byte unchanged
#   boot1  after the install it restarts by itself, with the stick still in, into Rare OS
#   boot2  powered off, stick out, powered on: the launcher starts
#   update bootc follows the signed $REPO:stable (the first start moves it to ostree-image-signed,
#          the signature checks against /etc/pki/containers/rare-os.pub) and `bootc upgrade --check` works
# ISO_DISK=windows: the NVMe drive holds a Windows-style GPT first (EFI, MSR, NTFS, recovery) and the
# stick is picked from the boot menu (it boots first); afterwards no Windows partition may be left.
# RARE_VM_SECUREBOOT=1: Secure Boot on with the Microsoft keys; the installer queues Rare OS's key
# and the harness walks the blue key screen (password rare). ISO_NAME names the screenshots.
usbiso() {
    use_vsock
    local iso="${RARE_ISO:?set RARE_ISO to rare-os-installer.iso}" name="${ISO_NAME:-usbiso}" ok=0 t0 n
    local nvme="$WORK/$name-nvme.raw" usb="$WORK/$name-usbdrive.raw" sd="$WORK/$name-sdcard.raw"
    local repo="${ISO_REPO:-ghcr.io/rare-market/rare-os}"
    export RARE_VM_DISK_BUS=nvme RARE_VM_ISO_BUS=usb RARE_VM_RES="${RARE_VM_RES:-1920x1200}"
    export RARE_VM_USB_DISK="$usb" RARE_VM_SD="$sd"
    check() { if [[ "$2" == 1 ]]; then echo "CHECK $1: PASS"; else echo "CHECK $1: FAIL"; ok=1; fi; }
    mark() { # disk size label: GPT, one FAT32 partition with a file, then remember its bytes
        local f="$1" lo
        rm -f "$f"; truncate -s "$2" "$f"
        sgdisk -n 1:1MiB:+2GiB -t 1:0700 "$f" >/dev/null  # (-q drops a new table in gdisk 1.0.8)
        lo="$(attach "$f")"
        mkfs.vfat -n "$3" "${lo}p1" >/dev/null
        mkdir -p "$WORK/mnt"; mount "${lo}p1" "$WORK/mnt"; echo "$3 $(date -u +%s)" > "$WORK/mnt/KEEP-ME.txt"; umount "$WORK/mnt"
        losetup -d "$lo"
    }
    attach() { # loop device with its partitions (the table is written to the file first)
        local lo; lo="$(losetup -fP --show "$1")"
        for _ in $(seq 1 20); do [[ -e "${lo}p1" ]] && break; partx -u "$lo" 2>/dev/null || true; udevadm settle 2>/dev/null || true; sleep 0.5; done
        [[ -e "${lo}p1" ]] || { echo "no partitions on $lo" >&2; losetup -d "$lo"; return 1; }
        echo "$lo"
    }
    sums() { # the regions anything writing a partition table or a file system touches
        local f="$1" sz; sz=$(stat -c %s "$f")
        { dd if="$f" bs=1M count=2100 status=none; dd if="$f" bs=1M skip=$(( sz / 1048576 - 64 )) status=none; } | sha256sum | cut -d' ' -f1
    }

    echo "::group::disks"
    rm -f "$RARE_VM_STATE"/vars-*.fd
    mark "$usb" 256G USBDRIVE; mark "$sd" 128G SDCARD
    local usb0 sd0; usb0=$(sums "$usb"); sd0=$(sums "$sd")
    rm -f "$nvme"; truncate -s 64G "$nvme"
    if [[ "${ISO_DISK:-blank}" == windows ]]; then
        local lo
        sgdisk -n 1:1MiB:+100MiB -t 1:ef00 -c 1:"EFI system partition" -n 2:0:+16MiB -t 2:0c01 -c 2:"Microsoft reserved partition" \
            -n 3:0:-1GiB -t 3:0700 -c 3:"Basic data partition" -n 4:0:0 -t 4:2700 -c 4:"Basic data partition" "$nvme" >/dev/null
        lo="$(attach "$nvme")"
        mkfs.vfat -F 32 -n SYSTEM "${lo}p1" >/dev/null
        mmd -i "${lo}p1" ::/EFI ::/EFI/Microsoft ::/EFI/Microsoft/Boot
        echo "not a real loader" > "$WORK/bootmgfw.efi"; mcopy -i "${lo}p1" "$WORK/bootmgfw.efi" ::/EFI/Microsoft/Boot/bootmgfw.efi
        mkfs.ntfs -Q -q -L Windows "${lo}p3"; mkfs.ntfs -Q -q -L Recovery "${lo}p4"
        sgdisk -p "$lo"; blkid "$lo"* || true
        losetup -d "$lo"
        export RARE_VM_ISO_FIRST=1
    fi
    echo "::endgroup::"

    echo "::group::install from the stick"
    export RARE_VM_ISO="$iso"
    # Secure Boot: the key screen comes at the restart, so the harness takes over there (stick out).
    [[ "${RARE_VM_SECUREBOOT:-0}" == 1 ]] && export RARE_VM_ISO_FIRST=1
    # With the stick picked from the boot menu, the installer's own restart would start it again
    # (QEMU's boot order outranks the firmware's): QEMU stops at the restart instead.
    [[ "${RARE_VM_ISO_FIRST:-0}" == 1 ]] && export RARE_VM_NO_REBOOT=1
    $H stop
    python3 -c "import sys; sys.path.insert(0, '$HERE'); import harness; harness.start('$nvme')"
    t0=$(date +%s); n=0
    local installed=0
    while :; do
        n=$((n + 1)); $H shot --out "$OUT/$name-1-install-$(printf %03d $n).png" >/dev/null 2>&1 || true
        if ! python3 -c "import sys; sys.path.insert(0, '$HERE'); import harness; sys.exit(0 if harness.running() else 1)"; then
            echo "QEMU stopped at the installer's restart after $(( $(date +%s) - t0 )) s"; installed=2; break
        fi
        if (( n % 6 == 0 )) && timeout 15 $H ssh --timeout 12 --cmd 'grep -q "^NAME=\"Rare OS\"" /etc/os-release && ! test -d /run/install && echo INSTALLED' 2>/dev/null | grep -q INSTALLED; then
            echo "installed, restarted into Rare OS with the stick in, after $(( $(date +%s) - t0 )) s"; installed=1; break
        fi
        (( $(date +%s) - t0 > ${ISO_TIMEOUT:-3600} )) && { echo "install timed out"; break; }
        sleep 5
    done
    unset RARE_VM_NO_REBOOT
    echo "::endgroup::"
    [[ $installed != 0 ]] || { check "install finished" 0; $H stop; return 1; }

    if [[ $installed == 1 ]]; then
        echo "::group::first start (stick still in)"
        $H wait --name "$name-2-boot1" --out "$OUT" --wait-launcher "${WAIT:-1800}" || ok=1
        echo "::endgroup::"
    else
        echo "::group::first start (stick out)"
        unset RARE_VM_ISO RARE_VM_ISO_FIRST
        python3 -c "import sys; sys.path.insert(0, '$HERE'); import harness; harness.start('$nvme')"
        if [[ "${RARE_VM_SECUREBOOT:-0}" == 1 ]]; then mok_walk "$name"; fi
        $H wait --name "$name-2-boot1" --out "$OUT" --wait-launcher "${WAIT:-1800}" || ok=1
        echo "::endgroup::"
    fi
    if [[ $installed == 1 && "${RARE_VM_SECUREBOOT:-0}" == 1 ]]; then echo "(Secure Boot: the key screen came before this start)"; fi

    echo "::group::the drive the installer picked, and the disks it must not touch"
    $H ssh --cmd 'lsblk -o NAME,TRAN,RM,SIZE,FSTYPE,LABEL,PARTLABEL,MOUNTPOINTS; echo; sgdisk -p /dev/nvme0n1; echo; blkid; echo; efibootmgr -v 2>&1; mokutil --sb-state 2>&1; true' | tee "$OUT/$name-3-disks.txt"
    check "installed on the NVMe drive (btrfs rare-os)" "$(grep -qE 'nvme0n1p[0-9].*btrfs.*rare-os' "$OUT/$name-3-disks.txt" && echo 1)"
    check "no Windows partition left (NTFS, MSR)" "$(grep -qiE 'ntfs|Microsoft reserved|Basic data' "$OUT/$name-3-disks.txt" || echo 1)"
    echo "::endgroup::"

    echo "::group::signed updates (rare-signed-switch, a minute after boot)"
    $H ssh --timeout 1800 --cmd 'for i in $(seq 1 120); do rpm-ostree status --json | jq -e "[.deployments[] | .[\"container-image-reference\"] // \"\" | startswith(\"ostree-image-signed:\")] | any" >/dev/null && rpm-ostree status | grep -q "^State: idle" && break; sleep 10; done; journalctl -b --no-pager -u rare-signed-switch | tail -n 20; bootc status' | tee "$OUT/$name-4-signed-switch.txt"
    check "the first start moved updates to the signed source" "$(grep -q "ostree-image-signed:docker://$repo:stable" "$OUT/$name-4-signed-switch.txt" && echo 1)"
    echo "::endgroup::"

    echo "::group::power off, stick out, power on"
    $H ssh --timeout 30 --cmd 'systemctl poweroff' || true
    for _ in $(seq 1 120); do python3 -c "import sys; sys.path.insert(0, '$HERE'); import harness; sys.exit(0 if harness.running() else 1)" || break; sleep 2; done
    $H stop
    check "USB drive untouched" "$([[ "$(sums "$usb")" == "$usb0" ]] && echo 1)"
    check "SD card untouched" "$([[ "$(sums "$sd")" == "$sd0" ]] && echo 1)"
    unset RARE_VM_ISO RARE_VM_ISO_FIRST
    python3 -c "import sys; sys.path.insert(0, '$HERE'); import harness; harness.start('$nvme')"
    local up=0
    $H wait --name "$name-5-boot2" --out "$OUT" --wait-launcher "${WAIT:-1800}" && up=1
    check "launcher starts without the stick" "$up"
    $H ssh --timeout 900 --cmd "bootc status; echo; bootc status --json | jq -r '.status.booted.image.image.image, .status.booted.image.imageDigest, .status.booted.image.image.signature // empty'; echo; bootc upgrade --check 2>&1 | tail -n 5; echo UPGRADE_CHECK_EXIT=\${PIPESTATUS[0]}; grep -E '^(NAME|VERSION|RARE_VERSION)=' /usr/lib/os-release; systemctl --failed --no-pager" | tee "$OUT/$name-6-update.txt"
    check "booted image is the signed $repo:stable" "$(grep -q "ostree-image-signed:docker://$repo:stable" "$OUT/$name-6-update.txt" && echo 1)"
    check "bootc upgrade --check works (signature and registry)" "$(grep -q 'UPGRADE_CHECK_EXIT=0' "$OUT/$name-6-update.txt" && echo 1)"
    $H stop
    cp "$RARE_VM_STATE/serial-$(basename "$nvme").log" "$OUT/$name-serial.log" 2>/dev/null || true
    [[ "${KEEP_DISK:-0}" == 1 ]] || rm -f "$nvme" "$usb" "$sd"
    echo "::endgroup::"
    return $ok
}

# The blue key screen (shim's MokManager) at the first start with Secure Boot on, walked with the
# keys the installer's screen and docs/INSTALL.md list: Enroll MOK, Continue, Yes, password, Reboot.
# A device on the previous release takes the new one the way Settings > System > Update now does it:
# pkexec os-admin update-now (polkit org.rare.os.admin for the launcher's session), os-update --now
# in rare-update-now.service, `bootc upgrade --check` then `bootc upgrade` with the device's own
# bootc and containers signature policy, the staged deployment finalized at shutdown, and the new
# version booted. 0.1.3 was the first image after the rechunk (128 new zstd layers, a manifest
# without a mediaType field), and no test had updated an installed release before.
#
#   FROM_IMAGE   the release devices run now (default ghcr.io/rare-market/rare-os:0.1.2), installed
#                from the registry with its update source set to ghcr.io/rare-market/rare-os:stable,
#                signature-checked (ostree-image-signed:, what rare-signed-switch leaves on a device)
#   UPDATE_TO    local (default): the image under test ($IMAGE), pushed zstd to a local registry as
#                the publish step pushes it and signed with a throwaway cosign key; the guest trusts
#                that key for that registry only, with the same sigstoreSigned policy as ghcr.io.
#                published: the device's own source as it is, ghcr.io/rare-market/rare-os:stable,
#                checked with the image's own key (proves a release that is already out).
#   COSIGN       a cosign v2 binary (default: v2.4.3 downloaded and checked, as image.yml does)
# Checks: the launcher's process may run os-admin (pkcheck), a power-off during the download ends
# within POWEROFF_MAX seconds and the download goes on after it, the update stages the expected
# digest, a power-off with it staged (ostree-finalize-staged installs it on the way down) ends
# within POWEROFF_MAX, the device boots it, and a power-off on the new version ends in time too.
update() {
    use_vsock
    local from="${FROM_IMAGE:-ghcr.io/rare-market/rare-os:0.1.2}" to="${UPDATE_TO:-local}" ok=0
    local poweroff_max="${POWEROFF_MAX:-90}" want policy_sh ck="$RARE_VM_STATE/cosign-test"
    check() { if eval "$2"; then echo "CHECK $1: PASS"; else echo "CHECK $1: FAIL"; ok=1; fi; }
    # Seconds until the guest has powered off and QEMU has exited, after `systemctl poweroff`.
    poweroff_secs() {
        local pid t0 n=0; pid="$(cat "$RARE_VM_STATE/qemu.pid")"; t0=$(date +%s)
        $H ssh --timeout 15 --cmd "systemctl poweroff" >/dev/null 2>&1 || true
        while kill -0 "$pid" 2>/dev/null && (( n < 900 )); do sleep 1; n=$((n + 1)); done
        kill -0 "$pid" 2>/dev/null && { $H stop; echo 9999; return; }
        rm -f "$RARE_VM_STATE/qmp.sock" "$RARE_VM_STATE/qemu.pid"
        echo $(( $(date +%s) - t0 ))
    }
    # The update state as the launcher reads it, until the run is over (no fresh heartbeat).
    wait_update() { # timeout-secs
        $H ssh --timeout "$(( $1 + 60 ))" --cmd "for i in \$(seq 1 $(( $1 / 10 ))); do
            s=\$(cat /var/lib/rare/update-state.json 2>/dev/null); ph=\$(jq -r '.phase // empty' <<<\"\$s\")
            if ! systemctl -q is-active rare-update-now.service rare-update.service && [[ \$ph != checking && \$ph != downloading ]]; then break; fi
            sleep 10; done; jq -c 'del(.notes)' /var/lib/rare/update-state.json"
    }

    echo "::group::install the previous release ($from)"
    local reg=10.0.2.2:5000/rare-os:test
    [[ "$to" == published ]] && reg=ghcr.io/rare-market/rare-os:stable
    # The update source a device on that release has. rare-signed-switch would move an unsigned
    # source to the signed one with `rpm-ostree rebase` (a pull of its own), so the test sets the
    # signed source itself and keeps that unit from running.
    KARGS+=(--karg systemd.mask=rare-signed-switch.service --karg systemd.mask=rare-signed-switch.timer)
    INSTALL_ARGS=(--target-imgref "$reg")
    SOURCE_IMGREF="registry:$from" install_disk "$from" "$WORK/update.raw" 64G btrfs
    INSTALL_ARGS=()
    # The installer image is not needed any more (CI runners are tight on disk).
    [[ "${KEEP_FROM:-0}" == 1 ]] || podman rmi -f "$from" >/dev/null 2>&1 || true
    echo "::endgroup::"

    if [[ "$to" == local ]]; then
        echo "::group::serve the image under test from a local registry, signed with a throwaway key"
        local cosign="${COSIGN:-$WORK/cosign}"
        if [[ ! -x "$cosign" ]]; then
            curl -fsSLo "$cosign" https://github.com/sigstore/cosign/releases/download/v2.4.3/cosign-linux-amd64
            echo "caaad125acef1cb81d58dcdc454a1e429d09a750d1e9e2b3ed1aed8964454708  $cosign" | sha256sum -c --quiet && chmod +x "$cosign"
        fi
        rm -f "$ck".key "$ck".pub
        (cd "$RARE_VM_STATE" && COSIGN_PASSWORD="" "$cosign" generate-key-pair --output-key-prefix cosign-test >/dev/null)
        podman rm -f rare-registry >/dev/null 2>&1 || true
        mkdir -p "$WORK/registry"
        podman run -d --name rare-registry -p 127.0.0.1:5000:5000 -v "$WORK/registry:/var/lib/registry" docker.io/library/registry:2
        sleep 3
        # As the publish step pushes it: zstd, the manifest podman writes.
        podman push --tls-verify=false --compression-format zstd --digestfile "$WORK/update-digest" "$IMAGE" localhost:5000/rare-os:test
        want="$(cat "$WORK/update-digest")"
        COSIGN_PASSWORD="" "$cosign" sign -y --tlog-upload=false --allow-insecure-registry --key "$ck.key" "localhost:5000/rare-os@$want"
        if [[ "${FREE_AFTER_PUSH:-0}" == 1 ]]; then podman rmi -f "$IMAGE" || true; podman image prune -af || true; fi
        # The guest reaches the registry as 10.0.2.2; the signature names localhost:5000/rare-os, the name it was
        # pushed under (exactRepository: that repository, any tag or digest).
        policy_sh='install -m 0644 /var/tmp/rare-os-test.pub /etc/pki/containers/rare-os-test.pub
            jq ".transports.docker[\"10.0.2.2:5000/rare-os\"] = [{\"type\": \"sigstoreSigned\", \"keyPath\": \"/etc/pki/containers/rare-os-test.pub\",
                \"signedIdentity\": {\"type\": \"exactRepository\", \"dockerRepository\": \"localhost:5000/rare-os\"}}]" \
                /etc/containers/policy.json > /etc/containers/policy.json.new && mv /etc/containers/policy.json.new /etc/containers/policy.json
            printf "docker:\n  10.0.2.2:5000/rare-os:\n    use-sigstore-attachments: true\n" > /etc/containers/registries.d/rare-os-test.yaml
            printf "[[registry]]\nlocation = \"10.0.2.2:5000\"\ninsecure = true\n" > /etc/containers/registries.conf.d/99-rare-os-test.conf'
        echo "::endgroup::"
    else
        want="$(skopeo inspect --raw "docker://$reg" | sha256sum | cut -d' ' -f1)"; want="sha256:$want"
        policy_sh=true
    fi
    echo "update to: $reg ($want)"

    echo "::group::boot the previous release"
    $H boot --disk "$WORK/update.raw" --name update-0-from --out "$OUT" --wait-launcher "${WAIT:-2700}" || ok=1
    # The signed source a device has (rare-signed-switch's ostree-image-signed:docker://...).
    [[ "$to" == local ]] && to_guest "$ck.pub" /var/tmp/rare-os-test.pub
    $H ssh --cmd "$policy_sh
        # /sysroot is read-only; bootc rewrites the booted deployment's origin in a mount namespace of its own.
        rpm-ostree status --booted | grep -q 'ostree-image-signed:docker://$reg' ||
            unshare -m sh -c 'mount -o remount,rw /sysroot && bootc switch --mutate-in-place --enforce-container-sigpolicy $reg'
        systemctl restart rpm-ostreed; rpm-ostree status --booted; grep -E '^(VERSION|IMAGE_ID)=' /etc/os-release" | tee "$OUT/update-0-from.txt"
    check "the previous release runs with the signed update source" "grep -q 'ostree-image-signed:docker://$reg' '$OUT/update-0-from.txt'"
    # pkexec in the launcher asks polkit about the launcher's process: allowed without a password.
    $H ssh --cmd 'p=$(pgrep -u 1000 -o -f rare-launcher); echo "launcher pid $p"; pkcheck --action-id org.rare.os.admin --process "$p" && echo POLKIT=yes || echo POLKIT=no' | tee "$OUT/update-polkit.txt"
    check "the launcher may run os-admin" "grep -q POLKIT=yes '$OUT/update-polkit.txt'"
    echo "::endgroup::"

    echo "::group::Update now, then power off during the download"
    $H ssh --cmd "/usr/libexec/rare/os-admin update-now; echo ADMIN_EXIT=\$?" | tee "$OUT/update-1-now.txt"
    check "os-admin update-now starts the update" "grep -q ADMIN_EXIT=0 '$OUT/update-1-now.txt'"
    $H ssh --timeout 900 --cmd 'for i in $(seq 1 90); do b=$(jq -r ".bytes // 0" /var/lib/rare/update-state.json 2>/dev/null); ph=$(jq -r ".phase // empty" /var/lib/rare/update-state.json 2>/dev/null); [[ $ph == downloading && $b -gt 0 ]] && break; [[ $ph == failed ]] && break; sleep 5; done; jq -c "del(.notes)" /var/lib/rare/update-state.json' | tee "$OUT/update-1-downloading.txt"
    if grep -q '"phase":"downloading"' "$OUT/update-1-downloading.txt"; then
        local secs; secs="$(poweroff_secs)"
        echo "power-off during the download took ${secs} s"
        check "power-off during the download ends within ${poweroff_max} s" "(( $secs <= $poweroff_max ))"
        $H boot --disk "$WORK/update.raw" --name update-2-after-poweroff --out "$OUT" --wait-launcher "${WAIT:-2700}" || ok=1
        $H ssh --cmd "/usr/libexec/rare/os-admin update-now; echo ADMIN_EXIT=\$?"
    else
        check "the download starts" false
    fi
    echo "::endgroup::"

    echo "::group::the download finishes and stages"
    wait_update 5400 | tee "$OUT/update-3-state.txt"
    $H ssh --cmd "journalctl -b --no-pager -t rare-update | tail -n 30; rpm-ostree status --json | jq -c '[.deployments[] | {staged, booted, version, digest: .[\"container-image-reference-digest\"], ref: .[\"container-image-reference\"]}]'" | tee "$OUT/update-3-status.txt"
    check "the update is staged" "grep -q '\"result\":\"staged\"' '$OUT/update-3-state.txt'"
    check "the staged digest is $want" "grep -q '\"staged\":true,[^}]*\"digest\":\"$want\"' '$OUT/update-3-status.txt'"
    echo "::endgroup::"
    if ! grep -q '"result":"staged"' "$OUT/update-3-state.txt"; then
        $H stop; [[ "$to" == local ]] && { podman rm -f rare-registry >/dev/null 2>&1; rm -rf "$WORK/registry"; }
        return 1
    fi

    echo "::group::Turn off with the update staged (it installs on the way down), then start"
    local secs; secs="$(poweroff_secs)"
    echo "power-off with the update staged took ${secs} s"
    check "power-off with the update staged ends within ${poweroff_max} s" "(( $secs <= $poweroff_max ))"
    $H boot --disk "$WORK/update.raw" --name update-4-to --out "$OUT" --wait-launcher "${WAIT:-2700}" || ok=1
    $H ssh --cmd "rpm-ostree status --booted --json | jq -c '.deployments[0] | {version, digest: .[\"container-image-reference-digest\"]}'; grep -E '^(VERSION|IMAGE_ID)=' /etc/os-release; systemctl --failed --no-pager --plain | head -20" | tee "$OUT/update-4-to.txt"
    check "the device runs $want" "grep -q '\"digest\":\"$want\"' '$OUT/update-4-to.txt'"
    secs="$(poweroff_secs)"
    echo "power-off after the update took ${secs} s"
    check "power-off ends within ${poweroff_max} s" "(( $secs <= $poweroff_max ))"
    echo "::endgroup::"
    cp "$RARE_VM_STATE"/serial-update.raw.log "$OUT/update-serial.log" 2>/dev/null || true
    [[ "$to" == local ]] && { podman rm -f rare-registry >/dev/null 2>&1; rm -rf "$WORK/registry"; }
    [[ "${KEEP_DISK:-0}" == 1 ]] || rm -f "$WORK/update.raw"
    return $ok
}

# A device switched in place from GameOS 43 (fedora-bootc:43 based) keeps GameOS's shim and GRUB on
# its EFI partition, since bootc never touches it; rare-bootloader-update puts Rare OS's there once.
# Stock fedora-bootc:43 on a disk, switched with get-rare-os.sh, booted into Rare OS; then:
#   a  rare-bootloader-update (the image's unit, or the repo's script with BL_SCRIPT=1) leaves the
#      ESP's shim and GRUB equal to the image's payload, and their PCR4 to the image's
#   b  a second run changes nothing
#   c  Rare OS boots again on the new boot loader
#   d  rare-undo: the old OS (fedora-bootc:43) boots on the new boot loader, and its own
#      bootloader-update.service does not move the ESP back
# Env: BL_BASE (default quay.io/fedora/fedora-bootc:43), BL_NO_STATE=1 (delete
#      /boot/bootupd-state.json first, as on an install bootupd never recorded: the adopt path),
#      BL_SWITCH_IMAGE (switch straight to this registry image, e.g. ghcr.io/rare-market/rare-os:stable;
#      default: IMAGE through a local registry), BL_SCRIPT=1 (for an image without the unit).
bootloader() {
    use_vsock
    local base="${BL_BASE:-quay.io/fedora/fedora-bootc:43}" ok=0 target
    pass() { echo "CHECK $1: PASS"; }
    failc() { echo "CHECK $1: FAIL"; ok=1; }
    echo "::group::stock OS ($base)"
    podman pull "$base"
    KARGS+=(--karg systemd.gpt_auto=0)  # as in rebase(): a stock bootc disk has no separate /boot
    install_disk "$base" "$WORK/bl.raw" 40G xfs
    $H boot --disk "$WORK/bl.raw" --name bl-0-stock --out "$OUT" --wait-ssh 1200
    to_guest "$HERE/esp.sh" /var/tmp/esp.sh
    to_guest "$REPO/tools/boot-pcr4.py" /var/tmp/boot-pcr4.py
    $H ssh --cmd "mkdir -p /var/log/journal && systemd-tmpfiles --create --prefix /var/log/journal && journalctl --flush; bootupctl status; bash /var/tmp/esp.sh" | tee "$OUT/bl-0-stock.txt"
    if [[ "${BL_NO_STATE:-0}" == 1 ]]; then $H ssh --cmd "mount -o remount,rw /boot 2>/dev/null; rm -v /boot/bootupd-state.json"; fi
    echo "::endgroup::"

    echo "::group::switch with get-rare-os.sh"
    target="${BL_SWITCH_IMAGE:-}"
    local reg=()
    if [[ -z "$target" ]]; then
        podman rm -f rare-registry >/dev/null 2>&1 || true
        mkdir -p "$WORK/registry"
        podman run -d --name rare-registry -p 127.0.0.1:5000:5000 -v "$WORK/registry:/var/lib/registry" docker.io/library/registry:2
        sleep 3
        podman push --tls-verify=false --compression-format zstd --compression-level 1 "$IMAGE" localhost:5000/rare-os:test
        target=10.0.2.2:5000/rare-os:test reg=(-E RARE_INSECURE_REGISTRY=1)
    fi
    to_guest "$REPO/tools/get-rare-os.sh" /root/get-rare-os.sh
    $H ssh --timeout 5400 --cmd "systemd-run --quiet --wait --pipe --collect --unit=rare-os-get -E RARE_IMAGE=$target -E RARE_YES=1 ${reg[*]} bash /root/get-rare-os.sh 2>&1 | tail -n 20"
    [[ -n "${BL_SWITCH_IMAGE:-}" ]] || { podman rm -f rare-registry; rm -rf "$WORK/registry"; }
    echo "::endgroup::"

    echo "::group::Rare OS: the boot loader update"
    $H reboot --name bl-1-rare --out "$OUT" --wait-ssh 1800 || ok=1
    to_guest "$HERE/esp.sh" /var/tmp/esp.sh
    to_guest "$REPO/tools/boot-pcr4.py" /var/tmp/boot-pcr4.py
    local run="systemctl start rare-bootloader-update.service; systemctl status --no-pager rare-bootloader-update.service | tail -n 3"
    if [[ "${BL_SCRIPT:-0}" == 1 ]]; then
        to_guest "$REPO/image/system_files/usr/libexec/rare/bootloader-update" /var/tmp/bootloader-update
        # The unit's sandbox, around the repo's script.
        run="systemd-run --quiet --wait --pipe --collect --unit=rare-bootloader-update-test -p PrivateNetwork=yes -p ProtectHome=yes -p PrivateMounts=yes -p KillMode=mixed bash /var/tmp/bootloader-update; echo exit=\$?"
    fi
    $H ssh --cmd "grep ^PRETTY_NAME /etc/os-release; bash /var/tmp/esp.sh; ls -la /boot/bootupd-state.json; systemctl is-enabled bootloader-update.service rare-bootloader-update.timer; journalctl -b --no-pager -u bootloader-update.service | tail -n 5" | tee "$OUT/bl-1-before.txt"
    $H ssh --timeout 900 --cmd "$run; journalctl -b --no-pager -t bootloader-update -t rare-bootloader-update -u 'rare-bootloader-update*' | tail -n 40; cat /var/lib/rare/bootloader/done; bash /var/tmp/esp.sh" | tee "$OUT/bl-1-update.txt"
    grep -q '^ESP matches image: yes' "$OUT/bl-1-update.txt" && grep -q '^how=' "$OUT/bl-1-update.txt" \
        && [[ "$(awk '/^PCR4 esp/ { print $3 }' "$OUT/bl-1-update.txt")" == "$(awk '/^PCR4 image/ { print $3 }' "$OUT/bl-1-update.txt")" ]] \
        && pass "a ESP has the image's shim and GRUB ($(awk '/^PCR4 esp/ { print $3 }' "$OUT/bl-1-update.txt"))" || failc "a ESP has the image's shim and GRUB"
    $H ssh --cmd "n() { journalctl -b --no-pager | grep -c 'rare-bootloader: updating'; }; a=\$(md5sum </var/lib/rare/bootloader/done) j=\$(n); $run; [[ \$a == \$(md5sum </var/lib/rare/bootloader/done) && \$j == \$(n) ]] && echo 'second run: nothing done'" | tee "$OUT/bl-1-again.txt"
    grep -q '^second run: nothing done' "$OUT/bl-1-again.txt" && ! grep -q 'rare-bootloader: updating' "$OUT/bl-1-again.txt" \
        && pass "b a second run is a no-op" || failc "b a second run is a no-op"
    echo "::endgroup::"

    echo "::group::Rare OS boots on the new boot loader"
    $H reboot --name bl-2-rare --out "$OUT" --wait-ssh 1800 || ok=1
    $H ssh --cmd "grep ^PRETTY_NAME /etc/os-release; bash /var/tmp/esp.sh" | tee "$OUT/bl-2.txt"
    grep -q '^PRETTY_NAME=.*Rare' "$OUT/bl-2.txt" && grep -q '^ESP matches image: yes' "$OUT/bl-2.txt" && pass "c Rare OS boots" || failc "c Rare OS boots"
    echo "::endgroup::"

    echo "::group::rare-undo: the old OS on the new boot loader"
    $H ssh --cmd "rare-undo"
    $H reboot --name bl-3-undo --out "$OUT" --wait-ssh 1200 || ok=1
    $H ssh --cmd "grep ^PRETTY_NAME /etc/os-release; journalctl -b --no-pager -u bootloader-update.service | tail -n 3; bash /var/tmp/esp.sh" | tee "$OUT/bl-3-undo.txt"
    if grep -q '^PRETTY_NAME=.*Rare' "$OUT/bl-3-undo.txt" || ! grep -q '^PRETTY_NAME=' "$OUT/bl-3-undo.txt"; then failc "d the old OS boots after rare-undo"
    elif [[ "$(awk '/^PCR4 esp/ { print $3 }' "$OUT/bl-3-undo.txt")" == "$(awk '/^PCR4 esp/ { print $3 }' "$OUT/bl-1-update.txt")" ]]; then pass "d the old OS boots after rare-undo, ESP unchanged"
    else failc "d the old OS's bootloader-update moved the ESP"; fi
    $H stop
    [[ "${KEEP_DISK:-0}" == 1 ]] || rm -f "$WORK/bl.raw"
    echo "::endgroup::"
    return $ok
}

mok_walk() {
    sleep "${MOK_WAIT:-40}"
    $H shot --out "$OUT/$1-mok-0.png" || true
    $H keys --out "$OUT" --name "$1-mok" --gap 3 --keys "down ret down ret down ret text:${RARE_MOK_PASSWORD:-rare} ret"
    $H keys --out "$OUT" --name "$1-mok-reboot" --gap 3 --keys "ret"
}

case "${1:-}" in
    fresh) fresh ;;
    rebase) rebase ;;
    handheld) handheld ;;
    secureboot) secureboot ;;
    usbiso) usbiso ;;
    update) update ;;
    bootloader) bootloader ;;
    *) echo "usage: $0 fresh|rebase|handheld|secureboot|usbiso|update|bootloader" >&2; exit 2 ;;
esac
