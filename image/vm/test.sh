#!/usr/bin/env bash
# Rare OS VM tests. Run as root on a Linux host with podman and QEMU (CI runner or the build machine).
#
#   image/vm/test.sh fresh    install the image to a disk, boot it, wait for the launcher,
#                             then run game mode under headless gamescope (gamescope-focus.sh)
#   image/vm/test.sh rebase   install stock Fedora bootc, switch it to Rare OS with
#                             tools/get-rare-os.sh, boot Rare OS, roll back, boot the old OS
#   image/vm/test.sh handheld a Bazzite handheld reachable only over SSH, switched offline from an
#                             OCI archive (RARE_ARCHIVE), checked and rolled back (see handheld())
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

install_disk() { # image disk-file size filesystem
    local img="$1" disk="$2" size="$3" fs="$4"
    rm -f "$disk"; truncate -s "$size" "$disk"
    podman run --rm --privileged --pid=host --security-opt label=type:unconfined_t \
        -v /dev:/dev -v "$STORAGE:/var/lib/containers/storage" -v "$WORK:/output" \
        "$img" bootc install to-disk ${SOURCE_IMGREF:+--source-imgref "$SOURCE_IMGREF"} --via-loopback --generic-image --wipe --skip-fetch-check \
        --filesystem "$fs" --root-ssh-authorized-keys /output/id.pub "${KARGS[@]}" \
        "/output/$(basename "$disk")"
}

fresh() {
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
        scp -i "$RARE_VM_KEY" -P "${RARE_VM_SSH_PORT:-2222}" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
            "$HERE/gamescope-focus.sh" root@127.0.0.1:/root/gamescope-focus.sh
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
    scp -i "$RARE_VM_KEY" -P "${RARE_VM_SSH_PORT:-2222}" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        "$REPO/tools/get-rare-os.sh" root@127.0.0.1:/root/get-rare-os.sh
    $H ssh --timeout 5400 --cmd "RARE_IMAGE=10.0.2.2:5000/rare-os:test RARE_YES=1 RARE_INSECURE_REGISTRY=1 bash /root/get-rare-os.sh 2>&1 | tail -n 40"
    $H ssh --cmd "bootc status"
    podman rm -f rare-registry; rm -rf "$WORK/registry"
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

case "${1:-}" in
    fresh) fresh ;;
    rebase) rebase ;;
    handheld) handheld ;;
    secureboot) secureboot ;;
    *) echo "usage: $0 fresh|rebase|handheld|secureboot" >&2; exit 2 ;;
esac
