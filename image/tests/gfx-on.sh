#!/usr/bin/env bash
# Tests for the shutdown hook system_files/usr/lib/systemd/system-shutdown/rare-gfx-on with a fake
# debugfs: image/tests/gfx-on.sh
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
H="$here/../system_files/usr/lib/systemd/system-shutdown/rare-gfx-on"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
export RARE_DEBUGFS="$T/debug"
fails=0
setup() { # a GPU at dri/0, the same device linked as dri/0000:c5:00.0 and dri/128
    rm -rf "$RARE_DEBUGFS"; mkdir -p "$RARE_DEBUGFS/dri/0"
    : >"$RARE_DEBUGFS/dri/0/amdgpu_gfxoff"
    ln -s 0 "$RARE_DEBUGFS/dri/0000:c5:00.0"; ln -s 0 "$RARE_DEBUGFS/dri/128"
}
check() { # name, verb, expected bytes written (as od shows them)
    setup
    "$H" "$2" 2>/dev/null || { echo "FAIL $1: exit $?"; fails=$((fails + 1)); return; }
    local got; got="$(od -An -tx1 "$RARE_DEBUGFS/dri/0/amdgpu_gfxoff" | tr -s ' ' | sed 's/^ //;s/ $//')"
    if [[ "$got" == "$3" ]]; then echo "ok   $1"; else echo "FAIL $1: got [$got] want [$3]"; fails=$((fails + 1)); fi
}

check "restart: one u32 0 (GFXOFF not allowed), once per GPU" reboot "00 00 00 00"
check "power-off: the same" poweroff "00 00 00 00"
check "halt: the same" halt "00 00 00 00"
check "kexec: nothing" kexec ""

rm -rf "$RARE_DEBUGFS"; mkdir -p "$RARE_DEBUGFS"
if "$H" reboot 2>/dev/null; then echo "ok   no amdgpu: nothing to do, exit 0"; else echo "FAIL no amdgpu: exit $?"; fails=$((fails + 1)); fi

(( fails == 0 )) && echo "all passed" || { echo "$fails failed"; exit 1; }
