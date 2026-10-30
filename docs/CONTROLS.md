# Controls

In one sentence: **the Rare button always means Rare, the Quick button always means the device, and everything else belongs to whatever is on screen.**

The SuiPlay0X1's own buttons (Rare, Quick, LC, RC) reach the launcher through InputPlumber's intercept mode (build items 1 and 2 in [UX-BUILD.md](UX-BUILD.md)). Until those land on the device, the launcher's hints show the older chords. Settings > Controls shows the map and remaps LC and RC.

## Rare OS

| Control | Launcher | A game or app in front | Rare menu, Quick Settings or Switcher open | First run | Asleep |
| --- | --- | --- | --- | --- | --- |
| **Rare** press | Rare menu; again: Home | Rare menu over the game (the game gets no input); again: Home, game keeps running | In the Rare menu: Home. In Quick Settings or the Switcher: the Rare menu | Nothing | Nothing |
| **Rare** hold 0.6 s | Switcher | Switcher | Switcher | Nothing | Nothing |
| **Rare** + Y | Screenshot | Screenshot | Screenshot of what is under the panel | Nothing | Nothing |
| **Rare** + View | Save the last 30 s | Save the last 30 s | Same | Nothing | Nothing |
| **Rare** + X | On-screen keyboard | On-screen keyboard (typed into the focused window) | Nothing | Nothing | Nothing |
| **Rare** + B hold 1.5 s | Nothing | Force quit the app in front, no confirm | Nothing | Nothing | Nothing |
| **Rare** + volume | Brightness | Brightness | Brightness | Brightness | Nothing |
| **Quick** press | Quick Settings (again or B closes) | Quick Settings over the game | Swaps to Quick Settings, or closes it | Quick Settings | Nothing |
| **LC** press (default) | Screenshot | Nothing (Settings > Controls > LC in games, or the game's page) | Screenshot | Nothing | Nothing |
| **LC** hold 1 s | Save the last 30 s | Only when LC in games is set | Same | Nothing | Nothing |
| **RC** press (default) | On-screen keyboard | Nothing (Settings > Controls > RC in games, or the game's page) | Nothing | On-screen keyboard | Nothing |
| **RC** hold 1 s | Performance overlay: next level | Only when RC in games is set | Same | Nothing | Nothing |
| **Power** press | Sleep | Sleep (game paused in memory) | Sleep | Sleep | Wake |
| **Power** hold 1 s | Power menu | Same | Same | Same | Wake |
| **Power** hold 10 s | Hardware off | Same | Same | Same | Same |
| **Volume** | Volume, Rare's bar | Same | Same | Same | Nothing |
| **A** | Select | Game | Select | Select | Nothing |
| **B** | Back | Game | Close, back to the game | Back one step | Nothing |
| **X** | The screen's extra action | Game | The panel's extra action | Step extra | Nothing |
| **Y** | Search | Game | The panel's second action | Skip setup | Nothing |
| **Menu** | Options for the focused item | Game | Nothing | Nothing | Nothing |
| **View** | The screen's view (sort, filter, list or grid, chart mode) | Game | Nothing | Nothing | Nothing |
| **LB / RB** | Top tabs | Game | Nothing | Nothing | Nothing |
| **LT / RT** | Sections of the screen | Game | The panel's tabs | Nothing | Nothing |
| **D-pad, left stick** | Move focus | Game | Move focus | Move focus | Nothing |
| **Right stick** | Scroll, chart pan and zoom | Game | Scroll | Nothing | Nothing |
| **L3, R3** | Nothing | Game | Nothing | Nothing | Nothing |
| **Touch** | Tap focuses and selects; a game never starts from a touch | Game | Tap selects, tap outside closes | Tap selects | Nothing |
| **View + Menu** hold 0.6 s | Switcher (silent rescue) | Switcher | Switcher | Nothing | Nothing |

- **Remapping.** LC and RC presses can be set to Capture, Keyboard, Performance overlay or Nothing in Settings > Controls (`lcAction`, `rcAction` in the launcher config). Their holds, Rare and Quick are fixed. The SuiPlay has no back paddles.
- **Intercept.** While a game or app has the screen, InputPlumber's intercept mode is `Pass`: everything goes to the game except Rare and Quick, which go only to Rare over D-Bus. While a Rare surface is open it is `All`, so the game sees nothing. The launcher in front is `Pass`.
- **Sticks, gyro, rumble.** Deadzones, vertical invert, gyro on or off with its sensitivity, "Gyro in Steam games" and rumble strength are in Settings > Controls. The gyro is read through InputPlumber; Steam games see the pad as a Steam Deck while they run.

## Shoulder buttons in the launcher

| Button | Always |
| --- | --- |
| **LB / RB** | The five tabs (Home, Library, Sui, Market, Wallet), wrapping. A page or sheet over the tab closes first. Only first run and sign-in hold them. |
| **LT / RT** | The current screen's sections: Library filters, Sui Featured / All games / Store / Coming soon, Market Collections / Coins / Perps / Traders, a collection's Items / Activity / Holders, a coin's Trades / Holders, Wallet Coins / Collectibles / Activity, the Rare menu's tabs. Without sections they jump through lists. |

**Where LT / RT mean something else.** Only while that panel or mode has focus: the trade order panel (less or more amount or leverage), chart mode (shorter or longer timeframe), the price alert sheet (lower or higher). Modal sheets (a purchase, a wallet approval) hold the shoulder buttons until they close with B.

A starts a game or app only on a deliberate press (or a click), never from a touch, a held button or focus moving.

## Guest mode

When Rare runs as a shortcut inside Steam Game Mode, Steam owns Guide and QAM and Rare cannot intercept them.

| Control | Does |
| --- | --- |
| Guide, QAM | Steam's |
| Hold View 0.5 s | Rare menu over a game or app Rare started |
| Hold View + Menu 0.6 s | Switcher |
| Everything else | As Rare OS in the launcher; games get everything |

## Hint bar

Hints name the SuiPlay's own buttons with device glyphs (Rare, Quick, LC, RC), never "Hold Select + Start". Every screen shows LB RB Tabs, and LT RT Sections when it has sections.

In games every button follows one map, per console too: [EMULATION.md](EMULATION.md#the-standard-in-game-button-map).

Trade's layout: [CONTROLS-TRADE.md](CONTROLS-TRADE.md). The Rare menu: [OVERLAY.md](OVERLAY.md).
