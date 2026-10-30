#!/usr/bin/bash
# variant.sh <mode> <label> [VAR=value...]: restart only the launcher in RARE_WEBKIT=<mode> with
# extra env (through a wrapper as the test launcher), settle, measure 20 s still, capture 14 tab
# switches (INPUT=ei). Needs the idle guard (guard-run.sh) running: aborts, and the session is
# restored (restore.sh, while ~/flashcap/ARMED exists), when the guard saw foreign input, exited for
# any reason or stalled (beat.py). Copy this directory to ~/flashcap on the device.
set -u
mode=$1 label=$2; shift 2
run=$XDG_RUNTIME_DIR
export DISPLAY=:0
guard() {
  local why
  if [[ -e ~/flashcap/INPUT ]] || ! why=$(python3 -I ~/flashcap/beat.py check --lost); then
    echo "ABORT: ${why:-$(cat ~/flashcap/INPUT)}"; exit 9
  fi
}
guard
printf '%s\n' "$@" > ~/flashcap/wl-env.conf
cat > ~/flashcap/wl-env.sh <<'W'
#!/usr/bin/bash
set -a; . "$HOME/flashcap/wl-env.conf"; set +a
exec "${RARE_TEST_BIN:-$HOME/flashcap/Rare-wl.AppImage}" "$@"
W
chmod +x ~/flashcap/wl-env.sh
echo "$mode" > $run/rare-webkit
echo "$HOME/flashcap/wl-env.sh" > $run/rare-launcher-bin
old=$(pgrep -x rare-launcher)
touch $run/rare-launcher-restart; pkill -x rare-launcher
for i in $(seq 60); do sleep 0.5; p=$(pgrep -x rare-launcher); [[ -n $p && $p != "$old" ]] && break; done
for i in $(seq 40); do sleep 0.5; xprop -root GAMESCOPE_FOCUSED_APP | grep -qw 1380012613 && break; done
sleep ${SETTLE:-18}
guard
p=$(pgrep -x rare-launcher)
echo "== $label mode=$mode env=[$*] pid=$p scope=$(sed 's|.*/||' /proc/$p/cgroup) focused=$(xprop -root GAMESCOPE_FOCUSED_APP | cut -d= -f2)"
for w in $(pgrep -f WebKitWebProcess); do echo "  WebKitWebProcess $w: $(grep -oE '(libgallium[^ ]*|libEGL_mesa[^ ]*)' /proc/$w/maps | sort -u | tr '\n' ' ')"; done
~/flashcap/measure.sh 20 | tail -1
guard
INPUT=ei GAP=1.2 ~/flashcap/capture.sh "$label" e e e e e e e q q q q q q q 2>&1 | tail -1
echo "  keys sent: $(wc -l < ~/flashcap/$label/keys.log)"
guard
