#!/usr/bin/env bash
# Headless touch shots (RR_SCRIPT tap/hold/drag steps), both sizes.
# BIN=<rare-native dir> shots.sh NAME "ENV..." "SCRIPT"  — SCRIPT may use fractions written fX/fY (0..1) for coordinates:
# tap:f0.5:f0.3 becomes tap:960:360 at 1920x1200. Writes ~/rare-dev/touch/shots/NAME-WxH.png
set -u
name=$1 envs=$2 script=$3
bin=${BIN:?BIN=<a packaged rare-native dir>}
out=$HOME/rare-build/touch/shots; mkdir -p $out
cd $bin
for sz in ${SIZES:-1920x1200 1280x800}; do
  W=${sz%x*}; H=${sz#*x}
  s=$(python3 - "$script" $W $H <<'P'
import re,sys
s,W,H=sys.argv[1],float(sys.argv[2]),float(sys.argv[3])
def sub(m):
    parts=m.group(0).split(':'); out=[parts[0]]
    for i,p in enumerate(parts[1:]):
        if p.startswith('f'):
            v=float(p[1:]); out.append(str(round(v*(W if i%2==0 else H))))
        else: out.append(p)
    return ':'.join(out)
print(re.sub(r'(tap|hold|drag)(:[f0-9.]+)+',sub,s))
P
)
  env RR_SILENT=1 RR_NO_IP=1 RR_FIXTURE=1 RR_WINDOWED=$sz RR_CLOCK="8:05 PM" RR_TOUCH_LOG=1 $envs RR_SCRIPT="$s" RR_SHOT=$out/$name-$sz.png timeout 90 ./rare-launcher-native > $out/$name-$sz.log 2>&1 || echo "FAIL $name $sz"
  grep "touch:" $out/$name-$sz.log | tail -4
done
