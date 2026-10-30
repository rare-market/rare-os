#!/usr/bin/env bash
# Live-test the Rare Speakers chain on a running SuiPlay0X1 WITHOUT restarting PipeWire or
# WirePlumber. The chain is loaded into a separate client process (pw-cli); it appears as a sink
# named rare_speaker_eq_test with LOW priority (so the default sink does not change) and plays
# into the ALSA analog sink. Stopping = killing that process: the chain vanishes, nothing else
# is touched. Never use pkill on pipewire; this script only signals its own pw-cli.
#
#   speaker-eq-live-test.sh start [conf]   load (conf defaults to /etc/pipewire/pipewire.conf.d/50-rare-speaker-eq.conf,
#                                          or copy the repo file to /tmp and pass it)
#   speaker-eq-live-test.sh route          apply wet/dry for the current port to the test chain
#   speaker-eq-live-test.sh play [file]    play a 1 kHz tone (or file) through the test chain
#   speaker-eq-live-test.sh status         show the test node and its links
#   speaker-eq-live-test.sh stop           unload
set -euo pipefail
D=${XDG_RUNTIME_DIR:-/tmp}/rare-eq-test
CONF_DEFAULT=/etc/pipewire/pipewire.conf.d/50-rare-speaker-eq.conf
case "${1:-}" in
start)
    [ ! -e "$D/pid" ] || { echo "already started (run stop first)"; exit 1; }
    conf=${2:-$CONF_DEFAULT}
    mkdir -p "$D"
    args=$(sed -n '/#BEGIN-ARGS/,/#END-ARGS/p' "$conf" | grep -v '#BEGIN-ARGS\|#END-ARGS' \
        | sed -e 's/"rare_speaker_eq"/"rare_speaker_eq_test"/' \
              -e 's/"rare_speaker_eq_out"/"rare_speaker_eq_test_out"/' \
              -e 's/"Rare Speakers"/"Rare Speakers TEST"/' \
              -e 's/priority\.session = 950/priority.session = 100/' \
              -e 's/priority\.driver = 950/priority.driver = 100/')
    printf 'load-module libpipewire-module-filter-chain { %s }\n' "$(echo "$args" | tr '\n' ' ')" > "$D/cmd"
    mkfifo "$D/fifo"
    # keep the fifo open for writing, then feed the load command; pw-cli (and the module) lives until killed
    ( exec sleep 86400 > "$D/fifo" ) & echo $! > "$D/sleeper"
    pw-cli < "$D/fifo" > "$D/log" 2>&1 & echo $! > "$D/pid"
    sleep 0.5
    # the sleeper keeps the fifo open, so this write does not end pw-cli's input
    cat "$D/cmd" > "$D/fifo"
    sleep 1.5
    cat "$D/log"
    pw-cli ls Node | grep -A0 -B2 rare_speaker_eq_test || { echo "test node not found; see $D/log"; exit 1; }
    echo "loaded. Next: $0 route ; $0 play"
    ;;
route)
    r=/usr/libexec/rare/speaker-eq-route
    [ -x "$r" ] || r=$(dirname "$0")/../../image/system_files/usr/libexec/rare/speaker-eq-route
    RARE_EQ_NODE=rare_speaker_eq_test RARE_EQ_ONCE=1 "$r"
    ;;
play)
    f=${2:-}
    if [ -z "$f" ]; then
        f=/tmp/rare-eq-1k.wav
        python3 - "$f" <<'P'
import wave, struct, math, sys
w = wave.open(sys.argv[1], "w"); w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
a = 0.89  # -1 dBFS
w.writeframes(b"".join(struct.pack("<hh", *(2 * [int(32767 * a * math.sin(2 * math.pi * 1000 * i / 48000))])) for i in range(96000)))
P
    fi
    pw-cat -p --target rare_speaker_eq_test "$f"
    ;;
status)
    pw-cli ls Node | grep -B3 -A3 rare_speaker_eq_test || echo "not loaded"
    pw-link -l 2>/dev/null | grep -A2 rare_speaker_eq_test || true
    ;;
stop)
    for f in pid sleeper; do [ -s "$D/$f" ] && kill "$(cat "$D/$f")" 2>/dev/null || true; done
    rm -rf "$D"
    sleep 0.5
    pw-cli ls Node | grep -q rare_speaker_eq_test && echo "still present" || echo "unloaded"
    ;;
*) sed -n '2,16p' "$0"; exit 2 ;;
esac
