# Web games: checker results

Every Rare web game, checked the way the launcher runs it: the game's profile (`launcher/src/games/profiles.json`) and Rare's runtime (`launcher/src-tauri/src/webgame.js`) are injected before the page's own scripts, then the page is loaded in Chrome and in WebKit (Playwright's WebKit is the closest engine to the device's WebKitGTK that runs on a Mac). The checker verifies that the page loads, that Rare Wallet registers as a Wallet Standard wallet (including the older Block API names that older dapp-kit pages require), and that controller input arrives in the game's input mode:

- **gamepad** (ACT): the page reads the standard Gamepad API; Rare exposes one virtual pad with analog sticks and triggers, and sends no keys.
- **focus** (Pawtato Land, SuiFrens): the d-pad moves a focus ring over the page's own buttons and A presses them. A profile can map extra buttons to keys (X to Backspace, Menu to Enter), and View opens Rare's keyboard, which sends key presses for faster typing.
- **keys** (default, Cosmocadia, DoubleUp): buttons become keyboard events, the right stick drives a pointer and RB clicks.

Run it with `node launcher/scripts/webgame-check.mjs [slug ...] [--engine chrome|webkit|both] [--shots dir] [--md docs/WEBGAMES.md]`. `--as '{"input":"focus"}'` tries a profile change before editing `profiles.json`.

No title needs the Chrome fallback (`"browser": "chrome"` in a profile). The fallback stays available on every web game's page as "Open in Google Chrome" (the device has the Chrome Flatpak).

Results from 2026-10-26:

| Game | Catalog status | Input mode | Play URL | Chrome | WebKit | Verdict |
|---|---|---|---|---|---|---|
| ACT (`act`) | live | gamepad | https://play.actuniverse.com/ | HTTP 200; canvas webgl2:ok 2d:ok; wallet 8 features incl. legacy Block API; input ok (virtual pad standard, axes ok, page polled 23x/s) | HTTP 200; canvas webgl2:ok 2d:ok; wallet 8 features incl. legacy Block API; input ok (virtual pad standard, axes ok, page polled 122x/s); errors: 1 | Plays in WebKit: loads, input arrives |
| Pawtato Land (`pawtato-land`) | live | focus | https://pawtato.gg/ | HTTP 200; wallet 8 features incl. legacy Block API; input ok (ring on "Explore Without Acco", A clicked it); signals: sign-in | HTTP 200; wallet 8 features incl. legacy Block API; input ok (ring on "Explore Without Acco", A clicked it); signals: sign-in; errors: 2 | Plays (sign-in needed): loads, input arrives; account or wallet sign-in in the page |
| Cosmocadia (`cosmocadia`) | beta | keys | https://www.cosmocadia.io/ | HTTP 200; canvas webgl2:ok; wallet 8 features incl. legacy Block API; input ok (keys w,Enter, pointer moves) | HTTP 200; canvas webgl2:ok; wallet 8 features incl. legacy Block API; input ok (keys w,Enter, pointer moves) | Plays in WebKit: loads, input arrives |
| Pebble City (`pebble-city`) | live | keys | none |  |  | Offline: Pebble City's web page is offline right now. |
| Suuuiplash (`suuuiplash`) | live | keys | none |  |  | Offline: Suuuiplash has no web build yet; its page is a trailer and app download. |
| DoubleUp (`doubleup`) | live | keys | https://www.doubleup.io/ | HTTP 200; wallet 8 features incl. legacy Block API; input ok (keys ArrowUp,Enter, pointer moves); signals: region | HTTP 200; wallet 8 features incl. legacy Block API; input ok (keys ArrowUp,Enter, pointer moves); signals: region | Region blocked: geo-blocked from the test machine |
| SuiFrens (`suifrens`) | live | focus | https://suifrens.com/ | HTTP 200; wallet 8 features incl. legacy Block API; input ok (ring on "Connect Wallet", A clicked it) | HTTP 200; wallet 8 features incl. legacy Block API; input ok (ring on "Connect Wallet", A clicked it) | Plays in WebKit: loads, input arrives |
| Sui 8192 (`sui-8192`) | dead | keys | none |  |  | Offline: Sui 8192 is no longer online. |

## Developer mode and testnet

`rare-launcher --dev-game <url>` opens a local web game (https, or http on localhost) in the web game runtime with Rare Wallet and `rare:platform` (the Local API, docs/LOCAL-API.md) injected, and an amber banner across the top so it can't pass for a shipped game. A game that isn't in the catalog has no collections, so give them on the command line:

```
rare-launcher --dev-game http://localhost:5173 --dev-slug my-game --dev-collection 0xabc::items::Sword
```

`--dev-slug` names the game (default `dev-game`); `--dev-collection` is repeatable and scopes `inventory`, `sell` and `buy` like a catalog entry. Starting the launcher again with `--dev-game` reloads the game in the running one. A local path is not served: run your dev server and pass its URL.

Settings > Developer switches the launcher to testnet: Sui reads and signing use the testnet RPC, rare.market and the gas sponsor get `?network=testnet`, and `me().network` says `testnet`. The testnet RPC, market API and sponsor addresses can be replaced (https, or http on this machine). Config keys: `network`, `testnetRpc`, `testnetApi`, `testnetSponsor`.
