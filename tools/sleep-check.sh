#!/usr/bin/env bash
# Rare OS sleep check. Run as root on the device:
#   sudo tools/sleep-check.sh [-s SECONDS] [-n] [-a]
#     -s N   sleep for N seconds (default 120; use 600+ for a real drain figure)
#     -n     do not suspend, only print the pre-flight checks
#     -a     also run AMD's `amd-s2idle test` first when it is installed
# Suspends through systemctl (so logind, Steam and the launcher react as they do for a real press),
# arms the RTC with rtcwake, and reports: s0ix hardware residency, the wake source, resume time
# from the journal, Wi-Fi/controller/fan/panel state after resume, and battery drain per hour.
# Unplug the charger first, or the drain figure is meaningless. Nothing here changes settings
# except /sys/power/pm_debug_messages, which is put back.
set -u

SECS=120 DRY=0 AMD=0
while getopts s:na o; do
  case $o in s) SECS=$OPTARG ;; n) DRY=1 ;; a) AMD=1 ;; *) sed -n '2,10p' "$0"; exit 2 ;; esac
done
[ "$(id -u)" = 0 ] || { echo "run as root (sudo)"; exit 1; }

ok() { printf '  ok    %s\n' "$*"; }
warn() { printf '  WARN  %s\n' "$*"; WARNS=$((WARNS + 1)); }
hdr() { printf '\n== %s\n' "$*"; }
rd() { cat "$1" 2>/dev/null || true; }
WARNS=0
DBG=/sys/kernel/debug/amd_pmc
mountpoint -q /sys/kernel/debug || mount -t debugfs none /sys/kernel/debug 2>/dev/null

bat=$(ls -d /sys/class/power_supply/BAT* 2>/dev/null | head -1)
energy() { # microwatt-hours now; falls back to charge*voltage
  if [ -r "$bat/energy_now" ]; then rd "$bat/energy_now"
  elif [ -r "$bat/charge_now" ]; then echo $(( $(rd "$bat/charge_now") * $(rd "$bat/voltage_now") / 1000000 ))
  else echo 0; fi
}
full() { [ -r "$bat/energy_full" ] && rd "$bat/energy_full" || echo $(( $(rd "$bat/charge_full") * $(rd "$bat/voltage_now") / 1000000 )); }
hw_us() { rd /sys/power/suspend_stats/last_hw_sleep; }
# S0ix residency from the PMC's debugfs: "Residency Time: N" in microseconds.
pmc_resid() { awk -F: '/Residency Time/ {gsub(/ /,"",$2); print $2}' $DBG/s0ix_stats 2>/dev/null; }
wakecounts() { for w in /sys/class/wakeup/wakeup*; do echo "$(rd $w/name) $(rd $w/event_count) $(rd $w/wakeup_count) $(readlink -f $w/device | sed 's|/sys/devices/||')"; done; }

hdr "Pre-flight (what AMD's amd-s2idle prerequisites look for)"
echo "  kernel $(uname -r), $(rd /sys/class/dmi/id/product_name), BIOS $(rd /sys/class/dmi/id/bios_version)"
[ "$(rd /sys/power/mem_sleep)" = "[s2idle]" ] && ok "mem_sleep is s2idle only" || warn "mem_sleep = $(rd /sys/power/mem_sleep) (s2idle must be the only mode, or the selected one)"
grep -q amd_pmc /proc/modules && ok "amd_pmc loaded" || warn "amd_pmc not loaded: no s0ix residency figures"
[ -r $DBG/s0ix_stats ] && ok "amd_pmc debugfs readable" || warn "no $DBG/s0ix_stats (debugfs?); using /sys/power/suspend_stats/last_hw_sleep only"
[ -n "$(rd $DBG/smu_fw_version)" ] && echo "  SMU firmware $(rd $DBG/smu_fw_version)"
journalctl -k -b --no-pager 2>/dev/null | grep -qi "Low-power S0 idle used by default" && ok "kernel uses Low-power S0 idle (LPS0 present)" || warn "no 'Low-power S0 idle used by default' line in this boot's log (BIOS lacks LPS0, or log rotated)"
[ -e /sys/module/acpi/parameters/sleep_no_lps0 ] && [ "$(rd /sys/module/acpi/parameters/sleep_no_lps0)" = Y ] && warn "acpi.sleep_no_lps0=Y"
echo "  suspend_stats: success=$(rd /sys/power/suspend_stats/success) fail=$(rd /sys/power/suspend_stats/fail) max_hw_sleep=$(rd /sys/power/suspend_stats/max_hw_sleep)"
if [ -n "$(rd /sys/power/suspend_stats/last_failed_dev)" ]; then warn "last failed device: $(rd /sys/power/suspend_stats/last_failed_dev) step $(rd /sys/power/suspend_stats/last_failed_step) errno $(rd /sys/power/suspend_stats/last_failed_errno)"; fi
echo "  enabled ACPI wake sources (/proc/acpi/wakeup):"; awk '$3=="*enabled"{print "    "$1" "$4}' /proc/acpi/wakeup
echo "  enabled wake devices in sysfs:"
for f in $(find /sys/devices -name wakeup -path '*power*' 2>/dev/null); do [ "$(rd $f)" = enabled ] && echo "    $(dirname $(dirname $f) | sed 's|/sys/devices/||' | rev | cut -d/ -f1-2 | rev)"; done
if command -v systemd-inhibit >/dev/null; then
  inh=$(systemd-inhibit --list --no-legend --mode=block 2>/dev/null | awk '$0 ~ /sleep|idle/')
  [ -n "$inh" ] && warn "something blocks sleep:" && echo "$inh" | sed 's/^/    /'
fi
for n in $(ls /sys/class/nvme 2>/dev/null); do
  echo "  $n: $(rd /sys/class/nvme/$n/model | xargs), power state $(rd /sys/class/nvme/$n/power_state 2>/dev/null)"
done
[ -n "$bat" ] || warn "no battery found"
if [ "$(rd $bat/status)" != "Discharging" ]; then warn "battery is $(rd $bat/status): unplug the charger for a drain figure"; fi

if [ "$AMD" = 1 ]; then
  hdr "AMD amd-s2idle"
  if command -v amd-s2idle >/dev/null; then
    amd-s2idle test --count 1 --duration "$SECS" --force || warn "amd-s2idle reported problems (see its report above)"
  else
    warn "amd-s2idle not installed (pip install amd-debug-tools); skipped"
  fi
fi
[ "$DRY" = 1 ] && { echo; echo "dry run: $WARNS warning(s)"; exit 0; }

hdr "Suspend for ${SECS}s"
dbg_was=$(rd /sys/power/pm_debug_messages); echo 1 > /sys/power/pm_debug_messages 2>/dev/null
trap 'echo "${dbg_was:-0}" > /sys/power/pm_debug_messages 2>/dev/null' EXIT
w0=$(wakecounts); e0=$(energy); h0=$(hw_us); r0=$(pmc_resid); ok0=$(rd /sys/power/suspend_stats/success)
if [ -t 1 ]; then echo "  the screen will go dark; press the power button to end early"; fi
rtcwake -m no -s "$SECS" >/dev/null
t0=$(date +%s.%N); marker=$(date '+%Y-%m-%d %H:%M:%S')
systemctl suspend
t1=$(date +%s.%N)   # systemctl returns after resume
sleep 1
e1=$(energy); h1=$(hw_us); r1=$(pmc_resid); ok1=$(rd /sys/power/suspend_stats/success)
w1=$(wakecounts)
elapsed=$(awk -v a="$t0" -v b="$t1" 'BEGIN{printf "%.1f", b-a}')

hdr "Result"
if [ "$ok1" = "$ok0" ]; then warn "suspend_stats success did not increase: the suspend failed or was aborted"; journalctl -k --since "$marker" --no-pager | grep -E "PM:|suspend|failed" | tail -15 | sed 's/^/    /'; fi
echo "  slept ${elapsed}s wall clock (asked for ${SECS}s)"
awk -v e="$elapsed" -v s="$SECS" 'BEGIN{ if (e < s*0.9) printf "  WARN  woke early: %.0fs of %ss\n", e, s }'

# Residency: last_hw_sleep is microseconds in the deepest state during the last cycle.
if [ "${h1:-0}" -gt 0 ] 2>/dev/null; then
  pct=$(awk -v h="${h1:-0}" -v e="$elapsed" 'BEGIN{printf "%.1f", 100*h/1e6/e}')
  echo "  s0ix hardware sleep: $(awk -v h="$h1" 'BEGIN{printf "%.1f", h/1e6}')s = ${pct}% of the sleep (suspend_stats/last_hw_sleep)"
  awk -v p="$pct" 'BEGIN{exit !(p>=90)}' && ok "residency 90% or better" || warn "residency below 90%: the SoC was not in s0ix for most of the sleep"
elif journalctl -k --since "$marker" --no-pager 2>/dev/null | grep -q "ioremap error"; then
  warn "last_hw_sleep is 0 because amd_pmc could not map its SMU table (ioremap error: ryzen_smu holds it cached; rare-pmc-first.service fixes the order at boot). Trust the PMC s0ix residency line below, not this one"
else
  warn "last_hw_sleep is 0: the SoC never reached s0ix (typical culprits: NVMe not in D3, a USB device or GPU not idle, firmware without LPS0)"
fi
[ -n "$r1" ] && echo "  PMC s0ix residency: $(awk -v a="${r1:-0}" 'BEGIN{printf "%.1f", a/1e6}')s (debugfs; previous cycle $(awk -v a="${r0:-0}" 'BEGIN{printf "%.1f", a/1e6}')s)"
[ -r $DBG/s0ix_stats ] && sed 's/^/    /' $DBG/s0ix_stats | head -8

echo "  wake source:"
journalctl -k --since "$marker" --no-pager 2>/dev/null | grep -iE "wakeup|wake up|Triggering|IRQ [0-9]+|PM: Wakeup|s0ix|hw sleep|Last suspend|amd_pmc" | sed 's/^/    /' | head -12
diff <(echo "$w0") <(echo "$w1") | awk '/^>/ {$1=""; print "    sysfs wakeup counter moved:" $0}' | head -8
elapsed_ok=$(awk -v e="$elapsed" -v s="$SECS" 'BEGIN{print (e >= s-3 && e <= s+8) ? 1 : 0}')
[ "$elapsed_ok" = 1 ] && echo "  (duration matches the RTC alarm: woken by rtcwake)" || echo "  (duration differs from the RTC alarm: woken by something else, see above)"

echo "  resume timing (journal):"
journalctl --since "$marker" --no-pager -o short-precise 2>/dev/null \
  | grep -E "PM: suspend (entry|exit)|System returned from sleep|Wake up from|Freezing user space|Restarting tasks|resumed after|rare-fand|NetworkManager.*state is now|manager: NetworkManager state|device \(wl.*(activated|disconnected)|gamescope.*(resume|wake)" \
  | sed 's/^/    /' | cut -c1-170 | head -24

hdr "After resume"
state() { systemctl is-active "$1" 2>/dev/null; }
echo "  rare-fand: $(state rare-fand)   inputplumber: $(state inputplumber)   NetworkManager: $(state NetworkManager)   bluetooth: $(state bluetooth)"
# Wi-Fi: seconds until NetworkManager says connected (it may already be).
if command -v nmcli >/dev/null; then
  n=0
  until nmcli -t -f STATE g 2>/dev/null | grep -q '^connected'; do sleep 1; n=$((n + 1)); [ $n -ge 30 ] && break; done
  [ $n -ge 30 ] && warn "Wi-Fi not connected 30 s after resume" || ok "Wi-Fi connected (waited $n s after suspend returned)"
fi
js=$(ls /dev/input/js* 2>/dev/null | wc -l); [ "$js" -gt 0 ] && ok "$js controller device(s) present" || warn "no /dev/input/js*: controller not back (InputPlumber?)"
lsusb 2>/dev/null | grep -qiE "xbox|microsoft" && ok "gamepad on USB" || warn "built-in gamepad not on USB after resume"
pgrep -x gamescope >/dev/null && ok "gamescope running" || warn "gamescope not running (desktop session?)"
for c in /sys/class/drm/card*-eDP-*; do [ -e "$c" ] && echo "  panel $(basename $c): $(rd $c/status), $(rd $c/enabled), dpms $(rd $c/dpms)"; done
journalctl -k --since "$marker" --no-pager -p warning 2>/dev/null | grep -vE "Bluetooth: hci0: (unexpected|Opcode)" | head -12 | sed 's/^/  kernel: /'

hdr "Battery"
status=$(rd "$bat/status")
if [ -n "$bat" ] && [ "$status" != Charging ] && [ "$status" != Full ]; then
  awk -v a="$e0" -v b="$e1" -v t="$elapsed" -v f="$(full)" 'BEGIN{
    d=(a-b)/1e6; h=t/3600; if (h<=0) exit;
    printf "  drained %.3f Wh in %.0f s = %.3f Wh/h (%.2f W) = %.2f %%/h of full\n", d, t, d/h, d/h, 100*(d/h)/(f/1e6);
    if (t < 300) print "  (under 5 minutes: gauge resolution makes this rough; use -s 900 or more)";
    if (d/h > 0.9) print "  WARN  more than 0.9 W asleep: expect s0ix not to be reached or a device awake";
  }'
else
  echo "  on the charger: no drain figure"
fi
echo; echo "$WARNS warning(s)"
exit 0
