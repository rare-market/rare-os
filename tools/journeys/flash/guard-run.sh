#!/usr/bin/bash
# Idle guard for the device tests: guard-run.sh [seconds, default 7200]. A hard dependency of every
# test's input (beat.py): idle.py keeps a heartbeat; eikey.py, pad.py and capture.sh send nothing
# while it is missing or 1 s old. When the guard exits for ANY reason (foreign input, its time is
# up, a crash, a kill), stalls, or this script is signalled, every injector is stopped at once and,
# while a test is armed (~/flashcap/ARMED), the stock session is restored (restore.sh).
# End it on purpose after a finished test with: python3 ~/flashcap/beat.py stop
here=$(cd "$(dirname "$0")" && pwd)
exec python3 -I "$here/beat.py" supervise --secs "${1:-7200}" 2>>"${FLASHCAP:-$HOME/flashcap}/guard.log"
