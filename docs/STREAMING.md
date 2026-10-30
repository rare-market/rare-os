# PC streaming

Rare Launcher plays the games on the owner's gaming PC on the SuiPlay0X1. The PC runs Sunshine (or an older GeForce Experience with GameStream); the SuiPlay runs Moonlight-Qt, started and driven by Rare. Streamed games show in Library > Streaming and, while a paired PC answers, as an "On your PC" row on Home.

Screenshots (mocked PC, both sizes): `docs/screenshots/stream-*.png`.

| Shot | What it shows |
|---|---|
| `stream-home` | Home's "On your PC" row with Sunshine's box art |
| `stream-library` | Library > Streaming, one game streaming (status bar: Streaming ELDEN RING) |
| `stream-library-empty` | No PC yet: one sentence, Set up streaming |
| `stream-setup` | PCs found on the network, one paired, one not |
| `stream-setup-moonlight` | Moonlight not installed: Get Moonlight with the Flathub command |
| `stream-pair` | Pairing: the PIN big, where to type it in Sunshine |
| `stream-game` | A PC game's page: Play and the stream quality |
| `stream-game-running` | Streaming: Resume and Quit game |
| `stream-game-busy` | The PC runs another game: Quit it and play |
| `stream-game-asleep` | The PC is asleep: Wake and play (Wake-on-LAN) |
| `stream-guide-hold` | Holding Guide: the stream in the running list |

## How it works

Code: `launcher/src-tauri/src/stream/` (native) and `launcher/src/stream/` (UI).

- **Moonlight is a separate program.** Rare installs the Flathub app `com.moonlight_stream.Moonlight` for the user (no sudo) with the Apps installer, then runs its command line. Moonlight is GPL-3.0; Rare never links it, so Rare keeps its own licence. A native `moonlight` on PATH works too.
- **Finding PCs.** Rare browses mDNS `_nvstream._tcp` itself (the `mdns-sd` crate) and asks each answer for `http://<pc>:47989/serverinfo`: name, unique id, MAC address, codecs, the game running. A PC can also be added by address.
- **Pairing.** Rare picks a 4 digit PIN and shows it big, then runs `moonlight pair <pc> --pin <PIN>` on Qt's offscreen platform, so no Moonlight window covers the PIN. The PC counts as paired once Moonlight has saved its certificate in `Moonlight.conf` (read only; Moonlight owns it). Rare then ends the hidden Moonlight, which would otherwise wait on its own "paired" dialog.
- **Games and box art.** `moonlight list <pc> --csv`, also offscreen. Moonlight fetches each app's image from Sunshine into its cache before it exits; Rare copies them to `~/.cache/rare/stream/<pc>/<app>.png` and keeps the list in `~/.config/rare/stream.json`, so the Library has them while the PC sleeps.
- **Playing.** `moonlight stream <pc> "<game>"` with the SuiPlay presets (below), in its own systemd scope under the launcher's runner as `stream:<pc uuid>:<app id>`. Resume, Quit game, the running dot, the status bar and holding Guide work as they do for games. Quit game ends Moonlight and then runs `moonlight quit <pc>`, so the game closes on the PC too.
- **Wake-on-LAN.** Moonlight's CLI has no wake action, so Rare sends the same packets Moonlight's "Wake PC" does (magic packet to the broadcast address and the PC's last addresses, ports 9, 47009, 47998 to 48010) with the MAC from serverinfo or from Moonlight.conf. Play on a sleeping PC wakes it, waits up to a minute for it to answer, then streams.
- **The PC is busy.** If the PC runs a different game, the page says so and Play becomes "Quit <game> and play", which runs `moonlight quit` first. Without it Moonlight would ask in its own window.

### The stream command

```
flatpak run com.moonlight_stream.Moonlight stream 192.168.0.20 "Cyberpunk 2077" \
  --resolution 1920x1200 --fps 60 --bitrate 20000 --video-codec AV1 \
  --display-mode borderless --audio-config stereo \
  --no-quit-after --no-background-gamepad --no-swap-gamepad-buttons --keep-awake
```

| Option | Why |
|---|---|
| `--resolution 1920x1200` or `1280x800` | The panel, or half the pixels for busy Wi-Fi and less decode work. Both 16:10, 60 fps |
| `--bitrate` 10, 20, 30 or 50 Mbps | 20 is the default; 30 to 50 needs Wi-Fi 6/6E near the router |
| `--video-codec` | Auto picks AV1, then HEVC, then H.264: the first the SuiPlay decodes (`vainfo`; the Radeon 780M decodes AV1 and HEVC) and the PC encodes (serverinfo's codec bits; AV1 needs an RTX 40, RX 7000 or Arc). Unknown on either side: Moonlight decides |
| `--display-mode borderless` | gamescope scales it; no display mode change at 1280 x 800 |
| `--no-quit-after` | The game keeps running on the PC if Wi-Fi drops, so Resume picks it up |
| `--no-background-gamepad` | Once Guide brings Rare to the front, the pad drives Rare and not the PC |
| `--no-swap-gamepad-buttons` | The SuiPlay's pad is Xbox layout; Moonlight (SDL) already maps its X and Y right, so nothing may flip them |

### The controller

Moonlight reads the gamepad itself (SDL). While a stream is in front Rare's pointer mode from Apps is off (`stream::to_front` clears it; a unit test covers it) and the launcher ignores everything except Guide and quick access. The X/Y fix in `gamepad.rs` is for Rare's own gilrs input only: SDL maps the xpad-style pad (X on `BTN_X`, Y on `BTN_Y`) correctly, so Moonlight gets the real X and Y and `--no-swap-gamepad-buttons` keeps it that way.

Guide brings Rare back with the stream still running. Moonlight also forwards Guide to the PC (it maps it to the Xbox button), which may open Steam's overlay or Game Bar there; that is Moonlight's behaviour and its CLI has no switch for it.

## If something goes wrong

- **Pairing never finishes.** Check the PIN went in on the right PC. To rule out the hidden window, pair once in Moonlight itself (Desktop mode: open Moonlight, select the PC, type the PIN it shows into Sunshine). Rare reads Moonlight's pairing and lists the games.
- **No PCs found.** mDNS can be blocked by the router (guest networks, client isolation). Add the PC by address.
- **"No games listed".** Sunshine's Applications tab is empty, or every app is hidden. Add one, then Look again.
- **Black screen or stutter.** Lower the bitrate, try 1280 x 800, or set Video to HEVC.
