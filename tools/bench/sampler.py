#!/usr/bin/env python3
"""Device-side sampler for the Rare OS bench suite. Runs ON the handheld, stdlib only.
Read only: sysfs, /proc, the gamescope stats pipe and nmcli state. Writes only under --out (in /tmp).
  samples.csv  one row per --interval (power, battery, thermals, CPU, brightness, Wi-Fi)
  fps.csv      one row per fps line seen on the gamescope stats pipe (t,fps,focus)
  meta.json    start conditions;  done  written when the run ends (reason inside)
Stops at --secs, at --target-pct (battery capacity, only while discharging), or when the file STOP exists.
"""
import argparse, glob, json, os, subprocess, sys, threading, time

ap = argparse.ArgumentParser()
ap.add_argument("--out", required=True)
ap.add_argument("--interval", type=float, default=1.0)
ap.add_argument("--secs", type=float, default=0)
ap.add_argument("--target-pct", type=float, default=0)
ap.add_argument("--pipe", action="store_true", help="also read the gamescope stats pipe")
a = ap.parse_args()
os.makedirs(a.out, exist_ok=True)

def rd(p):
    try:
        with open(p) as f: return f.read().strip()
    except Exception: return ""
def num(s, div=1.0, fmt="%.3f"):
    try: return fmt % (float(s) / div)
    except Exception: return ""
def hw(name):
    for h in sorted(glob.glob("/sys/class/hwmon/hwmon*")):
        if rd(h + "/name") == name: return h
    return None

BAT = "/sys/class/power_supply/BAT0"
apu = (glob.glob("/sys/class/drm/card*/device/hwmon/hwmon*/power1_average") or [""])[0]
k10, gpu = hw("k10temp"), hw("amdgpu")
fans = glob.glob("/sys/class/hwmon/hwmon*/fan*_input") + glob.glob("/sys/class/drm/card*/device/hwmon/hwmon*/fan*_input")
cpus = glob.glob("/sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_cur_freq")
bl = (glob.glob("/sys/class/backlight/*") or [""])[0]
wifi_if = next((os.path.basename(p) for p in glob.glob("/sys/class/net/*") if os.path.isdir(p + "/wireless")), "")
ac_paths = [p for p in glob.glob("/sys/class/power_supply/*") if rd(p + "/type") == "Mains"]

def wifi():
    if not wifi_if: return "none"
    st = rd("/sys/class/net/%s/operstate" % wifi_if)
    try:
        o = subprocess.run(["nmcli", "-t", "-g", "GENERAL.STATE", "dev", "show", wifi_if],
                           capture_output=True, text=True, timeout=5).stdout.split("\n")
        return (st + ":" + o[0]).replace(",", ";")   # state only, no SSID
    except Exception:
        return st

def snapshot():
    return {
        "host_kernel": os.uname().release,
        "bat_energy_full_uwh": rd(BAT + "/energy_full"), "bat_energy_full_design_uwh": rd(BAT + "/energy_full_design"),
        "bat_cycle_count": rd(BAT + "/cycle_count"), "bat_temp_dc": rd(BAT + "/temp"),
        "acpitz_c": num(rd((hw("acpitz") or "/x") + "/temp1_input"), 1000),
        "brightness": rd(bl + "/brightness"), "max_brightness": rd(bl + "/max_brightness"), "backlight": os.path.basename(bl),
        "wifi": wifi(), "wifi_if": wifi_if,
        "ac_online": [rd(p + "/online") for p in ac_paths], "bat_status": rd(BAT + "/status"), "bat_capacity": rd(BAT + "/capacity"),
        "governor": rd("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        "epp": rd("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference"),
        "platform_profile": rd("/sys/firmware/acpi/platform_profile"),
        "uptime_s": rd("/proc/uptime").split(" ")[0],
    }

meta = {"start": time.time(), "interval": a.interval, "secs": a.secs, "target_pct": a.target_pct, "pid": os.getpid(), **snapshot()}
json.dump(meta, open(a.out + "/meta.json", "w"), indent=1)

fpsf = open(a.out + "/fps.csv", "w"); fpsf.write("t,fps,focus\n"); fpsf.flush()
latest = {"focus": ""}
def pipe():
    while True:
        for p in glob.glob("/run/user/%d/gamescope.*/stats.pipe" % os.getuid()):
            try:
                with open(p) as f:
                    for line in f:
                        k, _, v = line.strip().partition("=")
                        if k == "focus": latest["focus"] = v
                        elif k == "fps":
                            fpsf.write("%.3f,%s,%s\n" % (time.time(), v, latest["focus"])); fpsf.flush()
            except Exception: pass
        time.sleep(1)
if a.pipe: threading.Thread(target=pipe, daemon=True).start()

cols = "t,cap_pct,energy_uwh,power_uw,status,apu_w,tctl_c,edge_c,fan_rpm,cpu_avg_mhz,cpu_max_mhz,brightness,wifi"
sf = open(a.out + "/samples.csv", "w"); sf.write(cols + "\n"); sf.flush()
t0 = time.time(); n = 0; reason = "secs"; last_wifi = ""; last_wifi_t = 0
while True:
    n += 1
    time.sleep(max(0, t0 + n * a.interval - time.time()))
    now = time.time()
    if now - last_wifi_t >= 30 or not last_wifi:
        last_wifi, last_wifi_t = wifi(), now
    fr = [int(x) for x in map(rd, cpus) if x.isdigit()]
    st = rd(BAT + "/status"); cap = rd(BAT + "/capacity")
    sf.write(",".join([("%.3f" % now), cap, rd(BAT + "/energy_now"), rd(BAT + "/power_now"), st,
        num(rd(apu), 1e6) if apu else "", num(rd(k10 + "/temp1_input"), 1000) if k10 else "",
        num(rd(gpu + "/temp1_input"), 1000) if gpu else "", num(rd(fans[0])) if fans else "",
        "%.0f" % (sum(fr) / len(fr) / 1000) if fr else "", "%.0f" % (max(fr) / 1000) if fr else "",
        rd(bl + "/brightness"), last_wifi]) + "\n"); sf.flush()
    if os.path.exists(a.out + "/STOP"): reason = "stop-file"; break
    if a.secs and now - t0 >= a.secs: reason = "secs"; break
    if a.target_pct and st == "Discharging" and cap.isdigit() and float(cap) <= a.target_pct: reason = "target-pct"; break
meta.update({"end": time.time(), "end_snapshot": snapshot()})
json.dump(meta, open(a.out + "/meta.json", "w"), indent=1)
open(a.out + "/done", "w").write(reason + "\n")
