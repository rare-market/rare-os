#!/usr/bin/env bash
# The first-install walk: a new owner's whole first hour in a VM, with a pad and no keyboard.
# Run as root on a Linux host with KVM, QEMU, OVMF and socat (the build machine).
#
#   first-install.sh iso ARCHIVE OUT      build the installer ISO from an oci-archive of the image
#                                        (podman + bootc-image-builder with image/installer/config.toml,
#                                        then image/installer/finish-iso.sh); root's walk key reaches the
#                                        installed system as a systemd credential, so SSH works over vsock
#   first-install.sh install ISO [RES]   blank 64 GB disk, boot the ISO with the pad, time the install
#                                        until the installed system's first screen (RES 1920x1200)
#   first-install.sh boot [RES]          boot the installed disk (pad, vsock), wait for the launcher
#   first-install.sh pad STEP...         press on the virtual pad (vpad.py steps)
#   first-install.sh step NAME STEP...   press and time the answer (walk.py step)
#   first-install.sh ssh CMD             run in the guest over vsock (works with its network off)
#   first-install.sh net on|off          plug or unplug the guest's network cable
#   first-install.sh launcher DIR        walk a launcher build before it ships: DIR's
#                                        rare-launcher-native over the image's (a /usr overlay that is
#                                        gone at the next boot), game mode in the VM runs the native
#                                        launcher, and the session starts again
#   first-install.sh stop | clean        stop the VM | stop and delete the disk and the pad
#
# Env: WORK (default /var/tmp/rare-walk), OUT (shots and timing, default $WORK/out).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
WORK="${WORK:-/var/tmp/rare-walk}"
export RARE_VM_STATE="$WORK/state" RARE_VM_KEY="$WORK/state/id_ed25519" RARE_VM_VSOCK="${RARE_VM_VSOCK:-42}"
export RARE_VM_ROOT_KEY="$RARE_VM_KEY.pub" RARE_VM_MEM="${RARE_VM_MEM:-16384}" RARE_VM_CPUS="${RARE_VM_CPUS:-8}"
export WALK_OUT="${OUT:-$WORK/out}" WALK_PAD="$WORK/pad.sock"
DISK="$WORK/rare.qcow2"
H="python3 $HERE/harness.py"
mkdir -p "$RARE_VM_STATE" "$WALK_OUT"
[[ -f "$RARE_VM_KEY" ]] || ssh-keygen -q -t ed25519 -N "" -C rare-walk -f "$RARE_VM_KEY"

pad_up() {
    if ! { [[ -S "$WALK_PAD" ]] && [[ -e "$(head -1 "$WORK/pad.dev" 2>/dev/null)" ]]; }; then
        rm -f "$WALK_PAD"
        nohup python3 "$HERE/vpad.py" serve "$WALK_PAD" > "$WORK/pad.dev" 2> "$WORK/pad.log" &
        for _ in $(seq 1 50); do [[ -S "$WALK_PAD" ]] && break; sleep 0.1; done
    fi
    export RARE_VM_PAD; RARE_VM_PAD="$(head -1 "$WORK/pad.dev")"
    echo "pad $RARE_VM_PAD"
}

case "${1:-}" in
    iso)
        archive="$2" out="$3"
        mkdir -p "$out"
        # the shipped config as it is (the walk reaches the guest with a systemd credential instead)
        cfg="$out/config.toml"
        cp "$REPO/image/installer/config.toml" "$cfg"
        podman load -i "$archive" | tee "$out/load.txt"
        img="$(sed -n 's/^Loaded image: //p' "$out/load.txt" | tail -1)"
        # The installed system follows the ref the ISO is built from (bib's %post bootc switch), so
        # build from the one devices follow, as a real ISO must.
        ref="${WALK_REF:-ghcr.io/rare-market/rare-os:stable}"
        podman tag "$img" "$ref"
        podman run --rm --privileged --pull=newer --security-opt label=type:unconfined_t \
            -v "$cfg:/config.toml:ro" -v "$out:/output" -v "$(podman info --format '{{.Store.GraphRoot}}'):/var/lib/containers/storage" \
            quay.io/centos-bootc/bootc-image-builder:latest --type anaconda-iso --rootfs btrfs --use-librepo=True "$ref"
        bash "$REPO/image/installer/finish-iso.sh" "$out/bootiso/install.iso" "$out/rare-os-installer.iso"
        rm -f "$out/bootiso/install.iso"
        ;;
    install)
        iso="$2"; export RARE_VM_RES="${3:-1920x1200}" RARE_VM_ISO="$iso"
        rm -f "$DISK" "$RARE_VM_STATE"/vars-*.fd; qemu-img create -q -f qcow2 "$DISK" 64G
        pad_up
        t0=$(date +%s)
        $H stop; python3 -c "import sys; sys.path.insert(0, '$HERE'); import harness; harness.start('$DISK')"
        n=0
        # The installer reboots by itself; the ISO stays in the drive but the disk boots first after it.
        while :; do
            n=$((n + 1)); python3 "$HERE/walk.py" shot "install-$(printf %03d $n)" >/dev/null || true
            echo "install-$(printf %03d $n) $(( $(date +%s) - t0 ))" >> "$WALK_OUT/install-times.txt"
            # The installer's own sshd answers on the vsock too (and refuses the key): ask only now and then.
            if (( n % 10 == 0 )) && timeout 12 $H ssh --timeout 10 --cmd 'grep -q "^NAME=\"Rare OS\"" /etc/os-release && ! test -d /run/install && echo INSTALLED' 2>/dev/null | grep -q INSTALLED; then break; fi
            (( $(date +%s) - t0 > 3600 )) && { echo "install timed out" >&2; exit 1; }
            sleep 2
        done
        echo "installed and booted after $(( $(date +%s) - t0 )) s"
        ;;
    boot)
        export RARE_VM_RES="${2:-1920x1200}"
        pad_up
        $H stop; python3 -c "import sys; sys.path.insert(0, '$HERE'); import harness; harness.start('$DISK')"
        python3 "$HERE/walk.py" until boot 600 --quiet 8
        ;;
    pad) shift; python3 "$HERE/vpad.py" send "$WALK_PAD" "$@" ;;
    step) shift; python3 "$HERE/walk.py" step "$@" ;;
    shot) python3 "$HERE/walk.py" shot "$2" ;;
    ssh) shift; $H ssh --cmd "$*" ;;
    net) python3 -c "import sys; sys.path.insert(0, '$HERE'); import harness; harness.qmp().cmd('set_link', name='n0', up=('$2' == 'on'))" ;;
    launcher)
        dir="$2"
        scp -q -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -i "$RARE_VM_KEY" \
            -o "ProxyCommand=socat - VSOCK-CONNECT:$RARE_VM_VSOCK:22" "$dir/rare-launcher-native" root@vm:/var/tmp/rare-launcher-native
        $H ssh --cmd 'test -w /usr/libexec/rare/native || rpm-ostree usroverlay >/dev/null 2>&1 || ostree admin unlock >/dev/null
            install -m 0755 /var/tmp/rare-launcher-native /usr/libexec/rare/native/rare-launcher-native
            sed -i "s|exec cage -s -d -- /usr/bin/rare-launcher$|exec cage -s -d -- /usr/libexec/rare/rare-launcher-native-run|" /usr/bin/rare-session
            rm -f /run/user/1000/rare-launcher-bin; systemctl restart sddm; echo "launcher $(sha256sum /usr/libexec/rare/native/rare-launcher-native | cut -c1-12)"'
        ;;
    stop) $H stop ;;
    clean)
        $H stop
        python3 "$HERE/vpad.py" send "$WALK_PAD" quit 2>/dev/null || true
        rm -f "$DISK" "$WORK/pad.dev" "$RARE_VM_STATE"/vars-*.fd
        ;;
    *) sed -n '2,20p' "$0"; exit 2 ;;
esac
