# Apps

Rare Launcher has an Apps row on Home and an Apps filter in Library, like a console's media row: Steam, Battle.net, Telegram, X, Netflix, YouTube, Discord and Google Chrome. Screenshots: `docs/screenshots/apps-*.png`.

Code: the catalog is `launcher/src/apps/catalog.json`, read by the UI (`launcher/src/apps`) and compiled into the native side (`launcher/src-tauri/src/apps`). Icons are each service's own (apple-touch icons and favicons), bundled in `launcher/public/apps` with a `NOTICE`.

## How each app opens

Each app lists ways to open it, best first. The first way the device can use is the default; the app's page offers the others.

| App | Default | Fallback | Controller |
|---|---|---|---|
| Telegram | Telegram Desktop (`org.telegram.desktop` Flatpak) | Telegram Web (`web.telegram.org/k`) in Rare's window | pointer / focus |
| X | `x.com` in Rare's window, sign-in kept per app | Chrome app window | focus / pointer |
| Netflix | Chrome app window (`--app=https://www.netflix.com/browse --start-fullscreen`) | none | pointer |
| YouTube | Chrome app window on `youtube.com/tv` with a TV user agent | none | gamepad |
| Discord | Discord (`com.discordapp.Discord` Flatpak) | `discord.com/app` in Rare's window | pointer / focus |
| Google Chrome | Chrome itself, fullscreen, the normal profile | none | pointer |
| Battle.net | Blizzard's app through umu-run (Proton) in its Wine prefix | none (Rare installs it) | pointer |

- **DRM.** WebKitGTK has no Widevine, so Netflix only opens in Google Chrome or Microsoft Edge (`drm: true`). Chromium and Brave don't count.
- **Chrome app windows** get their own `--user-data-dir` (`~/.var/app/com.google.Chrome/data/rare-apps/<id>` for the Flatpak), so each app keeps its sign-in and runs as its own Chrome. That is what lets Close end just that app and the running dot follow it. On the SuiPlay0X1 they also get `--force-device-scale-factor=1.5`, the same scale as Rare's UI.
- **Rare's window** is the web game window (`webgame.rs`) with its own storage per app, the focus controller mode and the on-screen keyboard. One web window at a time: opening X replaces a running web game.
- Apps run in the launcher's runner as `app:<id>`, in their own systemd scope, so Resume, Close, the running dot and the status bar work as they do for games.

## Install state

The native side checks `~/.local/share/flatpak/app/<id>/current` and `/var/lib/flatpak/app/<id>/current` (no processes). When an app's best way needs a Flatpak that isn't there, its page shows **Get** and the exact one-line command:

```
flatpak install --user -y flathub org.telegram.desktop
```

If the user installation has no Flathub remote yet, the command starts with `flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo &&`. Get runs the same thing from the launcher (user scope, no sudo) under `script`, so flatpak draws its progress bar and the page shows a percentage; `app-install` events carry it.

## Battle.net

Blizzard's games (World of Warcraft, Diablo, Overwatch, Hearthstone, StarCraft, Heroes of the Storm, Warcraft III and Call of Duty) come through Battle.net, Blizzard's Windows app. Rare runs it with Proton, the same way it runs Windows games from GOG.

**Getting it.** Open Battle.net in Apps (Home's Apps row or Library > Apps) and press **Get Battle.net**. Rare downloads Blizzard's installer from Blizzard and runs it. Blizzard's own window takes the screen, installs the app (about a minute) and then shows Blizzard's sign-in. Sign in there with your Blizzard account. Rare never sees your password. The pad works as a mouse in Battle.net's window: right stick for the pointer, A to click, Y for the on-screen keyboard. Press the Rare button to come back to Rare; Battle.net stays in the Running list with Resume and Close.

**Installing games.** Install games inside Battle.net, as on a PC. Once a game is installed it shows in **Library > Battle.net** (and in All and on Home), after the next library scan. Press Play in Rare: Rare starts Battle.net, which signs the game in and starts it. Blizzard's games need Battle.net running; Rare takes care of that.

**Already have Battle.net?** Rare finds it and its games in:

- Rare's own prefix, `~/.local/share/rare/prefixes/battlenet` (where Get Battle.net puts it).
- Steam, when Battle.net was added as a non-Steam game: its prefix in `steamapps/compatdata/<id>`. Rare uses the Proton Steam uses for that shortcut, so the prefix stays as it is.
- Lutris and other Wine prefixes in `~/Games` (for example `~/Games/battlenet`), Heroic's prefixes, Bottles and `~/.wine`.

Rare reads Battle.net's install list (`drive_c/ProgramData/Battle.net/Agent/product.db`) and, for older installs, each game folder's `.build.info`. Games that are also on Steam (Overwatch 2, Diablo IV, Call of Duty) get Steam's art and share one tile with the Steam copy.

**Updates, uninstall and moving** stay in Battle.net. Open Battle.net from Apps and use the game's settings menu. Rare's Uninstall says so instead of deleting files, since every Battle.net game shares one prefix.

**World of Warcraft with a controller.** World of Warcraft has its own controller mode, off by default. Turn it on once: in the game, type `/console GamePadEnable 1` in the chat, then `/reload`. The game saves it (`SET GamePadEnable "1"` in `WTF/Config.wtf`). Until it is on, Rare shows that tip when World of Warcraft starts. Diablo IV, Diablo II: Resurrected, Overwatch 2 and Call of Duty read the controller on their own. Hearthstone and StarCraft are played with the pointer.

**How Rare starts a game.** `umu-run "<prefix>/drive_c/Program Files (x86)/Battle.net/Battle.net.exe" --exec="launch <code>"`, with `WINEPREFIX` set to the prefix and `PROTONPATH` to its Proton. The codes: WoW (World of Warcraft), WoWC (Classic), D3, Fen (Diablo IV), OSI (Diablo II: Resurrected), Pro (Overwatch 2), WTCG (Hearthstone), Hero, S1, S2, W3. A game Rare has no code for (World of Warcraft Classic Era, a new game) opens Battle.net on its own, and Play there starts it. Battle.net stays open after the game, so Rare counts the game as running while a program from its folder runs, and Quit game closes only the game.

Code: `launcher/crates/rare-library/src/battlenet.rs` (prefixes, games, launch), `launcher/crates/rare-launch/src/apps/battlenet.rs` (Get Battle.net). Device check: `cargo run -p rare-library --example battlenet_scan` prints what Rare finds and the command for each game.

## The controller outside the launcher

Chrome, Telegram Desktop and Discord don't read a gamepad. Rare already reads the pad itself (gilrs on evdev, also while another window has focus), so the choice was where the mouse and keyboard events come from.

| Option | Verdict |
|---|---|
| Handheld Daemon | No. Its controller outputs are controller types only (Xbox, Elite, DualSense, hidden); there is no mouse or keyboard mode to switch to. Its HTTP API (`localhost:5335`, bearer token) can change the emulated controller, nothing else that helps here. |
| Steam Input | No. The non-Steam shortcut's layout applies to everything Rare starts, including Rare itself, and only a game using the Steamworks input API can switch action sets. A mouse layout would click inside Rare too. |
| InputPlumber | Yes on the Rare OS image, not today. `LoadProfilePath` / `SetTargetDevices` on `org.shadowblip.Input.CompositeDevice` do exactly this, but the polkit policy says `auth_admin` for them, so a normal session can't call them without a rule. |
| **Rare's own virtual device** | **What ships now.** While an app in pointer mode is in front, Rare writes mouse and key events to a virtual device it creates through `/dev/uinput`. Bazzite tags `/dev/uinput` `uaccess` (Steam's udev rules), so the logged-in user can open it without sudo. |

Pointer mode layout: right stick moves the pointer (the same as Rare's web games), left stick scrolls (hi-res wheel), d-pad arrow keys, A or RT left click, LT right click, B back (Alt+Left in Chrome, Escape elsewhere), X space (play or pause), Y Steam's on-screen keyboard (`steam://open/keyboard`, Game Mode), LB and RB Shift+Tab and Tab, Menu Enter, View Escape. While it is on, the left stick no longer acts as a d-pad. The app's page shows the layout.

**Back to Rare, always.** Guide is never taken by pointer mode: it hands the screen and the pad back to the launcher with the app still running. Holding Guide for 0.6 s opens the running list with Resume and Close (or Quit game). The launcher also takes the pad back on its own when the app in front exits. With the launcher in front, the pad drives it even while apps or games run in the background.

**The Rare OS image** keeps the uinput path (it works on any input stack) and adds InputPlumber switching for devices InputPlumber manages:

1. Ship `/usr/share/rare/inputplumber/rare-desktop.yaml`, a device profile with the same layout as above (mouse and keyboard targets, Guide and quick access to D-Bus).
2. Ship a polkit rule that lets the active local session call `LoadProfilePath` and `SetTargetDevices` for InputPlumber, with no password.
3. When a pointer-mode app comes to the front, the launcher calls `SetTargetDevices(["gamepad","mouse","keyboard"])` and `LoadProfilePath(rare-desktop.yaml)` on the composite device, and loads the default profile again when Guide brings Rare back. Rare's own uinput device stays off on those devices so input never arrives twice.
4. The image sets the udev rule for `/dev/uinput` itself rather than relying on Steam's package.

