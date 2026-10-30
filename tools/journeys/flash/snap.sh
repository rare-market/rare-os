#!/usr/bin/bash
# One composited frame of gamescope's output as PNG (downscaled): snap.sh <out.png>
gst-launch-1.0 -q pipewiresrc target-object=gamescope num-buffers=3 ! videoconvert ! videoscale ! video/x-raw,width=960,height=600 ! pngenc snapshot=true ! filesink location="${1:-$HOME/flashcap/snap.png}" >/dev/null 2>&1
