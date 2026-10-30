#!/usr/bin/env bash
# Tests for os-admin factory-reset and the boot-time factory-reset, run as an unprivileged user
# against a temp RARE_ROOT.
#   image/tests/factory-reset.sh
# The checks are eval'd strings, so they expand later on purpose.
# shellcheck disable=SC2016,SC2034
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
lib="$here/../system_files/usr/libexec/rare"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
fails=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }

setup() {
    rm -rf "$T/root"; R="$T/root"
    mkdir -p "$R/etc/skel" "$R/var/lib/rare" "$R/etc/NetworkManager/system-connections" "$R/var/lib/bluetooth/AA:BB:CC:DD:EE:FF/11:22:33:44:55:66" "$R/var/lib/bluetooth/AA:BB:CC:DD:EE:FF/cache"
    printf 'root:x:0:0::/root:/bin/bash\ntester:x:1000:1000::/home/tester:/bin/bash\nodd:x:1001:1001::/opt/odd:/bin/bash\n' >"$R/etc/passwd"
    echo skel >"$R/etc/skel/.bashrc"
    touch "$R/etc/NetworkManager/system-connections/home.nmconnection" "$R/var/lib/bluetooth/AA:BB:CC:DD:EE:FF/settings"
    H="$R/var/home/tester"
    mkdir -p "$H/.config/rare" "$H/.local/share/rare/wallet" "$H/.local/share/rare/friends" "$H/.local/share/rare/prefixes/g1" "$H/.local/share/rare/webgames/w1" \
        "$H/.local/share/Steam/steamapps" "$H/Games/rom" "$H/.cache/rare" "$H/.local/state/rare" "$H/Documents" "$R/opt/odd"
    touch "$H/.config/rare/launcher.json" "$H/.local/share/rare/wallet/vault.json" "$H/.local/share/Steam/steamapps/game.acf" "$H/Documents/note.txt" "$R/opt/odd/keep"
}

export RARE_ROOT RARE_LIB_DIR="$lib" RARE_NO_REBOOT=1 RARE_SYSTEMCTL=true
setup; RARE_ROOT="$R"
check "os-admin refuses a bad mode" '! "$lib/os-admin" factory-reset everything 2>/dev/null && [[ ! -e $R/var/lib/rare/factory-reset ]]'
"$lib/os-admin" factory-reset keep-games
check "os-admin writes the marker" '[[ "$(cat "$R/var/lib/rare/factory-reset")" == keep-games ]]'
out="$("$lib/factory-reset")"
check "marker is gone" '[[ ! -e $R/var/lib/rare/factory-reset ]]'
check "keep-games clears Rare" '[[ ! -e $H/.config/rare && ! -e $H/.local/share/rare/wallet && ! -e $H/.local/share/rare/friends && ! -e $H/.cache/rare ]]'
check "keep-games keeps games" '[[ -e $H/.local/share/Steam/steamapps/game.acf && -d $H/Games/rom && -d $H/.local/share/rare/prefixes/g1 && -d $H/.local/share/rare/webgames/w1 && -e $H/Documents/note.txt ]]'
check "Wi-Fi and pairings gone, adapter settings kept" '[[ -z "$(ls "$R/etc/NetworkManager/system-connections")" && ! -e "$R/var/lib/bluetooth/AA:BB:CC:DD:EE:FF/11:22:33:44:55:66" && -e "$R/var/lib/bluetooth/AA:BB:CC:DD:EE:FF/settings" ]]'
check "home outside /var/home skipped" '[[ -e $R/opt/odd/keep && "$out" == *"skipped odd"* ]]'
check "no marker, nothing done" '[[ -z "$("$lib/factory-reset")" ]]'

setup; RARE_ROOT="$R"
"$lib/os-admin" factory-reset erase-all
"$lib/factory-reset" >/dev/null
check "erase-all empties the home from skel" '[[ "$(ls -A "$H")" == .bashrc && -d $H ]]'

setup; RARE_ROOT="$R"
echo bogus >"$R/var/lib/rare/factory-reset"
"$lib/factory-reset" >/dev/null
check "unknown mode does nothing" '[[ ! -e $R/var/lib/rare/factory-reset && -e $H/.config/rare/launcher.json ]]'

if (( fails == 0 )); then echo "all passed"; else echo "$fails failed"; exit 1; fi
