#!/usr/bin/env python3
"""Sends keys to whatever gamescope has focused, through gamescope's libei socket (gamescope-0-ei).

xdotool only reaches X11 clients of Xwayland; a native Wayland launcher (RARE_WEBKIT=wayland) never
sees its keys. gamescope's EIS server (InputEmulation.cpp) feeds keys into its own Wayland seat, which
delivers them to the focused window, Xwayland or xdg alike, so this drives both modes the same way.

  eikey.py [--gap S] [--only APPID] key...    keys: e q Up Down Left Right Return Escape BackSpace,
                                              a number (evdev code), or wait:<seconds>
  --only APPID: send each key only while GAMESCOPE_FOCUSED_APP is APPID; stop otherwise.
Needs libei.so.1 (libei package). Prints one line per key: "<unix time> <key>".
Needs a live idle guard (guard-run.sh, beat.py): before every key the guard's heartbeat must be under
1 s old, else it sends nothing more, calls beat.lost() (stop, restore while armed) and exits 4.
RARE_KEYS_DRY=1 prints the keys without libei (selftest).
"""
import ctypes as C
import os, select, subprocess, sys, time

KEYS = {"e": 18, "q": 16, "Up": 103, "Down": 108, "Left": 105, "Right": 106, "Return": 28, "Escape": 1, "BackSpace": 14, "Tab": 15}
EV_CONNECT, EV_DISCONNECT, EV_SEAT_ADDED, EV_DEVICE_ADDED, EV_DEVICE_RESUMED = 1, 2, 3, 5, 8
CAP_KEYBOARD = 1 << 2

ei = None


def load_ei():
    global ei
    ei = C.CDLL("libei.so.1")
    ei.ei_new_sender.restype = C.c_void_p
    ei.ei_new_sender.argtypes = [C.c_void_p]
    ei.ei_configure_name.argtypes = [C.c_void_p, C.c_char_p]
    ei.ei_setup_backend_socket.argtypes = [C.c_void_p, C.c_char_p]
    ei.ei_get_fd.argtypes = [C.c_void_p]
    ei.ei_dispatch.argtypes = [C.c_void_p]
    ei.ei_get_event.restype = C.c_void_p
    ei.ei_get_event.argtypes = [C.c_void_p]
    ei.ei_event_get_type.argtypes = [C.c_void_p]
    ei.ei_event_get_seat.restype = C.c_void_p
    ei.ei_event_get_seat.argtypes = [C.c_void_p]
    ei.ei_event_get_device.restype = C.c_void_p
    ei.ei_event_get_device.argtypes = [C.c_void_p]
    ei.ei_event_unref.restype = C.c_void_p
    ei.ei_event_unref.argtypes = [C.c_void_p]
    ei.ei_device_ref.restype = C.c_void_p
    ei.ei_device_ref.argtypes = [C.c_void_p]
    ei.ei_device_has_capability.argtypes = [C.c_void_p, C.c_int]
    ei.ei_device_start_emulating.argtypes = [C.c_void_p, C.c_uint32]
    ei.ei_device_stop_emulating.argtypes = [C.c_void_p]
    ei.ei_device_keyboard_key.argtypes = [C.c_void_p, C.c_uint32, C.c_bool]
    ei.ei_device_frame.argtypes = [C.c_void_p, C.c_uint64]
    ei.ei_now.restype = C.c_uint64
    ei.ei_now.argtypes = [C.c_void_p]


def load_beat():
    """beat.py next to this file (imported by path: python3 -I leaves the script dir off sys.path)."""
    import importlib.util
    spec = importlib.util.spec_from_file_location("rare_beat", os.path.join(os.path.dirname(os.path.realpath(__file__)), "beat.py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def focused_app():
    try:
        out = subprocess.run(["xprop", "-display", os.environ.get("DISPLAY", ":0"), "-root", "GAMESCOPE_FOCUSED_APP"], capture_output=True, text=True, timeout=3).stdout
        return out.split("=")[-1].strip()
    except Exception:
        return ""


def main():
    args = sys.argv[1:]
    gap, only = 1.2, None
    while args and args[0].startswith("--"):
        if args[0] == "--gap":
            gap = float(args[1]); args = args[2:]
        elif args[0] == "--only":
            only = args[1]; args = args[2:]
        else:
            sys.exit(f"unknown option {args[0]}")
    beat = load_beat()
    dry = os.environ.get("RARE_KEYS_DRY") == "1"

    def guard_ok():
        why = beat.check()
        if why:
            print(f"{time.time():.3f} stop: {why}", flush=True)
            beat.lost(why)
            sys.exit(4)

    def pause(secs):  # sleeps in short slices so a lost guard stops the run within ~50 ms
        end = time.monotonic() + secs
        while time.monotonic() < end:
            guard_ok()
            time.sleep(min(0.05, max(0.0, end - time.monotonic())))

    guard_ok()
    if dry:
        for k in args:
            if k.startswith("wait:"):
                pause(float(k[5:]))
                continue
            guard_ok()
            print(f"{time.time():.3f} {k}", flush=True)
            pause(gap)
        return
    load_ei()
    ctx = ei.ei_new_sender(None)
    ei.ei_configure_name(ctx, b"rare-flash-keys")
    sock = os.environ.get("RARE_EI_SOCKET", (os.environ.get("GAMESCOPE_WAYLAND_DISPLAY") or "gamescope-0") + "-ei").encode()
    if ei.ei_setup_backend_socket(ctx, sock) != 0:
        sys.exit(f"eikey: cannot connect to {sock.decode()}")
    fd = ei.ei_get_fd(ctx)
    dev = None
    seq = 0
    t_end = time.time() + 5
    while dev is None and time.time() < t_end:
        select.select([fd], [], [], 0.2)
        ei.ei_dispatch(ctx)
        while True:
            ev = ei.ei_get_event(ctx)
            if not ev:
                break
            t = ei.ei_event_get_type(ev)
            if t == EV_DISCONNECT:
                sys.exit("eikey: gamescope disconnected us")
            if t == EV_SEAT_ADDED:
                ei.ei_seat_bind_capabilities(C.c_void_p(ei.ei_event_get_seat(ev)), C.c_int(CAP_KEYBOARD), C.c_void_p(None))
            if t == EV_DEVICE_RESUMED:
                d = ei.ei_event_get_device(ev)
                if ei.ei_device_has_capability(d, CAP_KEYBOARD):
                    dev = ei.ei_device_ref(d)
            ei.ei_event_unref(ev)
    if dev is None:
        sys.exit("eikey: no keyboard device from gamescope")

    def pump():
        ei.ei_dispatch(ctx)
        while True:
            ev = ei.ei_get_event(ctx)
            if not ev:
                break
            ei.ei_event_unref(ev)

    for k in args:
        if k.startswith("wait:"):
            pause(float(k[5:]))
            continue
        guard_ok()
        if only and focused_app() != only:
            print(f"{time.time():.3f} stop: focused app {focused_app()!r}, not {only}", flush=True)
            sys.exit(3)
        code = KEYS.get(k) or int(k)
        seq += 1
        ei.ei_device_start_emulating(dev, seq)
        ei.ei_device_keyboard_key(dev, code, True)
        ei.ei_device_frame(dev, ei.ei_now(ctx))
        pump()
        time.sleep(0.04)
        ei.ei_device_keyboard_key(dev, code, False)
        ei.ei_device_frame(dev, ei.ei_now(ctx))
        ei.ei_device_stop_emulating(dev)
        pump()
        print(f"{time.time():.3f} {k}", flush=True)
        pause(gap)


if __name__ == "__main__":
    main()
