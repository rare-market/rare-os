#!/usr/bin/bash
# Device side. Still-screen 20 s: CPU% (of one core) of WebKit*+rare-launcher+Xwayland,
# gamescope separately, GPU busy avg, APU W avg. Usage: measure.sh [seconds]
export DISPLAY=:0
S=${1:-20}
P=$(pgrep -f 'WebKitWebProcess|WebKitNetworkProcess|rare-launcher|Xwayland' | tr '\n' ' ')
G=$(pgrep -x gamescope-wl || pgrep -x gamescope)
ticks() { t=0; for p in "$@"; do s=$(awk '{print $14+$15}' /proc/$p/stat 2>/dev/null) && t=$((t+${s:-0})); done; echo $t; }
t0=$(ticks $P); g0=$(ticks $G); n=0; g=0; w=0
for i in $(seq $((S*2))); do
  g=$((g+$(cat /sys/class/drm/card1/device/gpu_busy_percent))); w=$((w+$(cat /sys/class/hwmon/hwmon4/power1_average))); n=$((n+1)); sleep 0.5
done
t1=$(ticks $P); g1=$(ticks $G)
echo "procs: $(for p in $P; do cat /proc/$p/comm; done | sort | uniq -c | tr '\n' ' ')"
echo "cpu_pct: $(echo "scale=1; ($t1-$t0)/$S" | bc)%  gamescope: $(echo "scale=1; ($g1-$g0)/$S" | bc)%  gpu_busy_avg: $(echo "scale=1; $g/$n" | bc)%  apu_W_avg: $(echo "scale=2; $w/$n/1000000" | bc)"
