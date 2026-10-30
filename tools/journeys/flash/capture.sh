#!/usr/bin/bash
# Device side of the flash check: records gamescope's composited output at full rate (every frame
# gamescope composites, downscaled to 640x400 RGB) while it optionally sends launcher keys.
# Read-only for the session: a second PipeWire consumer of the gamescope node, plus xdotool keys.
#
#   capture.sh <label> [steps...]
#   steps: an xdotool key name (e = RB next tab, q = LB, Down, Return, ...), or wait:<seconds>
#   env:   GAP (seconds after each key, 1.2), DUR (seconds to record when no steps, 10),
#          OUT (default ~/flashcap/<label>), FORCE=1 to send keys even when Rare is not in front,
#          INPUT=ei to send keys through gamescope's libei socket (eikey.py) instead of xdotool:
#          xdotool only reaches X11 clients, so a native Wayland launcher needs INPUT=ei
#
# Keys need a live idle guard (guard-run.sh): before every key its heartbeat must be under 1 s old
# (beat.py check), else no more keys go out and the session is restored while armed.
# Keys go to whatever has focus, so each one is sent only while the launcher (STEAM_GAME 1380012613)
# is GAMESCOPE_FOCUSED_APP; otherwise the run stops sending keys. The launcher also ignores keyboard
# keys for 1.5 s after any controller press, so do not run it while someone is using the device.
# Writes <label>.raw (frames), <label>.pts (seconds per frame), keys.log, focus.log under OUT,
# all on disk (never the 1.5 GB /run/user tmpfs).
set -u
label=${1:?label}; shift
out=${OUT:-$HOME/flashcap/$label}
rm -rf "$out"; mkdir -p "$out"
export DISPLAY=${DISPLAY:-:0}
rare=1380012613
focused() { xprop -root GAMESCOPE_FOCUSED_APP 2>/dev/null | grep -qw "$rare"; }

gst-launch-1.0 -e pipewiresrc target-object=gamescope do-timestamp=true ! video/x-raw,format=BGRx \
  ! videoscale method=nearest-neighbour ! video/x-raw,width=640,height=400 ! videoconvert ! video/x-raw,format=RGB \
  ! matroskamux ! filesink location="$out/cap.mkv" >"$out/gst.log" 2>&1 &
gp=$!
sleep 1.5
if (($#)) && [[ ${INPUT:-x11} == ei ]]; then
  # Keys through gamescope's libei socket (eikey.py): reaches a native Wayland launcher too.
  args=(--gap "${GAP:-1.2}"); [[ ${FORCE:-0} == 1 ]] || args+=(--only "$rare")
  python3 -I "$(dirname "$0")/eikey.py" "${args[@]}" "$@" >>"$out/keys.log" || echo "$(date +%s.%N) eikey stopped ($?)" | tee -a "$out/focus.log" >&2
  sleep 1
elif (($#)); then
  for k in "$@"; do
    if [[ $k == wait:* ]]; then sleep "${k#wait:}"; continue; fi
    if [[ ${FORCE:-0} != 1 ]] && ! focused; then
      echo "$(date +%s.%N) stop: Rare is not in front ($(xprop -root GAMESCOPE_FOCUSED_APP | cut -d= -f2))" | tee -a "$out/focus.log" >&2
      break
    fi
    if ! why=$(python3 -I "$(dirname "$0")/beat.py" check --lost 2>/dev/null); then
      echo "$(date +%s.%N) stop: $why" | tee -a "$out/focus.log" >&2
      break
    fi
    echo "$(date +%s.%N) $k" >>"$out/keys.log"
    xdotool key "$k"
    sleep "${GAP:-1.2}"
  done
  sleep 1
else
  sleep "${DUR:-10}"
fi
kill -INT $gp; wait $gp
ffmpeg -loglevel error -y -i "$out/cap.mkv" -fps_mode passthrough -f rawvideo -pix_fmt rgb24 "$out/$label.raw"
ffprobe -loglevel error -select_streams v -show_entries frame=pts_time -of csv=p=0 "$out/cap.mkv" >"$out/$label.pts"
rm -f "$out/cap.mkv"
echo "capture $label: $(wc -l <"$out/$label.pts") frames in $out"
