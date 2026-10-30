# Steam inside Rare

Rare is home and Steam is an app inside it. You open Steam from Rare, the Rare button brings you back, and Steam stays in the Running list until you close it. Rare never drops you into Steam without a visible way back.

## What the player sees

- **Steam is a tile** in Apps (Home's Apps row and Library > Apps), catalog id `steam`. A opens Steam's Big Picture. The same app opens from:
  - Quick settings (Steam)
  - the first-run and onboarding Steam steps
  - Home's "Open Steam" when the library is empty
  - a Steam game's "Open in Steam"
  - Steam store pages
- **Before Steam takes the screen**, Rare shows "Opening Steam. Press the Rare button to come back to Rare." for 1.6 s (`apps::STEAM_HINT`), then hands over. If the player presses the Rare button, or opens anything else, during that time, Steam doesn't get the screen.
- **The Rare button brings Rare back** from Steam's UI as from any game. The Rare button is the SuiPlay's Aya Space key; until UX-BUILD item 1 lands it arrives as Guide. Guide puts the launcher in front (`apps::front::to_launcher`), and the base-layer keeper puts Rare's app id first (`overlay/gamescope.rs`).
- **Steam is in the Running list** as "Steam", with Resume and Close like any app.
  - Resume opens Big Picture again, with the same hint.
  - Close runs `steam -shutdown`, which is what Steam's own Exit does. Games Steam runs close with it.
- **When Steam quits**, it leaves the Running list and Rare has the screen.
- **The "Rare" shortcut left in Steam's library** is a way back too. Bazzite installs added it, back when Rare ran as a Steam shortcut. Starting it reaches the running launcher (single instance), which takes the screen the way the Rare button does.
- **View on Home no longer opens Steam.** View is the screen's view, and Steam is a tile, not a button.

## Steam's overlay in a Steam game

Rare keeps the Rare and Quick buttons away from Steam, using InputPlumber intercept. Steam's overlay is reached from a **Steam** row instead.

- **Where the row is.** It is in the panel over a Steam game, and it moves into the Rare menu's Game tab when that menu lands.
- **What A on the row does.** It closes the panel, so the game has the screen and focus again. Then it sends Shift+Tab from Rare's virtual keyboard (`overlay::steam_menu`).
- **Why Shift+Tab reaches Steam:**
  - It is gamescope's `GuideKeyboardHotkey`, which gamescope registers once Steam runs in game mode. The device journal shows the line `binding: (GuideKeyboardHotkey) -> Adding new trigger [Tab + Shift_L]`.
  - It is also the Steam overlay's own in-game hotkey on every platform.

## Starting a Steam game

- **Steam is warm before Play.** Settings > Storage > Start Steam defaults to "After startup" in the native launcher: `steam -silent` a few seconds after the launcher (`backend::steam_at_start`). If Steam is not running at Play anyway, Rare starts it quietly and sends `steam://rungameid/<id>` once `steamwebhelper` is up (`launch::steam_game`).
- **Rare keeps the screen until the game has a window** (`apps::front::steam_pending`). While Steam starts a game its only window is its own UI, so handing over at Play showed Steam's home screen. The stage and game page say "Starting Steam", then "Starting the game". Steam's choices get the screen once gamescope lists a window under the game's id, or 15 s after the game's reaper started without one (a game that shows under another id), or when "didn't start" is said.
- **Steam asking the player.** `steamlog.rs` reads Steam's `logs/console_log.txt` GameAction lines. A wait with a reason ("cloudconflict") or one open longer than 2.5 s is a question (ShowInterstitials and CreatingProcess waits are answered by Steam itself within a second on every launch). Rare says it in a toast and on the start bar, for example "Steam needs an answer: your cloud save and this device's save differ", and 2 s later Steam's dialog gets the screen. Once answered, Rare again until the game's window; when the game ends, Rare.
- **Install scripts are not the game.** Steam runs a game's install script under `reaper ... AppId=<id> Install=1` before the game. It no longer counts as the game starting and ending (that read as "didn't start" while Steam waited on the cloud conflict).
- **The Rare button** while a Steam game is starting acts as on the launcher (Rare is what is on screen).

## Ways back from Steam's UI

- **The Rare button works whatever the pad target.** The image's InputPlumber profile sends it as the D-Bus action `ui_guide` (rare-input `ip.rs`), not through the pad, and the deck-uhid pad is read over hidraw (`deck.rs`). Device log 2026-10-27 23:21: `[overlay] Rare button Rare: OpenMenu` with Steam's UI on screen.
- **Steam's "Switch to Desktop"** goes back to Rare in Rare OS game mode. Steam runs `steamos-session-select plasma` through the PATH it started with (its log: "No SteamOSManager support for SwitchToDesktop, using legacy path"). Steam started by Rare finds `~/.local/share/rare/steam-path/steamos-session-select` first (written by the launcher, `launch::SESSION_SELECT`): it touches `$XDG_RUNTIME_DIR/rare-back-to-rare`, and the launcher takes the screen and drops Steam's UI from the Running list; Steam keeps running for games. Other sessions and other arguments go to `/usr/bin/steamos-session-select`. If Steam moves to SteamOS Manager's `SwitchToDesktopMode`, this stops applying and Steam goes to the desktop as before.
- **Open in Steam** on a Steam game's page or Options opens that game's own page in Steam (`steam://nav/games/details/<appid>`, `api::steam_game_page`), as "app:steam", so the Rare button and Switch to Desktop lead back from there too.
- **Steam's dialog only, then Rare** (`apps::steam_dialog(ctx, runner, url, done, max)`): for uninstall, installs and storage. Steam's UI opens at `url` with the hint; once `done()` is true (checked every 500 ms) or after `max`, Rare takes the screen back and Steam's UI leaves the Running list.
- Follow-up, not built: relabelling that entry "Back to Rare", or answering the cloud conflict from Rare, through Steam's CEF remote debugging (Decky's way: `.cef-enable-remote-debugging`, then script the SharedJSContext tab).

