# Emulation: controls and setup

How Rare OS sets up emulators so every button works out of the box on the SuiPlay0X1, and how the
owner chooses which emulators to install. Code: `launcher/crates/rare-emu` (`controls.rs`,
`setup.rs`, `actions.rs`, `defaults.rs`), the first-run step `screens/firstrun/emulators.rs`, Library >
Emulation (`features/emulation.rs`). Proof tools: `tools/emu`.

## One pad

Games must see exactly one pad: InputPlumber's target, **"Microsoft X-Box One Elite 2 pad"**
(045e:0b00, version 1; SDL calls it "Xbox One Elite 2 Controller", GUID
`030010f85e040000000b000001000000`, `030000005e040000000b000001000000` with the name CRC cleared).
The pad's own device, "Microsoft X-Box 360 pad" (045e:028e), is grabbed by InputPlumber and never
sends anything. Where it is visible, SDL lists it first, so anything bound to "pad 0" or "port 1"
got a silent pad.

- Everything Rare starts gets `SDL_GAMECONTROLLER_IGNORE_DEVICES=0x045e/0x028e,0x057e/0x2009` when
  InputPlumber's pad is there (`controls::launch_env`, through the launcher's launch extras). SDL 2 and
  SDL 3, Flatpaks and AppImages all honour it: the target becomes SDL's pad 0 and the only pad.
- RetroArch is started with SDL's joypad driver (`retroarch::PAD_CFG` in `rare.cfg`). With the
  Flatpak's default driver, udev, it gave port 1 to the silent pad ("Xbox 360 Controller configured
  in port 1", the SuiPlay's pad in port 2), so Start and every other button did nothing.
- Dolphin had no pad profile at all: it writes keyboard keys on its first start (Start = Return), so
  GameCube games such as Battle for Bikini Bottom never got Start.

## The standard in-game button map

One map for every game and emulator on the SuiPlay0X1 (`rare_emu::controls::STANDARD` and `CONSOLES`, shown in Settings > Controls > In games). Every profile in this file follows it, and `controls::tests::the_standard_map_holds_in_every_profile` checks it.

| SuiPlay button | In every game |
| --- | --- |
| A, B, X, Y | The console's face buttons by where they sit. On Nintendo systems the console's A is the right button (B) and its B the bottom one (A). |
| Menu | Start, or plus on Switch and Wii U |
| View | Select, Back, or minus on Switch and Wii U |
| LB, RB | The console's shoulder buttons |
| LT, RT | The console's triggers, analog where the console had them |
| Left stick | The console's stick |
| Right stick | Its second stick, or what the console used instead: C-stick, C buttons, the Wii Remote's pointer |
| L3, R3 | Stick clicks |
| D-pad | D-pad |
| Rare | The Rare menu: save and load states, fast forward, the emulator's menu, quit. Never sent to the game. |
| Quick | Quick settings. Never sent to the game. |
| LC, RC | Nothing in games unless you pick an action in Settings > Controls or on the game's page |
| View + Menu, held | Rare's way out of the game (the Switcher). The game sees both presses. |

Per console, what each SuiPlay button is:

| Console | SuiPlay button: console button |
| --- | --- |
| NES, Famicom | B: A; A: B; View: Select; Menu: Start; D-pad: D-pad |
| Super Nintendo | B: A; A: B; Y: X; X: Y; LB, RB: L, R; View: Select; Menu: Start |
| Game Boy, Game Boy Color, Game Boy Advance | B: A; A: B; LB, RB: L, R (Advance); View: Select; Menu: Start |
| Nintendo 64 | A: A; X: B; LT: Z; LB, RB: L, R; Menu: Start; Left stick: Stick; Right stick: C buttons (or hold RT with A, B, X, Y) |
| Genesis, Mega Drive | X: A; A: B; B: C; LB: X; Y: Y; RB: Z; View: Mode; Menu: Start |
| PlayStation, PlayStation 2, PlayStation 3 | A: Cross; B: Circle; X: Square; Y: Triangle; LB, RB: L1, R1; LT, RT: L2, R2 (pressure on PS2); L3, R3: L3, R3; View: Select; Menu: Start; Sticks: Left and right sticks; PS button: None: the Rare menu |
| PSP | A: Cross; B: Circle; X: Square; Y: Triangle; LB or LT: L; RB or RT: R; View: Select; Menu: Start; Left stick: Analog nub |
| GameCube | A: A; X: B; B: X; Y: Y; RB: Z; LT, RT: L, R, analog; Menu: Start; Left stick: Control stick; Right stick: C-stick |
| Wii (Wii Remote and Nunchuk) | A: A; RT: B; X: 1; Y: 2; View: Minus; Menu: Plus; R3: Home; B: Shake; Right stick: Pointer; D-pad: D-pad; Left stick: Nunchuk stick; LB: Nunchuk C; LT: Nunchuk Z; RB: Nunchuk shake |
| Wii U, Switch | B: A; A: B; Y: X; X: Y; LB, RB: L, R; LT, RT: ZL, ZR; View: Minus; Menu: Plus; L3, R3: Stick clicks; Sticks: Left and right sticks; Home, Capture: None: the Rare menu |
| Nintendo 3DS | B: A; A: B; Y: X; X: Y; LB, RB: L, R; LT, RT: ZL, ZR; View: Select; Menu: Start; Left stick: Circle Pad; Right stick: C-Stick; Touch screen: Bottom screen |
| Nintendo DS | B: A; A: B; Y: X; X: Y; LB, RB: L, R; View: Select; Menu: Start; D-pad: D-pad; Touch screen: Bottom screen |
| Xbox | A, B, X, Y: A, B, X, Y; LB: White; RB: Black; LT, RT: Triggers; View: Back; Menu: Start; L3, R3: Stick clicks; Sticks: Left and right sticks |
| Dreamcast | A, B, X, Y: A, B, X, Y; LT, RT: Triggers, analog; Menu: Start; Left stick: Stick |

Every other RetroArch system follows the same rule through the RetroPad: buttons by where they sit on the console, Menu is Start, View is Select.

## Hotkeys are never on the pad

No emulator has a hotkey on a pad button or chord. Start and Select (Menu and View) always reach the
game. Save state, load state, fast forward and the emulator's menu are rows in the Rare menu, which
sends them as keys from Rare's virtual keyboard (`actions.rs`):

| Emulator | Save state | Load state | Fast forward (toggle) | Emulator menu |
| --- | --- | --- | --- | --- |
| RetroArch | network command, slots with pictures | same | `FAST_FORWARD` | `MENU_TOGGLE` |
| DuckStation | F13 | F14 | F16 | F15 |
| PCSX2 | PINE, slot 1 | PINE, slot 1 | F16 (ToggleTurbo) | F15 |
| PPSSPP | F2 | F4 | ` (SpeedToggle) | Esc |
| Dolphin | Shift+F1 (its default, slot 1) | F1 | Tab | none (pointer menus) |
| Azahar | Ctrl+C | Ctrl+V | none | none |

RPCS3's PS button default (View + Menu) is removed: that chord is Rare's way out of a game.

## The profiles

Nintendo systems use the Nintendo layout by position: the console's A is the right face button (Xbox
B), B the bottom one (Xbox A). GameCube is positional too: B left of A, X right of A.

| Emulator | Binds the pad by | Profile (SuiPlay button: console button) | Where |
| --- | --- | --- | --- |
| RetroArch (every core) | SDL driver, port 1, its own autoconfig for 045e:0b00 | A: B, B: A, X: Y, Y: X (RetroPad), LB/RB: L/R, LT/RT: analog L2/R2, View: Select, Menu: Start, L3/R3, d-pad, both sticks, rumble | `rare.cfg` at each launch |
| DuckStation | SDL-0 | Analog controller: A: Cross, B: Circle, X: Square, Y: Triangle, LB/RB: L1/R1, LT/RT: L2/R2, View: Select, Menu: Start, L3/R3, sticks, both motors | `settings.ini` [Pad1], [Hotkeys] |
| PCSX2 | SDL-0 | DualShock 2 the same, pressure on LT/RT, both motors | `PCSX2.ini` [Pad1], [Hotkeys] |
| PPSSPP | SDL pad 0 (device 10) | A: Cross, B: Circle, X: Square, Y: Triangle, LB or LT: L, RB or RT: R, View: Select, Menu: Start, d-pad, left stick: nub | `controls.ini` [ControlMapping] |
| Dolphin, GameCube | `SDL/0/Xbox One Elite 2 Controller` | A: A, X: B, B: X, Y: Y, RB: Z, LT/RT: L/R (analog and full press), Menu: Start, left stick: main stick, right stick: C-stick, d-pad, rumble | `GCPadNew.ini` [GCPad1] |
| Dolphin, Wii | same | Wii Remote + Nunchuk: A: A, RT: B, X: 1, Y: 2, View: minus, Menu: plus, R3: Home, B: shake, right stick: pointer, d-pad; LB: C, LT: Z, left stick: Nunchuk stick, RB: Nunchuk shake; rumble | `WiimoteNew.ini` [Wiimote1] |
| Dolphin | | the pad works while the game has the screen (InputPlumber holds it under the Rare menu) | `Dolphin.ini` [Input] BackgroundInput |
| melonDS | SDL joystick 0 | B: A, A: B, Y: X, X: Y, LB/RB: L/R, View: Select, Menu: Start, d-pad (touch on the screen) | `melonDS.toml` [Instance0.Joystick] |
| Azahar, Lime3DS | full SDL GUID | B: A, A: B, Y: X, X: Y, LB/RB: L/R, LT/RT: ZL/ZR, View: Select, Menu: Start, d-pad, left stick: Circle Pad, right stick: C-Stick | `qt-config.ini` [Controls] |
| Citron | SDL GUID without CRC | Pro Controller: B: A, A: B, Y: X, X: Y, LB/RB: L/R, LT/RT: ZL/ZR, View: minus, Menu: plus, L3/R3, both sticks, d-pad, rumble | `qt-config.ini` [Controls] |
| Ryujinx | `0-00000003-045e-0000-000b-000001000000` | Pro Controller, same layout, rumble, motion | `Config.json` input_config, Player1 |
| Cemu | `0_030010f85e040000000b000001000000` (SDLController) | Wii U GamePad, same layout, LT/RT: ZL/ZR, View: minus, Menu: plus, L3/R3, both sticks, d-pad, rumble | `controllerProfiles/controller0.xml` |
| RPCS3 | SDL handler, `Xbox One Elite 2 Controller 1` | DualShock 3: A: Cross, B: Circle, X: Square, Y: Triangle, LB/RB, LT/RT, View: Select, Menu: Start, L3/R3, sticks, both motors; no PS button | `input_configs/global/Default.yml` |
| xemu | its own: the first game controller goes to port 1 | Xbox pad as is, rumble | nothing written |

RPCS3's Flatpak has no evdev handler, so Rare's earlier evdev profile left player 1 empty; Azahar
needs the full GUID, so Rare's earlier CRC-less one never matched; PPSSPP's stock L and R are
buttons its SDL build never sends.

## Never over the player's own settings

Rare writes only the controller part of each emulator's settings (one INI or TOML section, a
controller file, Ryujinx's Player1 entry), and classifies what is there first
(`controls::classify`):

| On disk | What Rare does |
| --- | --- |
| Missing | writes it |
| The emulator's own default (keyboard only, PPSSPP's and melonDS's stock mappings) | writes it |
| What Rare wrote last time, unchanged (hash in `~/.local/share/rare/emu-controls.json`), or one of Rare's earlier profiles | brings it up to date |
| The player's own pad setup | keeps it, and Library > Emulation > Controls lists it; "Use Rare's controls", pressed twice, replaces it and keeps the old file as `<name>.before-rare` |

This runs at every launch of a ROM or emulator and after setup. Re-running setup never replaces the
player's own settings.

First-run dialogs that would sit over a game Rare started (a pad cannot answer them) are off in the
first-run settings: DuckStation's wizard and desktop-file prompt, Citron's update prompt, RPCS3's
welcome. RPCS3's Flatpak asks "use this build anyway?" at every start with No (quit) as the default:
Rare starts it with `FLATPAK_ID` unset, which skips it. DuckStation's AppImage is the x64 build, not
the legacy SSE2 one.

## Choosing emulators

First run ends with **Emulators**: one line per emulator with the systems it plays, its size and how
many of the owner's games it starts. Lines for systems with games are ticked. **Set up N emulators**
installs the ticked ones in the background (Library > Emulation shows progress), **All** installs
everything, **Later** leaves it for Library > Emulation, which has the same ticks, Set up and All.
Sizes are measured on the build machine (`catalog::Emulator::size_mb`); the shared Flatpak runtimes (about
2.3 GB) come on top the first time.

