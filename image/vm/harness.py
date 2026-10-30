#!/usr/bin/env python3
"""VM test harness for Rare OS images (CI and the build machine).

Boots a raw disk in QEMU (KVM when available), takes screenshots through QMP every
few seconds, talks to the guest over SSH (root key injected at install time) and waits
for conditions such as "the launcher is running".

  harness.py boot   --disk D --name N --out DIR [--wait-launcher SECS] [--keep]
  harness.py ssh    --cmd "..."                 run a command in the running guest
  harness.py shot   --out FILE                  one screenshot of the running guest
  harness.py reboot --name N --out DIR [--wait-launcher SECS | --wait-ssh SECS]
  harness.py stop
  harness.py keys   --keys "ret down ret" --out DIR --name N [--gap SECS]
                                                send key presses (QMP qcodes; "text:abc" types abc),
                                                with a screenshot after each

Secure Boot: RARE_VM_SECUREBOOT=1 boots with OVMF's Secure Boot firmware and the Microsoft keys
enrolled (what a PC or handheld that shipped with Windows has), so only signed boot chains start.
"""
import argparse, json, os, shutil, socket, subprocess, sys, time

STATE = os.environ.get("RARE_VM_STATE", "/tmp/rare-vm")
KEY = os.environ.get("RARE_VM_KEY", os.path.join(STATE, "id_ed25519"))
PORT = int(os.environ.get("RARE_VM_SSH_PORT", "2222"))
# Rare OS refuses root over SSH (PermitRootLogin no) once it runs: tests then log in as the device user.
USER = os.environ.get("RARE_VM_SSH_USER", "root")


def log(*a):
    print("[vm]", *a, flush=True)


# ---------------- QMP ----------------
class QMP:
    def __init__(self, path):
        self.s = socket.socket(socket.AF_UNIX)
        self.s.connect(path)
        self.f = self.s.makefile("rwb")
        self._read()  # greeting
        self.cmd("qmp_capabilities")

    def _read(self):
        while True:
            line = self.f.readline()
            if not line:
                raise RuntimeError("QMP closed")
            msg = json.loads(line)
            if "event" not in msg:
                return msg

    def close(self):
        # the makefile() handle holds the socket open too: close both, or QEMU keeps serving this client
        self.f.close()
        self.s.close()

    def cmd(self, name, **args):
        self.f.write(json.dumps({"execute": name, "arguments": args}).encode() + b"\n")
        self.f.flush()
        r = self._read()
        if "error" in r:
            raise RuntimeError(f"{name}: {r['error']}")
        return r.get("return")


def qmp():
    return QMP(os.path.join(STATE, "qmp.sock"))


def screenshot(path):
    try:
        q = qmp()
        try:
            q.cmd("screendump", filename=path, format="png")
        except RuntimeError:
            ppm = path[:-4] + ".ppm"
            q.cmd("screendump", filename=ppm)
            subprocess.run(["convert", ppm, path], check=False)
            os.path.exists(ppm) and os.remove(ppm)
        return os.path.exists(path)
    except Exception as e:  # noqa: BLE001
        log("screenshot failed:", e)
        return False


# ---------------- SSH ----------------
def ssh(cmd, timeout=60, check=False):
    args = [
        "ssh", "-i", KEY, "-p", str(PORT), "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
        "-o", "ConnectTimeout=5", "-o", "BatchMode=yes", "-o", "LogLevel=ERROR", f"{USER}@127.0.0.1", cmd,
    ]
    cid = os.environ.get("RARE_VM_VSOCK")
    if cid:
        # over the vsock (systemd-ssh-generator's sshd-vsock.socket): works with the guest offline
        args[-2:-1] = ["-o", f"ProxyCommand=socat - VSOCK-CONNECT:{cid}:22", "root@vm"]
    try:
        r = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return 124, ""
    if check and r.returncode != 0:
        raise RuntimeError(f"ssh {cmd!r} failed: {r.stderr}")
    return r.returncode, r.stdout + r.stderr


# ---------------- QEMU ----------------
def qemu_cmd(disk):
    kvm = os.path.exists("/dev/kvm") and os.access("/dev/kvm", os.W_OK)
    mem_total = int(open("/proc/meminfo").read().split()[1]) // 1024
    mem = max(3072, min(8192, mem_total - 2048))
    cpus = max(2, (os.cpu_count() or 2))
    if os.environ.get("RARE_VM_SECUREBOOT") == "1":
        code = next(p for p in ["/usr/share/OVMF/OVMF_CODE_4M.secboot.fd", "/usr/share/OVMF/OVMF_CODE_4M.ms.fd", "/usr/share/edk2/ovmf/OVMF_CODE.secboot.fd"] if os.path.exists(p))
        vars_src = next(p for p in ["/usr/share/OVMF/OVMF_VARS_4M.ms.fd", "/usr/share/edk2/ovmf/OVMF_VARS.secboot.fd"] if os.path.exists(p))
    else:
        code = next(p for p in ["/usr/share/OVMF/OVMF_CODE_4M.fd", "/usr/share/OVMF/OVMF_CODE.fd", "/usr/share/edk2/ovmf/OVMF_CODE.fd"] if os.path.exists(p))
        vars_src = next(p for p in ["/usr/share/OVMF/OVMF_VARS_4M.fd", "/usr/share/OVMF/OVMF_VARS.fd", "/usr/share/edk2/ovmf/OVMF_VARS.fd"] if os.path.exists(p))
    vars_file = os.path.join(STATE, f"vars-{os.path.basename(disk)}.fd")
    if not os.path.exists(vars_file):
        shutil.copy(vars_src, vars_file)
    extra = []
    # e.g. "type=1,manufacturer=Mysten Labs,, Inc.,product=SuiPlay0X1" to look like a SuiPlay0X1
    smbios = os.environ.get("RARE_VM_SMBIOS")
    if smbios:
        extra += ["-smbios", smbios]
    # root's SSH key as a systemd credential (systemd-tmpfiles provisions ssh.authorized_keys.root):
    # reaches a system installed from the ISO, whose installer sets no keys.
    cred = os.environ.get("RARE_VM_ROOT_KEY")
    if cred and os.path.exists(cred):
        import base64
        b64 = base64.b64encode(open(cred, "rb").read()).decode()
        extra += ["-smbios", f"type=11,value=io.systemd.credential.binary:ssh.authorized_keys.root={b64}"]
    shared = os.environ.get("RARE_VM_EXTRA_DISK")
    if shared:
        extra += ["-drive", f"file={shared},format=raw,if=virtio,readonly=on"]
    # First-install walk (walk.py): the installer ISO, a host pad handed over whole (vpad.py), a
    # vsock for SSH that keeps working while the guest's network is off, and the panel size.
    iso = os.environ.get("RARE_VM_ISO")
    if iso:
        # The disk first: blank, it falls through to the stick; installed, it boots Rare OS (as a
        # handheld does once the installer added its boot entry).
        extra += ["-drive", f"file={iso},media=cdrom,readonly=on,if=none,id=cd0", "-device", "ide-cd,drive=cd0,bootindex=2"]
    pad = os.environ.get("RARE_VM_PAD")
    if pad:
        extra += ["-device", f"virtio-input-host-pci,evdev={pad}"]
    cid = os.environ.get("RARE_VM_VSOCK")
    if cid:
        extra += ["-device", f"vhost-vsock-pci,guest-cid={cid}"]
    xres, yres = os.environ.get("RARE_VM_RES", "1280x800").split("x")
    fmt = "qcow2" if disk.endswith(".qcow2") else "raw"
    mem = int(os.environ.get("RARE_VM_MEM", mem))
    cpus = int(os.environ.get("RARE_VM_CPUS", cpus))
    log(f"accel={'kvm' if kvm else 'tcg'} mem={mem}M cpus={cpus}")
    return [
        "qemu-system-x86_64",
        "-machine", "q35,accel=" + ("kvm" if kvm else "tcg") + (",smm=on" if "secboot" in code else ""),
        *(["-global", "driver=cfi.pflash01,property=secure,value=on"] if "secboot" in code else []),
        "-cpu", "host" if kvm else "max",
        "-m", str(mem), "-smp", str(cpus),
        "-drive", f"if=pflash,format=raw,readonly=on,file={code}",
        "-drive", f"if=pflash,format=raw,file={vars_file}",
        *(["-drive", f"file={disk},format={fmt},if=none,id=d0,cache=unsafe", "-device", "virtio-blk-pci,drive=d0,bootindex=1"] if iso else ["-drive", f"file={disk},format={fmt},if=virtio,cache=unsafe"]),
        "-device", f"virtio-vga,xres={xres},yres={yres}",
        "-display", "none",
        "-device", "qemu-xhci", "-device", "usb-tablet", "-device", "usb-kbd",
        "-netdev", f"user,id=n0,hostfwd=tcp:127.0.0.1:{PORT}-:22",
        "-device", "virtio-net-pci,netdev=n0",
        "-qmp", f"unix:{os.path.join(STATE, 'qmp.sock')},server=on,wait=off",
        "-serial", f"file:{os.path.join(STATE, 'serial-' + os.path.basename(disk) + '.log')}",
        "-pidfile", os.path.join(STATE, "qemu.pid"),
        "-daemonize",
    ] + extra


def running():
    try:
        pid = int(open(os.path.join(STATE, "qemu.pid")).read())
        os.kill(pid, 0)
        return pid
    except Exception:  # noqa: BLE001
        return None


def stop():
    pid = running()
    if pid:
        try:
            qmp().cmd("quit")
        except Exception:  # noqa: BLE001
            os.kill(pid, 9)
        for _ in range(30):
            if not running():
                break
            time.sleep(1)
    for f in ("qmp.sock", "qemu.pid"):
        p = os.path.join(STATE, f)
        os.path.exists(p) and os.remove(p)


def start(disk):
    os.makedirs(STATE, exist_ok=True)
    stop()
    subprocess.run(qemu_cmd(disk), check=True)
    for _ in range(50):
        if os.path.exists(os.path.join(STATE, "qmp.sock")):
            return
        time.sleep(0.2)
    raise RuntimeError("QEMU did not start")


# The WebKit launcher is rare-launcher; the native one runs as /usr/libexec/rare/native/rare-launcher-native
# (its process name is cut to 15 characters, so -x can't match it). The [r] keeps the shell running
# this check from matching itself.
LAUNCHER_UP = "{ pgrep -x rare-launcher || pgrep -f '/[r]are-launcher-native( |$)'; } >/dev/null && echo LAUNCHER_UP"


def wait(name, out, wait_launcher=0, wait_ssh=0, every=15):
    """Screenshots every `every` seconds until the launcher runs (or SSH answers)."""
    os.makedirs(out, exist_ok=True)
    limit = wait_launcher or wait_ssh
    t0 = time.time()
    n = 0
    while time.time() - t0 < limit:
        n += 1
        screenshot(os.path.join(out, f"{name}-{n:03d}.png"))
        if not running():
            log("QEMU exited")
            return False
        code, outp = ssh(LAUNCHER_UP if wait_launcher else "true", timeout=20)
        if wait_launcher and "LAUNCHER_UP" in outp:
            log(f"launcher running after {int(time.time() - t0)} s; letting it render")
            time.sleep(int(os.environ.get("RARE_VM_RENDER_WAIT", "45")))
            screenshot(os.path.join(out, f"{name}-launcher.png"))
            return True
        if wait_ssh and code == 0:
            log(f"ssh up after {int(time.time() - t0)} s")
            time.sleep(10)
            screenshot(os.path.join(out, f"{name}-ready.png"))
            return True
        time.sleep(every)
    log(f"timed out after {limit} s")
    return False


def send_keys(spec, out, name, gap):
    """spec: space-separated QMP qcodes ("ret", "down", "spc", "shift-a" for a chord) or "text:abc"."""
    os.makedirs(out, exist_ok=True)
    presses = []
    for tok in spec.split():
        if tok.startswith("text:"):
            presses += [[c] for c in tok[5:]]
        else:
            presses.append(tok.split("-"))
    for n, keys in enumerate(presses, 1):
        # QEMU's QMP socket takes one client at a time: close it before the screenshot opens its own.
        q = qmp()
        q.cmd("send-key", keys=[{"type": "qcode", "data": k} for k in keys])
        q.close()
        time.sleep(gap)
        screenshot(os.path.join(out, f"{name}-{n:02d}-{'-'.join(keys)}.png"))


def collect(name, out):
    for label, cmd in [
        ("journal", "journalctl -b --no-pager -o short-monotonic | tail -n 4000"),
        ("status", "bootc status 2>&1; rpm-ostree status 2>&1; cat /etc/os-release; systemctl --failed --no-pager; loginctl list-sessions --no-pager; ps aux | grep -E 'rare|cage|gamescope|sddm' | grep -v grep"),
    ]:
        _, o = ssh(cmd, timeout=120)
        open(os.path.join(out, f"{name}-{label}.txt"), "w").write(o)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("action", choices=["boot", "ssh", "shot", "reboot", "stop", "collect", "keys", "wait"])
    ap.add_argument("--keys")
    ap.add_argument("--gap", type=float, default=2.0)
    ap.add_argument("--disk")
    ap.add_argument("--name", default="vm")
    ap.add_argument("--out", default=os.path.join(STATE, "shots"))
    ap.add_argument("--wait-launcher", type=int, default=0)
    ap.add_argument("--wait-ssh", type=int, default=0)
    ap.add_argument("--cmd")
    ap.add_argument("--timeout", type=int, default=3600)
    a = ap.parse_args()

    if a.action == "boot":
        start(a.disk)
        ok = wait(a.name, a.out, a.wait_launcher, a.wait_ssh)
        collect(a.name, a.out)
        sys.exit(0 if ok else 1)
    if a.action == "reboot":
        ssh("systemctl reboot", timeout=15)
        time.sleep(20)
        ok = wait(a.name, a.out, a.wait_launcher, a.wait_ssh)
        collect(a.name, a.out)
        sys.exit(0 if ok else 1)
    if a.action == "ssh":
        code, o = ssh(a.cmd, timeout=a.timeout)
        print(o)
        sys.exit(code)
    if a.action == "keys":
        send_keys(a.keys, a.out, a.name, a.gap)
        sys.exit(0)
    if a.action == "wait":
        ok = wait(a.name, a.out, a.wait_launcher, a.wait_ssh)
        collect(a.name, a.out)
        sys.exit(0 if ok else 1)
    if a.action == "shot":
        sys.exit(0 if screenshot(a.out) else 1)
    if a.action == "collect":
        collect(a.name, a.out)
    if a.action == "stop":
        stop()


if __name__ == "__main__":
    main()
