#!/usr/bin/env bash
# Get Rare OS: switches this device to Rare OS in place, over the network.
#
#   curl -fsSL https://rare.market/os/get | sudo bash
#
# Works on systems that boot from a container image: Playtron GameOS, Fedora Atomic
# (Kinoite, Silverblue) and other bootc / rpm-ostree systems.
# Nothing is erased: your home folder, Steam library, installed games, Wi-Fi and other
# settings stay. The current system stays installed as the fallback.
# Undo at any time:  sudo rare-undo  and reboot (or pick the old system in the boot menu).
#
# Options (environment):
#   RARE_IMAGE=<registry>/<name>:stable          image to switch to (default: the published image,
#                                                @RARE_IMAGE_REPO@ is filled in when this script is published)
#   RARE_YES=1                                   do not ask for confirmation
#   RARE_REBOOT=1                                reboot when done
#   RARE_IGNORE_BATTERY=1                        skip the battery check
#   RARE_INSECURE_REGISTRY=1                     allow a plain-HTTP registry (testing only)
#   RARE_MOK_PASSWORD=<text>                     one-time password for the Secure Boot key screen
#                                                (default: rare)
#
# Secure Boot: Rare OS's kernel is signed with Universal Blue's key (Rare OS is built on Bazzite),
# not Fedora's. With Secure Boot on, that key must be added once (a "MOK", machine owner key),
# so this script queues it with mokutil and the device shows a blue key screen at the next start.
# A SuiPlay0X1 that came with GameOS has Secure Boot off (GameOS can't start with it on), so it
# never sees that screen. The key is also in Rare OS at /etc/pki/akmods/certs/akmods-ublue.der.
#
# A log of every run is kept in /var/log/rare-os-get.log.
# RARE_GET_V2
set -euo pipefail

IMAGE="${RARE_IMAGE:-${RARE_IMAGE_REPO:-@RARE_IMAGE_REPO@}:stable}"
[[ $IMAGE != @* ]] || { echo "Set RARE_IMAGE (this copy of the script was not published with an image)." >&2; exit 1; }
GUIDE="https://rare.market/os"
NEED_GB=20
MIN_BATTERY=30
STEPS=5
LOG=/var/log/rare-os-get.log
REG="${IMAGE%%/*}"
REF="${IMAGE#*/}"
REPO="${REF%%[:@]*}"
TAG="${REF#"$REPO"}"; TAG="${TAG#[:@]}"; TAG="${TAG:-latest}"
MOK_PASSWORD="${RARE_MOK_PASSWORD:-rare}"

# Universal Blue's kernel signing certificate ("ublue kernel", valid to 2124). Rare OS's kernel
# carries this signature; the same file is /etc/pki/akmods/certs/akmods-ublue.der in Rare OS and
# Bazzite, and secure_boot.der in github.com/ublue-os/bazzite. The copy from the downloaded Rare OS
# is used when it can be found; this one is the fallback.
UBLUE_KEY_SHA256=4e5c68474cb133fd8984d9599762cece9100c3e6cd8a9709aeaabd85dd9e70d1
UBLUE_KEY_B64='
MIIFtjCCA56gAwIBAgIUffh69d7nONn6wvijghk3S+ChgKcwDQYJKoZIhvcNAQELBQAwdTEXMBUGA1UECgwOVW5pdmVyc2FsIEJs
dWUxFzAVBgNVBAsMDmtlcm5lbCBzaWduaW5nMRUwEwYDVQQDDAx1Ymx1ZSBrZXJuZWwxKjAoBgkqhkiG9w0BCQEWG3NlY3VyaXR5
QHVuaXZlcnNhbC1ibHVlLm9yZzAgFw0yNDA3MTEwMzA3MDlaGA8yMTI0MDYxNzAzMDcwOVowdTEXMBUGA1UECgwOVW5pdmVyc2Fs
IEJsdWUxFzAVBgNVBAsMDmtlcm5lbCBzaWduaW5nMRUwEwYDVQQDDAx1Ymx1ZSBrZXJuZWwxKjAoBgkqhkiG9w0BCQEWG3NlY3Vy
aXR5QHVuaXZlcnNhbC1ibHVlLm9yZzCCAiIwDQYJKoZIhvcNAQEBBQADggIPADCCAgoCggIBAJrjC/NmzXRnUWisoRu8vUKr8jq7
QqlYWVSdT2jvsuA9qyQ/GKBg5A7kBV+XpKJCV1M7QkiUvQ7nn2pAYZAjWbRABBcoHlN1eFg7wTVP1C+wV5mdsXO/fBs896GCRcI2
Na+HJQ3o2m8PdCWGzgOYJCe9zHIwdbm6mPjgw56pIc50c7OHqU3qFsFZTXrJkq/cvyyr+Ue6j4JcJERm1IVMktRJ6nZ7NmmWjYTG
SlBQl9YIT2DktSzihxM9f23R1RujdFmy6I6pMwSB3F4ehhFWlgvZa1/AGaXg7dE06RwT8A5U+fb3iV/V66JaisiL8rISvmY7a5LM
RMGRNlVF54JeF08N/w/ylOQb6cSE6H6Ug60hn2sbFy0S7rHQKXb8ZA5Y59na4EfLRnGGJQDWT+znsBTkJ730GvrbWzA1HfzwazeH
QXSzyWf3h+d7tRM9DwjGz3RZ4YNQuttD3NcBBo/x8QZtOjbJGmPNg3k1OQ6m0nSBCf7lbtD9KMoGfAf2hF0y+Pw43AARhJyhrzzI
vhUYXtj/jPn+qQa7x+h/LBqdRVXqYRTnDddqM8OW8/m2oNRWAAqfrCxfNZjD5x1/1trzuw4flLM4xMxEele8x0d1rLoAfUIWU0Fj
WIJF1mCtrpXKNtfSpAyXylxzGTADGPAW+JrALu5/nVZGq8aBtQJOSlp7AgMBAAGjPDA6MAwGA1UdEwEB/wQCMAAwCwYDVR0PBAQD
AgeAMB0GA1UdDgQWBBQsJQYVWLUCDEsNnKVgYuAMbNsEajANBgkqhkiG9w0BAQsFAAOCAgEAUp+arzHK6qsFtCL++BSJCManvz9t
b5IwiqQXyQgh2XmOcALh/JN63evdc2/ECdBBD9qO/0H+a8u59A9C4EqghKFtTsb50vbMgJe/naAxuWU98oT9eqnAMnHJnr4FWY5r
GP3hbxbt51h5nUmcX5dpgOfFmu3bPpPYii9Ky/wN/KGAQBB7eOErbwRZHVFBtlKdN9Vz1UH0LVa1LyPyz8F60Yfjz0waQxm7T0Id
x74yCJbb1PX/1s71FHSOqSlFFycYsN5bBbS8DPTThxjcQjMpRHR+hhW6vPVflphRExGtBY3FP/rOZbWS+LlEJlWmkBD8WPMbt71f
DSpf3R9St7NJu6KtuiCAbrGpkGmhKYQOVVNNO5Uz127UlC7Llt7KUh+ftfIVPGuFyBAhV5jW2T+5FbT9fARITlD33TaAXgzALkch
z1+koiYhxW3SPVdjbkzowae5V+D9yp7kemY3yBT269D7BOwlPAzr2ncSbm6v54s59Wx60rOKq087FakK33YDPdd7guqwm5lQWiEM
WIS5MHsNDcqeaisz3KPe5KEaP2BEkBuVdZGBLwelO8SCJDoQH3ULK3mhL0nu7LfehYBEdj0ZaBIO6V2ej+Cx1uR7Cf4PKlLja/IA
ymEj6a8OJMN6+0pfX3u8DaO51gzKIn1Aumx5L76B64rp7LVWRpnwGPs=
'

# ------------------------------------------------------------------ output

if [[ -t 1 ]]; then
    B=$'\033[1m' D=$'\033[2m' R=$'\033[0m'
    BLUE=$'\033[1;38;2;92;118;255m' OK=$'\033[1;32m' BAD=$'\033[1;31m' WARN=$'\033[1;33m'
else
    B= D= R= BLUE= OK= BAD= WARN=
fi

step=0
step()  { step=$((step + 1)); printf '\n%s[%d/%d]%s %s%s%s\n' "$BLUE" "$step" "$STEPS" "$R" "$B" "$*" "$R"; }
ok()    { printf '  %s✓%s %s\n' "$OK" "$R" "$*"; }
note()  { printf '  %s%s%s\n' "$D" "$*" "$R"; }
warn()  { printf '  %s!%s %s\n' "$WARN" "$R" "$*"; }
fail()  {
    printf '  %s✗%s %s\n' "$BAD" "$R" "$1" >&2
    shift
    for l in "$@"; do printf '    %s\n' "$l" >&2; done
    printf '\n  Nothing was changed. Help and the full guide: %s\n\n' "$GUIDE" >&2
    exit 1
}

undo_text() {
    cat <<EOF
  Your current system stays installed next to Rare OS. To go back to it:
    - from Rare OS: open a terminal, run  ${B}sudo rare-undo${R}  and restart, or
    - with a USB-C keyboard: hold ${B}Shift${R} (or tap ${B}Esc${R}) while powering on and pick
      the older entry in the boot menu.
  Your files and games are the same in both systems.
EOF
}

qr() {
    # https://rare.market/os
    cat <<'EOF'
    █████████████████████████████
    ██ ▄▄▄▄▄ █ ██▀▀ ▄▄ █ ▄▄▄▄▄ ██
    ██ █   █ █  ▀█ ▀ █ █ █   █ ██
    ██ █▄▄▄█ █▀  █▄ █▄▄█ █▄▄▄█ ██
    ██▄▄▄▄▄▄▄█▄█ ▀▄█▄█ █▄▄▄▄▄▄▄██
    ██▄▄▀▀▄▄▄▀▀█▄█▄█ ▀ ██▄▀ ▄ ▄██
    ██▄▄▄▀▀█▄▄  ▄█▀▄  ▀▄██▄▄ ▀███
    ██ █ ▀▀ ▄▄ ▀▀▄▀█▀▀▀ ▄█▀▄▄ ▄██
    ████▄█ ▀▄██▀▄ ▄▄  █ ▀  ▄ ▀███
    ██▄▄▄▄▄█▄▄ ▄ █▄▀█▄ ▄▄▄ ▄▄████
    ██ ▄▄▄▄▄ █▀▄▀█▀▄▀█ █▄█ ▀▀▀▀██
    ██ █   █ █▄▀ ▄▀▄ █    ▄▄   ██
    ██ █▄▄▄█ █▀█  ▄ ▄▀█▄▄▀▀ ▀ ███
    ██▄▄▄▄▄▄▄█▄▄██▄▄███▄▄███▄▄▄██
    █████████████████████████████
EOF
}

ask() {
    [[ "${RARE_YES:-0}" == 1 ]] && return 0
    local a=""
    if [[ -t 0 ]]; then
        read -r -p "  $1 [y/N] " a
    elif [[ -r /dev/tty ]]; then
        read -r -p "  $1 [y/N] " a </dev/tty
    else
        fail "No terminal to ask for confirmation." "Run it again with RARE_YES=1 to continue without asking."
    fi
    [[ "$a" == [yY]* ]]
}

# ------------------------------------------------------------------ Secure Boot

# on | off. Off also covers firmware in setup mode (no platform key: nothing is checked) and
# machines without UEFI.
sb_state() {
    if command -v mokutil >/dev/null; then
        case "$(mokutil --sb-state 2>&1)" in
            *"SecureBoot enabled"*) echo on; return ;;
            *"SecureBoot disabled"*|*"not supported"*|*"doesn't support"*) echo off; return ;;
        esac
    fi
    local v=/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c
    if [[ -r $v && "$(od -An -tu1 -j4 -N1 "$v" 2>/dev/null | tr -d ' ')" == 1 ]]; then echo on; else echo off; fi
}

# Writes the key Rare OS's kernel is signed with to $1: from the downloaded (staged) Rare OS when
# it can be found, else the copy in this script. Prints where it came from.
write_key() {
    local out="$1" csum f
    # The first deployment listed is the one the next start uses: the staged Rare OS.
    csum="$(ostree admin status 2>/dev/null | awk '{for (i = 1; i <= NF; i++) if ($i ~ /^[0-9a-f]+\.[0-9]+$/ && length($i) > 64) {print $i; exit}}')"
    if [[ -n "$csum" ]]; then
        for f in /ostree/deploy/*/deploy/"$csum"/usr/etc/pki/akmods/certs/akmods-ublue.der \
                 /ostree/deploy/*/deploy/"$csum"/etc/pki/akmods/certs/akmods-ublue.der; do
            if [[ -s "$f" ]] && openssl x509 -inform der -in "$f" -noout 2>/dev/null; then
                cp "$f" "$out"; echo "Rare OS"; return 0
            fi
        done
    fi
    printf '%s' "$UBLUE_KEY_B64" | base64 -d > "$out"
    [[ "$(sha256sum "$out" | cut -d' ' -f1)" == "$UBLUE_KEY_SHA256" ]] || return 1
    echo "built in"
}

# enrolled | pending | missing
key_state() {
    local o; o="$(mokutil --test-key "$1" 2>&1 || true)"
    case "$o" in
        *"already in the enrollment request"*) echo pending ;;
        *"already enrolled"*) echo enrolled ;;
        *) echo missing ;;
    esac
}

key_screen_text() {
    cat <<EOF
  ${B}The blue key screen at the next start${R}
  Secure Boot is on, so the device asks once to trust Rare OS's start-up key.
  It needs a ${B}USB-C keyboard${R} plugged into the device: the screen is driven with the arrow
  keys and Enter, and a password has to be typed. Plug it in before you restart.

    1. "Perform MOK management"   press Down once to ${B}Enroll MOK${R}, press Enter
                                  (not "Continue boot": that skips it)
    2. "[Enroll MOK]"             pick ${B}Continue${R}, press Enter
    3. "Enroll the key(s)?"       pick ${B}Yes${R}, press Enter
    4. "Password:"                type ${B}${MOK_PASSWORD}${R} (nothing shows as you type), press Enter
    5. "Perform MOK management"   pick ${B}Reboot${R}, press Enter

  Rare OS then starts. This happens only once.
  Skipped it? The next screen says "bad shim signature" and shows a menu: pick the older entry
  (the one that is not Rare OS) to start as before, then run this command again.
  No keyboard? Turn Secure Boot off instead (firmware settings: Security > Secure Boot >
  Disabled, then Save), then restart.
EOF
}

# ------------------------------------------------------------------ start

printf '\n%sRare OS%s  %sswitch this device to Rare OS%s\n\n' "$BLUE" "$R" "$D" "$R"
printf '  The full guide, with pictures, is at %s%s%s.\n  Scan to open it on your phone:\n\n' "$B" "$GUIDE" "$R"
qr
printf '\n  What happens: five quick checks, then Rare OS downloads (about 8 GB) next to your\n'
printf '  current system. Nothing changes until you restart, and you can always go back.\n'

[[ $EUID -eq 0 ]] || fail "This needs administrator rights." "Run it with sudo:  curl -fsSL $GUIDE/get | sudo bash"

# Keep a log for support from here on (output still shows on screen).
if touch "$LOG" 2>/dev/null; then
    printf '\n----- %s\n' "$(date -Is)" >>"$LOG"
    exec > >(tee -a "$LOG") 2>&1
fi

# ------------------------------------------------------------------ 1. device

step "Checking this device"
[[ "$(uname -m)" == x86_64 ]] || fail "Rare OS needs an x86_64 PC or handheld; this is $(uname -m)."

if command -v bootc >/dev/null && bootc status --booted >/dev/null 2>&1; then
    TOOL=bootc
elif command -v rpm-ostree >/dev/null && rpm-ostree status >/dev/null 2>&1; then
    TOOL=rpm-ostree
else
    fail "This system does not boot from an image, so it can't switch in place." \
         "Use the Rare OS USB-C installer instead (it erases the device): $GUIDE"
fi

. /etc/os-release
PRODUCT="$(cat /sys/class/dmi/id/product_name 2>/dev/null || echo PC)"
ok "${PRODUCT}, running ${PRETTY_NAME:-$NAME}"
ok "Boots from an image (${TOOL}), so it can switch and switch back"

if [[ $TOOL == bootc ]]; then BOOTED="$(bootc status --booted 2>/dev/null || true)"; else BOOTED="$(rpm-ostree status --booted 2>/dev/null || true)"; fi
if [[ "$BOOTED" == *"$REG/$REPO"* && "${RARE_FORCE:-0}" != 1 ]]; then
    ok "This device already runs Rare OS. Updates arrive by themselves; nothing to do."
    exit 0
fi

SB="$(sb_state)" MOK_NEEDED=0 KEY_FILE=""
if [[ $SB == off ]]; then
    ok "Secure Boot is off, so no extra step at the next start"
else
    command -v mokutil >/dev/null || fail "Secure Boot is on, and this system can't add Rare OS's start-up key (mokutil is missing)." \
        "Turn Secure Boot off in the firmware settings (Security > Secure Boot > Disabled, then Save)" \
        "and run the command again."
    KEY_FILE="$(mktemp /tmp/rare-os-key.XXXXXX.der)"
    trap 'rm -f "$KEY_FILE"' EXIT
    printf '%s' "$UBLUE_KEY_B64" | base64 -d > "$KEY_FILE"
    case "$(key_state "$KEY_FILE")" in
        enrolled) ok "Secure Boot is on and Rare OS's start-up key is already trusted" ;;
        *)  MOK_NEEDED=1
            ok "Secure Boot is on: Rare OS's start-up key gets added once, at the next start"
            warn "That start shows a blue key screen which needs a USB-C keyboard (steps follow)." ;;
    esac
fi

# ------------------------------------------------------------------ 2. power

step "Checking power"
battery="" ac=0
for p in /sys/class/power_supply/*; do
    [[ -r "$p/type" ]] || continue
    case "$(cat "$p/type")" in
        Battery)
            if [[ -r "$p/capacity" && -z "$battery" ]]; then battery="$(cat "$p/capacity")"; fi
            case "$(cat "$p/status" 2>/dev/null || true)" in Charging|Full) ac=1 ;; esac ;;
        Mains|USB|USB_C|USB_PD)
            if [[ "$(cat "$p/online" 2>/dev/null || true)" == 1 ]]; then ac=1; fi ;;
    esac
done
if [[ -z "$battery" ]]; then
    ok "No battery (desktop or virtual machine)"
elif (( ac )); then
    ok "Battery ${battery}%, charger connected"
elif (( battery >= MIN_BATTERY )); then
    ok "Battery ${battery}%"
    warn "Plug in the charger if you can: the download and install take a while."
elif [[ "${RARE_IGNORE_BATTERY:-0}" == 1 ]]; then
    warn "Battery ${battery}%, continuing because RARE_IGNORE_BATTERY=1"
else
    fail "Battery is at ${battery}% and the charger is not connected." \
         "Plug in the charger (or charge to ${MIN_BATTERY}% or more) and run the command again."
fi

# ------------------------------------------------------------------ 3. disk

step "Checking free space"
avail_kb="$(df --output=avail -k /sysroot 2>/dev/null | tail -1 || true)"
[[ "$avail_kb" =~ ^[[:space:]]*[0-9]+$ ]] || avail_kb="$(df --output=avail -k / | tail -1)"
FREE_GB=$(( avail_kb / 1024 / 1024 ))
if (( FREE_GB >= NEED_GB )); then
    ok "${FREE_GB} GB free on the system drive (Rare OS needs about ${NEED_GB} GB)"
else
    fail "Only ${FREE_GB} GB is free on the system drive; Rare OS needs about ${NEED_GB} GB." \
         "Uninstall a game or two to make room (Steam > Library > Manage > Uninstall), then run it again."
fi

# ------------------------------------------------------------------ 4. network

step "Checking the network"
SCHEME=https
[[ "${RARE_INSECURE_REGISTRY:-0}" == 1 ]] && SCHEME=http

code="$(curl -sS -o /dev/null -w '%{http_code}' --max-time 15 "$SCHEME://$REG/v2/" 2>/dev/null || true)"
case "$code" in
    200|401) ok "Connected to the download server ($REG)" ;;
    *) fail "Can't reach the download server ($REG)." \
            "Check that Wi-Fi is connected and has internet, then run the command again." ;;
esac

if [[ "$REG" == ghcr.io ]]; then
    token="$(curl -fsS --max-time 15 "https://ghcr.io/token?scope=repository:${REPO}:pull" 2>/dev/null | sed -n 's/.*"token":"\([^"]*\)".*/\1/p' || true)"
    mcode="$(curl -sS -o /dev/null -w '%{http_code}' --max-time 15 -I \
        -H "Authorization: Bearer ${token}" \
        -H 'Accept: application/vnd.oci.image.index.v1+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json, application/vnd.docker.distribution.manifest.list.v2+json' \
        "https://ghcr.io/v2/${REPO}/manifests/${TAG}" 2>/dev/null || true)"
    [[ "$mcode" == 200 ]] || fail "Rare OS is not available for download yet." \
        "Follow $GUIDE to hear when it is. Nothing on this device was changed."
    ok "Rare OS (${TAG}) is ready to download"
fi

if [[ "${RARE_INSECURE_REGISTRY:-0}" == 1 ]]; then
    mkdir -p /etc/containers/registries.conf.d
    printf '[[registry]]\nlocation = "%s"\ninsecure = true\n' "$REG" > /etc/containers/registries.conf.d/50-rare-test.conf
    warn "Plain-HTTP registry allowed for testing ($REG)"
fi

# ------------------------------------------------------------------ 5. switch

step "Switching to Rare OS"
cat <<EOF
  ${B}Stays:${R} your home folder, Steam and other game libraries, installed games, Flatpak
         apps, Wi-Fi networks and Bluetooth pairings.
  ${B}Changes:${R} the system itself. After a restart you start in the Rare Launcher.
  ${B}Undo:${R}
EOF
undo_text
if (( MOK_NEEDED )); then
    printf '\n'
    key_screen_text
fi
printf '\n'
ask "Download Rare OS now? Your files and games stay." || { printf '\n  Stopped. Nothing was changed.\n\n'; exit 1; }

# The current OS does not have Rare's signing key yet, so this first download is checked by
# TLS only. Rare OS ships the key and moves its own updates to signature-checked downloads
# on its first boot.
note "Downloading about 8 GB. On good Wi-Fi this takes 10 to 30 minutes; keep the device awake and plugged in."
note "If it stops halfway, run the same command again: it keeps what it already downloaded."
if [[ $TOOL == bootc ]]; then
    bootc switch --transport registry "$IMAGE" \
        || fail "The download did not finish." "Your current system is untouched. Check Wi-Fi and run the command again."
else
    rpm-ostree rebase "ostree-unverified-registry:${IMAGE}" \
        || fail "The download did not finish." "Your current system is untouched. Check Wi-Fi and run the command again."
fi

drop_staged() {
    if [[ $TOOL == bootc ]]; then
        rpm-ostree cleanup -p >/dev/null 2>&1 || ostree admin undeploy 0 >/dev/null 2>&1 || true
    else
        rpm-ostree cleanup -p >/dev/null 2>&1 || true
    fi
}

if (( MOK_NEEDED )); then
    # Queue Rare OS's key for the blue key screen (shim's MokManager), the way Bazzite's
    # `ujust enroll-secure-boot-key` does, with a fixed one-time password.
    src="$(write_key "$KEY_FILE")" || { drop_staged; fail "Couldn't prepare Rare OS's start-up key." \
        "The download was removed again; Secure Boot is on, so Rare OS could not start without it."; }
    note "Start-up key: ublue kernel ($src copy), $(openssl x509 -inform der -in "$KEY_FILE" -noout -fingerprint -sha256 2>/dev/null | cut -d= -f2)"
    mokutil --revoke-import >/dev/null 2>&1 || true
    mokutil --timeout -1 >/dev/null 2>&1 || true
    printf '%s\n%s\n' "$MOK_PASSWORD" "$MOK_PASSWORD" | mokutil --import "$KEY_FILE" >/dev/null 2>&1 || true
    if [[ "$(key_state "$KEY_FILE")" != pending ]]; then
        drop_staged
        fail "Secure Boot is on and the firmware did not accept Rare OS's start-up key request." \
             "The download was removed again, so the device starts as before." \
             "Turn Secure Boot off in the firmware settings (Security > Secure Boot > Disabled, then Save)," \
             "then run the command again."
    fi
    ok "Start-up key queued for the blue key screen (password: ${MOK_PASSWORD})"
fi

printf '\n'
ok "Rare OS is installed and starts the next time you restart."
printf '\n  %sNext:%s restart with  %ssudo reboot%s  (or hold the power button and pick Restart).\n' "$B" "$R" "$B" "$R"
printf '  The first start takes a little longer. Rare OS then walks you through Wi-Fi, signing in,\n'
printf '  moving assets from your old wallet, and Steam.\n'
if [[ "$ID" == *playtron* ]]; then
    printf '  Restart before you use GameOS again: a GameOS update installed before the restart\n'
    printf '  replaces Rare OS (just run this command again if that happens).\n'
fi
if (( MOK_NEEDED )); then
    printf '\n'
    key_screen_text
fi
printf '\n  %sChanged your mind?%s\n' "$B" "$R"
undo_text
if [[ "$ID" == *playtron* || -d /var/home/playtron ]]; then
    printf '\n  On a SuiPlay that came with GameOS, change the default password after the switch:\n'
    printf '  open a terminal in Rare OS and run  %spasswd%s\n' "$B" "$R"
fi
printf '\n'
if [[ "${RARE_REBOOT:-0}" == 1 ]]; then
    systemctl reboot
elif [[ "${RARE_YES:-0}" != 1 ]] && { [[ -t 0 ]] || [[ -r /dev/tty ]]; } && ask "Restart now?"; then
    printf '\n  Restarting...\n'
    systemctl reboot
fi
