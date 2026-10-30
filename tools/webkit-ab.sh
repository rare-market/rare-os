#!/usr/bin/env bash
# Live A/B of how the launcher's WebKit draws under gamescope (launcher/src-tauri/src/render.rs).
# Run from the Mac; it drives the SuiPlay over ssh. For each variant it restarts only the launcher
# (rare-session-client keeps the gamescope session up), lets it settle, samples CPU (launcher,
# WebKit, Xwayland, gamescope), GPU busy % and APU watts on a still screen, then asks the owner to
# move between screens and say whether anything flashed.
#
#   tools/webkit-ab.sh setup                 # once: session client that survives launcher restarts
#   tools/webkit-ab.sh [variants...]         # default: cpu gpu dmabuf wayland
#   tools/webkit-ab.sh restore               # back to the default (gpu, or ~/.config/rare/webkit)
#
# Variants: cpu = WEBKIT_DISABLE_DMABUF_RENDERER=1 (old path), gpu = DMA-BUF renderer with SHM
# transport (new default), dmabuf = X11 DMA-BUF defaults (the flashing path), wayland = native
# Wayland client of gamescope in app-steam-app1380012613-<pid>.scope.
# Env: HOST (suiplay), IDLE (idle sample seconds, 20), SETTLE (seconds after start, 12).
# Needs a launcher build with render.rs on the device. Writes bench/<date>-webkit-ab.csv.
set -euo pipefail

HOST="${HOST:-suiplay}"
IDLE="${IDLE:-20}"
SETTLE="${SETTLE:-12}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APPID=1380012613
r() { ssh -o ConnectTimeout=8 "$HOST" "$@"; }

if [[ "${1:-}" == setup ]]; then
    # gamescope-session-plus ends the session when its client exits, so the client has to be a
    # loop. The image ships /usr/bin/rare-session-client; older images get a copy in ~/.local/bin.
    r 'bash -s' <<EOF
set -e
bin=/usr/bin/rare-launcher
[[ -x "\$HOME/Applications/Rare-Launcher-x86_64.AppImage" ]] && bin="\$HOME/Applications/Rare-Launcher-x86_64.AppImage"
client=/usr/bin/rare-session-client
if [[ ! -x \$client ]]; then
    mkdir -p "\$HOME/.local/bin"
    cat >"\$HOME/.local/bin/rare-session-client" <<'CLIENT'
$(cat "$ROOT/image/system_files/usr/bin/rare-session-client")
CLIENT
    chmod +x "\$HOME/.local/bin/rare-session-client"
    client="\$HOME/.local/bin/rare-session-client"
fi
f="\$HOME/.config/gamescope-session-plus/sessions.d/rare"
mkdir -p "\$(dirname "\$f")"
[[ -f "\$f" && ! -f "\$f.before-webkit-ab" ]] && cp "\$f" "\$f.before-webkit-ab"
printf '%s\n' "# webkit-ab setup (previous file: rare.before-webkit-ab)" "RARE_LAUNCHER_BIN=\$bin" "CLIENTCMD=\$client" >"\$f"
echo "client \$client, launcher \$bin, written to \$f"
EOF
    echo "Restart the session once so it takes the new client (this ends what is on screen):"
    echo "  ssh $HOST systemctl --user restart gamescope-session-plus@rare"
    exit 0
fi

# Remote helpers, sent with every call.
read -r -d '' HELPERS <<'EOF' || true
run="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
lpid() { pgrep -x rare-launcher | head -1; }
wpid() { pgrep -n -f /WebKitWebProcess; }
client_ok() { pgrep -f rare-session-client >/dev/null; }
EOF

restart() { # $1 = variant, or empty for the setting
    r "bash -s" <<EOF
$HELPERS
client_ok || { echo "NO-CLIENT"; exit 3; }
if [[ -n "$1" ]]; then echo "$1" >"\$run/rare-webkit"; else rm -f "\$run/rare-webkit"; fi
old=\$(lpid)
touch "\$run/rare-launcher-restart"
[[ -n "\$old" ]] && kill "\$old"
for _ in \$(seq 1 100); do n=\$(lpid); [[ -n "\$n" && "\$n" != "\$old" ]] && [[ -n "\$(wpid)" ]] && break; sleep 0.2; done
[[ -n "\$(wpid)" ]] || { echo "NO-WEBPROCESS"; exit 4; }
EOF
}

if [[ "${1:-}" == restore ]]; then
    restart ""
    echo "launcher restarted with its setting (gpu unless ~/.config/rare/webkit says otherwise)"
    exit 0
fi

VARIANTS=("$@")
[[ ${#VARIANTS[@]} -gt 0 ]] || VARIANTS=(cpu gpu dmabuf wayland)
mkdir -p "$ROOT/bench"
CSV="$ROOT/bench/$(date +%Y-%m-%d)-webkit-ab.csv"
[[ -f "$CSV" ]] || echo "time,variant,mesa_in_webprocess,backend,focused_app,cpu_launcher,cpu_webkit,cpu_xwayland,cpu_gamescope,cpu_total,gpu_busy,apu_w,flash,notes" >"$CSV"

measure() { # prints: mesa backend focused cpu_l cpu_w cpu_x cpu_g cpu_t gpu w
    r "IDLE=$IDLE APPID=$APPID bash -s" <<'EOF'
run="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
w=$(pgrep -n -f /WebKitWebProcess)
mesa=no; grep -qE 'libgallium|radeonsi_dri|dri_gbm' /proc/$w/maps 2>/dev/null && mesa=yes
backend=x11; tr '\0' '\n' </proc/$w/environ 2>/dev/null | grep -qx 'GDK_BACKEND=wayland' && backend=wayland
focused=$(DISPLAY=:0 xprop -root -notype GAMESCOPE_FOCUSED_APP 2>/dev/null | grep -oE '[0-9]+' | head -1)
card=$(for c in /sys/class/drm/card*/device; do [[ -r $c/gpu_busy_percent ]] && echo $c && break; done)
hw=$(for h in $card/hwmon/hwmon*; do [[ "$(cat $h/name)" == amdgpu ]] && echo $h && break; done)
python3 - "$IDLE" "$card" "$hw" <<'PY'
import os, sys, time
secs, card, hw = int(sys.argv[1]), sys.argv[2], sys.argv[3]
hz = os.sysconf('SC_CLK_TCK')
def groups():
    g = {'launcher': [], 'webkit': [], 'xwayland': [], 'gamescope': []}
    for p in os.listdir('/proc'):
        if not p.isdigit(): continue
        try: comm = open(f'/proc/{p}/comm').read().strip()
        except OSError: continue
        if comm == 'rare-launcher': g['launcher'].append(p)
        elif comm.startswith('WebKit'): g['webkit'].append(p)
        elif comm == 'Xwayland': g['xwayland'].append(p)
        elif comm in ('gamescope', 'gamescope-wl'): g['gamescope'].append(p)
    return g
def ticks(pids):
    t = 0
    for p in pids:
        try: f = open(f'/proc/{p}/stat').read().rsplit(')', 1)[1].split()
        except OSError: continue
        t += int(f[11]) + int(f[12])
    return t
g = groups()
t0 = {k: ticks(v) for k, v in g.items()}
busy, watts = [], []
start = time.time()
for _ in range(secs):
    time.sleep(1)
    try: busy.append(int(open(f'{card}/gpu_busy_percent').read()))
    except OSError: pass
    try: watts.append(int(open(f'{hw}/power1_average').read()) / 1e6)
    except OSError: pass
el = time.time() - start
cpu = {k: 100.0 * (ticks(v) - t0[k]) / hz / el for k, v in g.items()}
avg = lambda xs: sum(xs) / len(xs) if xs else float('nan')
print(f"{cpu['launcher']:.1f} {cpu['webkit']:.1f} {cpu['xwayland']:.1f} {cpu['gamescope']:.1f} {sum(cpu.values()):.1f} {avg(busy):.1f} {avg(watts):.2f}")
PY
echo "$mesa $backend ${focused:-0}"
EOF
}

declare -a SUMMARY=()
for v in "${VARIANTS[@]}"; do
    echo
    echo "=== $v: restarting the launcher"
    if ! out=$(restart "$v" 2>&1); then
        echo "$out"
        [[ "$out" == *NO-CLIENT* ]] && { echo "The session client cannot restart the launcher. Run: tools/webkit-ab.sh setup"; exit 1; }
        echo "launcher did not come back with $v; skipping"; continue
    fi
    echo "settling ${SETTLE} s (leave the device alone, launcher on its home screen)"
    sleep "$SETTLE"
    echo "sampling a still screen for ${IDLE} s ..."
    res=$(measure)
    read -r cl cw cx cg ct gb aw <<<"$(sed -n 1p <<<"$res")"
    read -r mesa backend focused <<<"$(sed -n 2p <<<"$res")"
    note=""
    [[ "$focused" == "$APPID" ]] || note="gamescope focus is app $focused, not Rare"
    echo "  web process Mesa: $mesa, backend: $backend, gamescope focus: $focused ${note:+($note)}"
    echo "  CPU % of a core: launcher $cl, WebKit $cw, Xwayland $cx, gamescope $cg, total $ct"
    echo "  GPU busy $gb %, APU $aw W"
    echo
    echo "Owner: on the device, move between screens for ~20 s (home, library, a game page, settings, back),"
    echo "open and close the overlay once if a game is running. Watch for any flash."
    read -r -p "Did anything flash with '$v'? [y/n] " flash </dev/tty
    read -r -p "Anything else (focus, input, smoothness)? (Enter for none) " extra </dev/tty
    notes="${note}${note:+; }${extra}"
    echo "$(date +%H:%M:%S),$v,$mesa,$backend,$focused,$cl,$cw,$cx,$cg,$ct,$gb,$aw,$flash,\"${notes//\"/\'}\"" >>"$CSV"
    SUMMARY+=("$(printf '%-8s mesa=%-3s %-7s cpu=%5s%% gpu=%5s%% apu=%5sW flash=%s' "$v" "$mesa" "$backend" "$ct" "$gb" "$aw" "$flash")")
done

echo
echo "=== summary ($CSV)"
printf '%s\n' "${SUMMARY[@]}"
echo
echo "The last variant stays until reboot. Back to the setting: tools/webkit-ab.sh restore"
echo "Keep a variant across reboots: ssh $HOST 'mkdir -p ~/.config/rare && echo <variant> > ~/.config/rare/webkit'"
