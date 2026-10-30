# Journeys: automated experience tests on the real SuiPlay

`run.sh` drives the device through every journey in `journeys/` the way a player would, records what
gamescope composites, and writes a pass/fail report per step with clips of the failures.

```
tools/journeys/run.sh check                         # read-only preflight, changes nothing
tools/journeys/run.sh run --i-have-a-window         # all journeys
tools/journeys/run.sh run --i-have-a-window --tags core --sudo
tools/journeys/run.sh abort                         # or Ctrl-C in run.sh, or touch the device
tools/journeys/run.sh selftest                      # proves the analysis, no device
tools/journeys/run.sh lint                          # checks every journey file
```

Reports land in `bench/journeys/<run id>/report.html` (and `report.md`, `results.json`, `clips/`).

## How it works

| Part | How |
| --- | --- |
| Input | InputPlumber `CompositeDevice.SendEvent` over D-Bus: the same composite device the physical buttons feed, so the launcher (gilrs), Steam and games see the target Xbox pad exactly as for a real press. If InputPlumber fails (0.83 can time out on SendEvent), the runner switches to a virtual uinput pad on its own, mid-run included, and logs `pad_fallback`; `--ip-only` forbids that. `key`/`type` steps use xdotool. |
| Capture | `pipewiresrc` on gamescope's `gamescope` node: every composited frame, 160x100 gray thumbnails with a CLOCK_MONOTONIC timestamp per frame, plus a VA-API H.264 `video.mkv` for clips. |
| State | `GAMESCOPE_FOCUSED_APP` (xprop -spy), the launcher's journey probe (`$XDG_RUNTIME_DIR/rare-journey.log`, see below), amdgpu `gpu_metrics` at 10 Hz (APU, CPU cores, GFX watts), coredumpctl, panic logs and process exits. |
| Analysis | Per step, from the input's timestamp: response (first changed frame), settle, black / blank / flash / stale frames, the longest gap between two moving frames, focus changes, crashes, power. The flash test is the `flash/analyze.py` criterion: 1 or 2 frames that differ from both neighbours while the neighbours agree. |

Latency is input to the first changed composited frame (gamescope's output, before scanout).

## Safety

- Runs only with `--i-have-a-window`.
- **Idle guard (mandatory):** before starting, no physical input for 5 minutes (pad, keys and power
  button on evdev without grabbing, i8042 and Goodix touchscreen interrupt counters) with the launcher in
  front (`GAMESCOPE_FOCUSED_APP` 1380012613). `--wait-idle MIN` keeps waiting for that.
- **Any physical input while running aborts at once** (anything on the pads that is not the echo of an
  injected press, a touch, a key). Holding L3 + R3 aborts too, as do Ctrl-C in run.sh and `run.sh abort`.
- **The guard is a hard dependency.** Every press, stick move and key goes through `Injector.gate`: it
  goes out only while the guard thread is alive with a heartbeat under 1 s old. If the guard exits for
  any reason (a crash included) the run aborts at once; if it stalls, within 1 s. After that, and after
  the owner's input, nothing more is pressed (button releases only): cleanup restores through the probe
  and by stopping what the journey started. `flash/` works the same way: `guard-run.sh` (`beat.py
  supervise`) runs `idle.py` with a heartbeat file, `eikey.py`, `pad.py` and `capture.sh` send nothing
  without a fresh one, and the guard exiting, stalling or being killed stops every injector and, while
  `~/flashcap/ARMED` exists, runs `restore.sh`. `beat.py stop` ends it on purpose. Selftest 3c and 3d
  kill and stall the guard mid-run.
- Every journey has a hard timeout (`timeout_s`), the run has `--max-minutes` (45).
- After every journey, and after any abort: release every button, quit what the journey started
  (the launcher's quit, then stopping the new app scopes and Steam game processes), close sheets and go
  Home with B (at most 4 presses) and the probe's `close` and `tab:home`. Cleanup and the probe
  wake-up never press Guide: it toggles the Rare menu. Home is then confirmed: the launcher is the
  focused app and a `screen` line written after the runner's `state` command says `tab=home overlay=-`
  (`home_confirmed` in events.jsonl). If that fails the run stops. Probe commands queue: a second
  command waits for the launcher to take the first instead of overwriting it.
- A is never pressed on Power off, Restart, Buy, Sell, Send, Sign, Swap and the like (needs the probe).
  Select alone is refused (it opens Steam). Volume is put back at the end.
- Launcher settings are saved before the run; the report lists any that changed and
  `run.sh restore-config <run id>` puts them back.
- Sleep/wake: `--sudo` asks for the password on the Mac and arms the RTC over ssh when the runner gets
  there (nothing is written on the device); `--attended` waits for the owner to press power.

## The journey probe (launcher)

`launcher/src/nav/journey.ts` + `src-tauri/src/journey.rs`. Off and free unless
`$XDG_RUNTIME_DIR/rare-journey` exists (the runner creates it). Then the UI logs `screen tab=… overlay=…`,
`focus <element>`, `ingame mode=…`, `pad <button>`, `painted`, `toast …`, stamped with CLOCK_MONOTONIC,
and takes setup commands (`tab:`, `open:game:`, `open:app:`, `close`, `quit:all`). Journeys with
`needs: [probe]` are skipped on a launcher without it. To test a launcher build before it ships, put its
path in `$XDG_RUNTIME_DIR/rare-launcher-bin` and restart only the launcher (rare-session-client).

## Writing a journey

One YAML file per journey in `journeys/` (any subfolder). `${name}` reads from `journeys/_targets.yaml`.

```yaml
name: tabs                     # lowercase, unique
title: Every top tab
tags: [core, launcher]
needs: [probe]                 # probe, network, steam, root_or_attended, session_client, video
timeout_s: 90
start: home                    # home (default: cleanup first) or as-is
steps:
  - press: rb                  # a b x y lb rb lt rt up down left right guide select start l3 r3 volume_up volume_down
    expect: {screen: {tab: library}, respond_ms: 150, settle_ms: 900}
  - press: [select, start]     # a chord; hold_ms (default 80), repeat, gap_ms
    hold_ms: 900
  - press: exit                # macros: exit (Select+Start hold), qam (Guide+A), guide_hold, panel
  - goto: open:game:${steam_game}   # setup through the probe, not timed (tab:<tab> or a bare tab name)
  - nav: {press: down, until: {focus: '"Sleep"'}, max: 12}
  - wait_for: {focus_app: not_launcher, timeout_ms: 60000}
  - wait: 2000
  - type: "fan"                # xdotool; key: Return
  - sleep_wake: {seconds: 20}  # presses A on the focused Sleep, waits for suspend and resume
  - restart_launcher: true     # rare-session-client starts it again
```

Every action step waits until the screen settles (300 ms without a change, at most 3 s;
`settle_timeout_ms`, `quiet_ms`) or `after_ms`. A `wait_for` next to an action is its post-condition.

`expect` (all optional): `respond_ms`, `settle_ms`, `screen` (`tab`, `overlay`, `running`),
`focus` (regex on the focused element), `focus_app` (`launcher`, `not_launcher` or an id), `change`
(true/false), `hitch_ms` (default 100), `no` (default `[black, blank, flash, stale]`), `fps_min`,
`power_max_w`, `max_focus_changes`. `allow: [black]` on a step that legitimately goes through black
(a game starting): the black frames are still reported as warnings. `wait_for` conditions:
`focus_app`, `screen`, `focus`, `ingame` (`{mode: running}`), `settled` (ms), `changed`, `probe_text`,
`process` (regex on command lines), `timeout_ms`. `continue_on_fail: true` keeps going after a failure.

## Files

- `run.sh` Mac driver. `analyze.py` report from a run, `--scan VIDEO` for any capture, `--lint`.
- `device/runner.py` runs on the device (`--mock` anywhere), `device/check.py` read-only preflight,
  `device/clips.py` cuts mp4 clips with ffmpeg.
- `jlib/` journey schema, frame metrics, analysis, report. Python 3.9+, stdlib only.
- `selftest.py` synthetic captures with injected black, blank, flash, stale frames, hitches and slow
  responses (and clean crossfades, slides and repeats that must pass), an mp4 round trip (needs ffmpeg
  or `pip install imageio-ffmpeg`), the runner against a simulated launcher, and the abort path.
- `flash/` the standalone full-rate flash check (640x400 RGB, keys over xdotool or libei), with the
  idle guard (`guard-run.sh`, `beat.py`, `idle.py`) its keys depend on.
