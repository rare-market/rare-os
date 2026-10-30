# Rare over a game: the Rare menu and the Switcher

**Press the Rare button** over any game or app and the Rare menu comes up over it: a right-side panel 32rem wide over a dimmed still of the game. It opens on **Game** with **Resume** focused, so Rare then A is always back to the game. LT and RT switch tabs, B goes back to the game, and the Rare button again goes **Home** (the game keeps its place). **Hold the Rare button** for the **Switcher**. The button logic is `overlay/focus.rs` (the focus keeper).

| Tab | Shown when | What |
| --- | --- | --- |
| Game | over a game or app | the game card (art, title, how long it has been playing), **Resume**, your items line for a Sui game ("22 items · worth 98.60 SUI · 9 offers", A opens Items), **Screenshot** and **Save clip** with their chords, **Performance** (A opens Quick Settings inside the menu), the **Steam** row for a Steam game (Steam's overlay: friends, guides, controller layout, through Shift+Tab), Steam's pages and **Close Steam** over Big Picture, **Game details** (leaves the game for its page), **Quit game** (asks once more) |
| Rare | over the launcher, instead of Game | Home, Your profile, Settings, Power (Sleep, Restart, Turn off), Desktop mode under a hairline |
| Items, Market | a Sui game with collections on rare.market | below |
| Trophies | a Steam game, or a Sui game | below |
| Friends | always | Party (an invite with Join, your party with Invite and Leave, or Start a party), Playing (A joins their game), Online, Requests |
| Notifications | always | every notice, newest first, unread dot; A acts on one (in the launcher) |

Over the launcher the same menu opens as the launcher's own sheet, on Notifications when any are unread, else on Rare. A party invite toast on screen when Rare is pressed opens Friends with Join focused.

Screenshots: `docs/screenshots/rare-menu-*-1280x800.png` and `switcher-*-1280x800.png`.

## The Switcher and quick resume

Hold the Rare button (or View + Menu, a silent rescue that works everywhere): a full-screen row of cards. **Home** first, then everything running, most recently used first, each as the still frame gamescope took when you left it (every way away from a game goes through the menu or the Switcher, which capture first; `overlay/switcher.rs` keeps one per game in `~/.cache/rare/switcher/`). Then up to four recent games that aren't running, smaller. A switches, X closes (X again confirms), B goes back. The footer lists the Rare chords.

The game you switch away from is **frozen** (`freeze.rs`) and its card says **Paused**; it thaws before anything shows it (`apps::front::to_front`), so it resumes at once.

- **How.** The cgroup v2 freezer on the program's own `app-steam-app<id>-<n>.scope` (`systemctl --user freeze`, `cgroup.freeze` as the fallback), with a Flatpak's own `app-flatpak-*` scope. A Steam game: its reaper's scope when Steam made one named for that game, else SIGSTOP of everything below the reaper (never the reaper or Steam). Anything else: SIGSTOP of its process tree.
- **Never frozen:** Steam itself and Big Picture, streams (the game runs on the PC), web games in Rare's own window (Rare's WebKit process, wallet approvals, live sessions).
- **Defaults, with "Keep running in the background" on each game's page to change them:** games pause; online games keep running (a Sui game, or a Steam game on AreWeAntiCheatYet's list: servers drop a silent client in 30 to 60 s and anti-cheat kicks a stalled one); apps keep running (messages and calls are network sessions, music and video are what you listen to); web games in Chrome keep running.
- **Two at most.** Leaving a third asks which paused game to close; it keeps running until then.
- **Safety.** Quit thaws first (a frozen game can't act on SIGTERM and save). What is frozen is written to `$XDG_RUNTIME_DIR/rare-frozen.json`; a restarted launcher thaws it, and the launcher thaws everything when it exits.

## Guest mode

When Rare runs as a shortcut inside someone else's Steam Game Mode (a `SteamLaunch` reaper above it, or a gamescope session it is not the client of; `overlay/guest.rs`), Steam owns Guide and quick access. There **hold View for half a second** for the Rare menu, and the hints say so. In Rare OS View is always the game's.

## Items, Market and Trophies

| Tab | Source | Actions |
| --- | --- | --- |
| Items | rare.market `/v1/accounts/<you>/items?collection=<type>` for each of the game's collections (from `games.json`), floors from `/v1/collections/<type>` | A opens the item: **Sell for X SUI** into the best offer, or **List** at Floor, Floor +10%, the current price or any price (on-screen keyboard) |
| Market | `/v1/collections/<slug>/items?sort=price_asc`, the cheapest of each collection in turn, your own listings left out | A opens the Marketplace's Buy sheet |
| Trophies | Rare Trophies for Steam games (`src/trophies`: Steam achievements read on the device, minted trophies on chain), then, from Sui gRPC, objects you own from the game's own Move packages that are not one of its tradeable collections (badges, rewards, soulbound items). A game with nothing on the market opens on this tab | none (claiming stays in the trophy case) |

- **Selling** uses rare.market's accept-offer builders (`marketplace/venues`): Tradeport bids, kiosk bids and escrow bids, and Drip bids.
  - Before Sell shows, the sale is built once as a dry run. If the collection's policy has a rule the builders can't prove (Pawtato Land's `table_allowlist_rule`, for example), the menu names the best offer and says selling into it isn't available, with no button.
  - The amount shown is what you receive. Some Tradeport kiosk bids make the seller pay the royalty, and `bestOffer` corrects for that.
- **Listing** is a plain `0x2::kiosk::list` in your own kiosk, as on rare.market today. Wallet items are placed, or locked when the policy has a lock rule, first.
  - Items listed on Tradeport or Drip say "Delist it there", because their listing holds the item.
- **Signing**: every sale, listing and purchase goes through `signAndExecute` and Rare Wallet's review sheet. That means a simulation, then what leaves and what arrives, then the PIN or the device. Watch-only accounts see prices but no actions.
- Tests:
  - `src/overlay/overlay.test.ts` covers game mapping, items, market order, notifications, trophies and price input.
  - `src/overlay/trade.test.ts` covers the PTB shapes.
  - `LIVE=1 npx vitest run src/overlay/trade.live.test.ts` runs mainnet dry runs of accept-offer on every live venue source and of listing. These only simulate; nothing is ever signed or sent.

