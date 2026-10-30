#!/usr/bin/env bash
# On the build machine (sudo, xserver-xorg-video-dummy, xserver-xorg-input-libinput, mesa lavapipe): the
# launcher (fixtures) in a real X server with libinput, driven by a virtual multitouch touchscreen
# (vtouch.py), the path the SuiPlay takes (gamescope's Xwayland, XI2 touch, winit). Logs every
# gesture (RR_TOUCH_LOG) and saves a screenshot of the X screen.
#   BIN=<rare-native dir> xtouch.sh NAME MODE ROTATE "LAUNCHER ENV" STEP...
#   MODE plain: a landscape panel; rot270: a portrait-native panel (raw points sent turned) with
#   RR_TOUCH_ROTATE=ROTATE undoing it. STEPs: see vtouch.py. Example:
#   BIN=~/rare-dev/out/x/rare-native xtouch.sh rot rot270 270 "" wait:1500 tap:419:51 drag:960:900:960:300:150 pinch:960:600 hold:1085:192
set -u
name=$1 mode=$2 rot=$3 envs=$4; shift 4
D=:57; W=1920; H=1200
dir=$(cd "$(dirname "$0")" && pwd); out=${OUT:-$HOME/rare-build/touch/out}; mkdir -p $out
bin=${BIN:?BIN=<a packaged rare-native dir>}
log=$out/$name.log; : >$log
# the device first (Xorg takes it by path)
SETTLE=8 sudo -E python3 $dir/vtouch.py $mode $W $H -- "$@" >$out/$name.dev 2>&1 &
dev=$!
for i in $(seq 50); do node=$(sed -n 's/^NODE //p' $out/$name.dev); [ -n "$node" ] && break; sleep 0.1; done
cat >$out/xorg-$name.conf <<C
Section "ServerFlags"
  Option "AutoAddDevices" "false"
  Option "AutoEnableDevices" "false"
  Option "DontVTSwitch" "true"
EndSection
Section "Device"
  Identifier "dummy"
  Driver "dummy"
  VideoRam 256000
EndSection
Section "Monitor"
  Identifier "mon"
  HorizSync 5.0-1000.0
  VertRefresh 5.0-200.0
  Option "ReducedBlanking"
  Modeline "${W}x${H}" 154.00 $W 1968 2000 2080 $H 1203 1209 1235 +hsync -vsync
EndSection
Section "Screen"
  Identifier "scr"
  Device "dummy"
  Monitor "mon"
  DefaultDepth 24
  SubSection "Display"
    Depth 24
    Modes "${W}x${H}"
    Virtual $W $H
  EndSubSection
EndSection
Section "InputDevice"
  Identifier "touch"
  Driver "libinput"
  Option "Device" "$node"
EndSection
Section "ServerLayout"
  Identifier "l"
  Screen "scr"
  InputDevice "touch"
EndSection
C
sudo Xorg $D vt1 -sharevts -novtswitch -noreset -nolisten tcp -config $out/xorg-$name.conf -logfile $out/$name.xorg.log >/dev/null 2>&1 &
xpid=$!
sleep 2.5
sudo chmod 666 /tmp/.X11-unix/X${D#:} 2>/dev/null
(cd $bin && env DISPLAY=$D XDG_SESSION_TYPE=x11 WAYLAND_DISPLAY= VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json RR_SILENT=1 RR_NO_IP=1 RR_FIXTURE=1 RR_WINDOWED=${W}x${H} RR_CLOCK="8:05 PM" RR_TOUCH_ROTATE=$rot $envs RR_TOUCH_LOG=1 ./rare-launcher-native >>$log 2>&1) &
app=$!
wait $dev
sleep 1.5
xwd -root -display $D -silent | convert xwd:- $out/$name.png 2>/dev/null
kill $app 2>/dev/null; sleep 0.3; sudo kill $xpid 2>/dev/null; wait 2>/dev/null
grep -E "touch|tap|panic" $log | tail -20
echo "shot $out/$name.png"
