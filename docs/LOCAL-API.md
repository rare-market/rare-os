# Rare Local API, `rare:platform` and `rare://` links

How a game calls Rare on the device: who is playing, their items from this game, a sell or buy sheet over the game, studio vouchers, Rare pages, and notices. It is the single source of truth for the launcher and for `@rare/sdk` (rare-sdk, `packages/sdk`); both follow this page.

A game can't move anything by itself. Every `sell`, `buy` and `claim` opens a Rare sheet the player acts on, and anything signed then goes through Rare Wallet's review sheet and PIN (or Ledger, Keystone or passkey), exactly like a purchase in the Marketplace. The launcher never signs or submits on a game's behalf.

## Transports

| Game | Transport | How it finds Rare |
|---|---|---|
| Native or Proton game the launcher started | Loopback HTTP and JSON | `RARE_API`, `RARE_TOKEN`, `RARE_GAME` in its environment |
| Web game in Rare's game window | `rare:platform`, a Wallet Standard feature of the injected Rare Wallet | `wallet.features["rare:platform"]` |
| Web game whose engine prefers HTTP (Unity WebGL, Godot web) | Loopback HTTP, CORS for that game's origin | `wallet.features["rare:platform"].api` = `{ url, token }` |
| Anything that can open a URL | `rare://` links and their https://rare.market twins | xdg-open, a link in a web game, a QR code |

The method set is the same on every transport.

## Environment (native and Proton games)

When the launcher starts a game (local and Store games, Proton through umu-run, Legendary), it adds:

```
RARE_API=http://127.0.0.1:<port>
RARE_TOKEN=<64 hex characters, new for every launch>
RARE_GAME=<slug>
```

- `RARE_GAME` is the Rare slug for Store games (`rare:<slug>`) and web games, and the launcher's library id otherwise. `GET /v1/me` always answers the resolved slug in `game.slug`.
- Wine passes the variables to Windows builds, and a Windows `.exe` reaches Linux's 127.0.0.1, so Proton games work unchanged.
- A token is bound to one game. Launching the game again replaces it, and it stops working about 20 seconds after the game is no longer running.
- Games Steam starts (`steam://rungameid`) don't get the variables: Steam starts them, not Rare. They can use `rare://` links.
- `RARE_API_PORT=<port>` in the launcher's own environment pins the port (development). Otherwise the OS picks a free one.

## HTTP

Every request needs `Authorization: Bearer $RARE_TOKEN` and `Host: 127.0.0.1:<port>` (or `localhost:<port>`). Bodies are JSON with `Content-Type: application/json`, at most 32 KB. Each connection carries one request.

```
GET  /v1/me
GET  /v1/inventory
POST /v1/sell     { "itemId": "0x..", "mode": "best-offer" | "list", "priceMist": "..."? }
POST /v1/buy      { "itemId"?: "0x..", "collection"?: "<Move type>", "kind"?: 3, "maxMist"?: "..." }
POST /v1/claim    { "voucher": "<base64 or base64url>" }
POST /v1/open     { "view": "item" | "collection" | "trophies" | "store" | "overlay", "id"?: "..." }
GET  /v1/notices  (text/event-stream)
```

`GET /v1/licence` arrives with Rare Store (build item 13). Until then it answers 404.

### `GET /v1/me`

```json
{ "address": "0x7aa0...d784", "username": "ghost", "network": "mainnet", "canSign": true,
  "game": { "id": "web:pawtato-land", "slug": "pawtato-land", "title": "Pawtato Land" } }
```

`canSign` is false for a watch-only account. With no Rare Wallet account the call fails with `no_account` (409).

### `GET /v1/inventory`

```json
{ "items": [ { "id": "0x7c2c...", "name": "Pawtato Land #742", "type": "0x..::land::Land", "imageUrl": "...",
  "collection": { "slug": "pawtato-land", "name": "Pawtato Land" }, "kioskId": "0x...", "owner": "0x...",
  "listing": null, "lastSaleMist": "35020000000", "topOfferMist": "31000000000", "floorMist": "35019897000" } ] }
```

Only the player's items in **this game's collections** (the game's entry in rare.market's games catalog), with floor, best offer and last sale. Items use rare.market's `/v1` `ItemCard` shape, plus `floorMist`.

### `POST /v1/sell`

Raises Rare over the game (gamescope's base layer in Game Mode) and shows the overlay's item sheet: sell into the best offer, or list at floor, floor +10% or any price. `mode` and `priceMist` preset the sheet; the player can still pick the other option. The call answers when the player is done:

```json
{ "status": "sold", "digest": "8x...", "receivedMist": "30069000000" }
{ "status": "listed", "digest": "Hk..." }
{ "status": "cancelled" }
{ "status": "unavailable", "reason": "You don't hold this item" }
```

An item that isn't from this game is refused with `not_this_game` (403). The sale uses the existing builders against Tradeport, Drip and kiosk offers, and the listing goes into the player's own kiosk.

### `POST /v1/buy`

One `itemId`, or the cheapest listing (all-in price) in this game's collections, narrowed by `collection` and by `kind` (a `rare_items` kind, read from the item's `kind` attribute), at or under `maxMist`. The player's own listings are skipped. The Marketplace's buy sheet shows the price, fees and royalty.

```json
{ "status": "bought", "digest": "...", "itemId": "0x..." }
{ "status": "cancelled" }
{ "status": "unavailable", "reason": "Nothing listed at or under maxMist" }
```

### `POST /v1/claim`

Submits a studio voucher (see [Vouchers](#vouchers)). The player confirms on a one-line sheet, then reviews the transaction in Rare Wallet.

```json
{ "status": "claimed", "digest": "..." }
{ "status": "cancelled" }
{ "status": "rejected", "reason": "This voucher is for another account" }
{ "status": "unavailable", "reason": "Rare items aren't live yet" }
```

`rejected`: expired, for another account, a state voucher for an older revision of the item, or a class or item that isn't Rare's. `unavailable`: the package isn't published yet (`rare_items`, or Rare Trophies), so every item, state and trophy claim answers `unavailable` until the owner deploys them. A voucher for a class outside this game's collections is refused with `not_this_game`.

Gas: see [Network fees](#network-fees). A trophy claim the game's budget doesn't pay can still go through Rare's own cover (category `trophy`) when the wallet is short of SUI.

### `POST /v1/open`

| view | id | Opens |
|---|---|---|
| `item` | object id (required) | The item's Marketplace page |
| `collection` | slug or Move type (required) | The collection |
| `trophies` | game slug (default: this game) | The trophy case for the game |
| `store` | game slug (default: this game) | The Store page (the Sui Games page while the Store is hidden) |
| `overlay` | none | The overlay over the game |

Answers `{ "ok": true }`. Pages open in the launcher, which comes in front of the game the way Guide does; the game keeps running.

### `GET /v1/notices`

A server-sent event stream: `event: ready` once, then `event: notice` for `offer_received`, `sold` and `bought` notices about **this game's** items (rare.market's live notify stream, about half a second after the checkpoint), and a `: ping` comment every 15 seconds.

```
event: notice
data: {"id":"...","kind":"offer_received","title":"Offer: 31 SUI for Land #812","body":null,"itemId":"0x...","item":{"id":"0x...","name":"Land #812","imageUrl":"...","collection":{"slug":"pawtato-land","name":"Pawtato Land"}},"amountMist":"31000000000","digest":"...","at":1793000000000}
```

### Errors

Every error answers `{ "error": { "code": "<code>", "message": "<a sentence>" } }`.

| Status | code | When |
|---|---|---|
| 400 | `bad_request` | Body or parameters wrong, unreadable voucher |
| 401 | `unauthorized` | No token, or not a current one |
| 403 | `origin` | A native token from a web page, or a web token from another origin |
| 403 | `not_connected` | A web game read the player before they connected Rare Wallet to it |
| 403 | `not_this_game` | An item, collection or item class outside this game |
| 404 | `not_found` | No such route (or `/v1/licence`, for now) |
| 405 | `method` | Wrong HTTP method |
| 409 | `busy` | A Rare sheet from this game is already open |
| 409 | `no_account`, `watch_only` | No Rare Wallet account, or one that can't sign |
| 415 | `bad_request` | A body that isn't `application/json` |
| 421 | `wrong_host` | `Host` isn't the loopback address and this port |
| 429 | `rate_limited` | Too many calls; `Retry-After` says when to retry |
| 503 | `starting`, `unavailable`, `offline` | The launcher UI is still starting, or rare.market can't be reached |
| 504 | `timeout` | A read the UI didn't answer within 30 s |

A sheet left open for 10 minutes answers `{ "status": "cancelled" }`.

### Security

- **Token**: 256 random bits per launch, bound to one game, compared in constant time. After 30 bad or missing tokens in a minute, every request waits out the rest of that minute, so guessing gains nothing.
- **Origin**: a native token refuses any request that carries an `Origin` header, so a web page elsewhere on the device can't use a game's token. A web game's token takes only its own origin. CORS preflights are answered only for origins of running web games.
- **Host**: must be `127.0.0.1`, `localhost` or `[::1]` with our port, which stops DNS rebinding.
- **Rate limits**, per token: reads (`me`, `inventory`) 20 at once then 2 a second; sheets (`sell`, `buy`, `claim`, `open`) 5 at once then one every 6 seconds, one open at a time; two notice streams.
- **Scope**: everything is limited to the game's own collections from rare.market's games catalog.
- **Privacy for web games**: `me`, `inventory` and `notices` need the player to have connected Rare Wallet to the game first (the same approval as Wallet Standard `connect`). Native games can read them because the player launched them from Rare.

The checks are in `src-tauri/src/localapi/` (`gate` runs them in order: host, preflight, route, lockout, token, origin, connect, body, parameters, rate limits) and are unit-tested in `auth.rs`, `http.rs` and `mod.rs`.

## Network fees

Studios fund a gas budget for their game at rare.market (Studio > Gas; market `api/src/sponsor/game.ts`). The launcher asks it to pay, with no game code:

- **What asks**: every transaction a web game has Rare Wallet sign **and execute** (`sui:signAndExecuteTransaction` and the older name), and every `sell`, `buy` and `claim` a game makes over the Local API or `rare:platform`. `signTransaction` alone can't be paid this way: the game submits it, so only the player can pay its fee.
- **How**: `GET /v1/sponsor/status?address=&game=<slug>` (cached 30 s) says whether the game's budget covers this player now; only then `POST /v1/sponsor` with `category: "game:<slug>"` and the transaction kind (for a web game's prebuilt bytes, their kind; the player's gas data is dropped). The review sheet shows those exact sponsored bytes, and rare.market submits them with both signatures.
- **Who pays, on the review sheet**: "Network fee: Paid by <game>" when the budget pays, "Paid by Rare" for Rare's own cover, the fee in SUI otherwise.
- **When the answer is no** (503 unavailable, 403 not covered by the game's policy, 429 limits, or the status says no), the player pays as before. `budget_exhausted` adds "<game>'s gas budget is used up, so you pay this fee." under the fee, and the per-player daily limit "<game> has covered as many of your fees as it can today, so you pay this one."
- The game's policy covers calls into its own verified packages, `rare_items` claims for its classes and its trophies. Marketplace sells and buys go to Tradeport, Drip or kiosk contracts, so the budget refuses them today and the player pays.
- Code: `launcher/src/wallet/sponsor.ts` (`sponsorForGame`, `gameNotCovered`), `wallet/sign.ts` (`signAndExecuteForGame`), tests in `wallet/sponsor.game.test.ts` against a mocked API with the market's shapes and codes.

## `rare:platform` (web games)

The Rare Wallet the launcher injects into every web game (`src-tauri/src/webgame.js`) has a ninth Wallet Standard feature:

```js
const rare = wallet.features["rare:platform"];
rare.version;                 // "1.0.0"
rare.game;                    // this game's slug
rare.api;                     // { url, token } for loopback HTTP, or null
await rare.me();
await rare.inventory();
await rare.sell({ itemId, mode: "list", priceMist: "1500000000" });
await rare.buy({ kind: 3, maxMist: "2000000000" });
await rare.claim({ voucher });          // or rare.claim(voucherString)
await rare.open({ view: "trophies" });  // or rare.open("item", "0x...")
const off = rare.on("notice", (n) => toast(n.title));
```

- Every method resolves with the same JSON as the HTTP route, or rejects with an `Error` whose message is the sentence above.
- Calls leave through the same blocked `rare-wallet://` navigation as wallet requests, and the launcher checks the window's real origin and this launch's token (injected into the window, kept off `window`).
- `on("notice", cb)` starts notices for the page. They arrive only while the player has the game connected to Rare Wallet.

## Vouchers

A voucher is what a studio's server signs with its ed25519 game key, as made by `@rare/sdk/server` (`itemVoucher`, `stateVoucher`, `trophyVoucher`). The game server sends the token to the client as base64url; `@rare/sdk` passes it to `claim` as padded base64. The launcher reads either.

The token is the base64url of this JSON:

```json
{ "v": 1, "type": "item",   "sig": "<base64, 64 bytes>", "class": "0x<Class id>", "player": "0x...", "kind": "3",
  "keys": ["power"], "vals": ["12"], "nonce": "7", "expiresMs": "1800000600000" }
{ "v": 1, "type": "state",  "sig": "...", "item": "0x<Item id>", "rev": "4", "keys": ["power"], "vals": ["13"], "nonce": "8", "expiresMs": "..." }
{ "v": 1, "type": "trophy", "sig": "...", "registry": "0x...", "player": "0x...", "game": "0x<trophies Game id>", "trophyId": "2", "nonce": "9", "expiresMs": "..." }
```

- u64 values are decimal strings. Keys are unique and line up with vals.
- The signature is over the raw message bytes (`RARE_ITEM_V1`, `RARE_ITEM_STATE_V1`, `RARE_TROPHY_V1`). The launcher doesn't check it; the Move package does, on chain.
- The voucher carries only signed fields. What the transaction also needs is read from chain, never taken from the voucher: an item claim reads the `Class<C>` object (the marker type `C` and its `TransferPolicy` id from `policy_cap.policy_id`); a state claim reads the `Item<C>` (its class and current `rev`).
- **Item**: `mint::claim<C>` into the player's plain kiosk, or `mint::claim_fresh<C>` when they have none.
- **State**: `state::apply<C>` on the item, borrowed from the player's kiosk with `kiosk::borrow_val` and put back with `return_val`, or held in the wallet. Items in a personal kiosk can't be updated yet.
- **Trophy**: `game::mint_attested`, creating the player's Profile in the same transaction when they have none (the Rare Trophies claim builder).
- The `rare_items` package id lives in `launcher/src/localapi/items.json` and the trophies deployment in `launcher/src/trophies/deployment.json`. Both are filled by the deploy kit. While empty, claims answer `unavailable`.

`@rare/sdk`'s documented test vector (`packages/sdk/test/server.test.ts`) is decoded in the launcher's tests (`src/localapi/localapi.test.ts`), so both sides read the same bytes.

## `rare://` links

| Link | Opens | Web twin |
|---|---|---|
| `rare://item/<objectId>[?sheet=sell\|buy]` | Item page, optionally with its sell or buy sheet | `https://rare.market/item/<objectId>` |
| `rare://collection/<slug or type>` | Collection | `https://rare.market/collection/<slug>` |
| `rare://game/<slug>` | Sui Games page | `https://rare.market/games/<slug>` |
| `rare://store/<slug>` | Store page | `https://rare.market/store/<slug>` |
| `rare://trophies/<slug>` | That game's trophies | `https://rare.market/games/<slug>#trophies` |
| `rare://u/<address or username>` (also `rare://profile/...`) | Profile (yours, or the player's Marketplace page) | `https://rare.market/u/<address>` |
| `rare://overlay[?tab=items\|market\|trophies\|notices]` | The overlay over the game on screen | none |
| `rare://claim?v=<voucher>` | The claim sheet | `https://rare.market/claim?v=` |

- **Parsing** (`src-tauri/src/localapi/deeplink.rs`): ids, slugs, types, usernames and vouchers are checked before the UI sees them; anything else is ignored.
- **Fire-and-forget**: a link opens a page or a sheet, nothing more. A sell, buy or claim sheet from a link still needs the player, and the review and PIN after it.
- **Registration**:
  - Rare OS: `/usr/share/applications/rare-launcher.desktop` declares `MimeType=x-scheme-handler/rare;`, and the image build runs `update-desktop-database` and fails without the association.
  - `.deb`, `.rpm` and macOS bundles: Tauri's deep-link plugin (`plugins.deep-link.desktop.schemes` in `tauri.conf.json`). The AppImage writes its own entry at start (`register_all`).
- **Delivery**: `xdg-open rare://...` starts `rare-launcher rare://...`. When Rare is already running, the single-instance plugin hands the arguments to it. On macOS the deep-link plugin delivers them. Links that arrive before the UI is listening are queued.
- **Web twins**: `rare-launcher https://rare.market/item/0x...` opens the same page in the launcher. Inside a web game window, `rare://` links and rare.market item, collection, game, store, trophies, profile and claim links open in Rare instead of navigating the game (unless the game itself is hosted on rare.market). rare.market's own pages can offer `rare://` first and fall back to the web page.

## Where it lives

| Piece | Files |
|---|---|
| Server, auth, rate limits, routing, deep links | `launcher/src-tauri/src/localapi/{mod,auth,http,deeplink}.rs` |
| Tokens at launch | `launcher/src-tauri/src/launch.rs` (`grant_native`), `webgame.rs` (`grant_web`) |
| `rare:platform` | `launcher/src-tauri/src/webgame.js`, `webgame.rs` (`platform` requests) |
| Launcher side: scoping, sheets, claims, links, notices | `launcher/src/localapi/{host,methods,voucher,claim,sheets,notices}.ts`, `GameSheets.tsx` |
| Reused | `src/overlay/{data,trade}.ts` and the overlay item sheet, `src/marketplace/BuySheet.tsx`, `src/trophies/attest.ts`, Rare Wallet's review sheet |

## Network apps (tools and agents on your laptop)

Off by default. Settings > Developer > Allow network apps opens the API on the local network, on port 47100,
with one device token (shown in Settings with Copy and New token, stored in `~/.config/rare/network-api-token`).
New token stops the old one at once. Same bearer auth and JSON as above, with these differences:

- `Host` must be the device's IP address or its own name (`suiplay.local`), and a request with an `Origin`
  header (a web page) is refused. The listener has its own token, limits and lockout.
- Every capability is its own switch in Settings > Developer, all off until the owner turns them on. A call
  whose switch is off answers `403 capability_off` and names the switch:
  `{ "error": { "code": "capability_off", "capability": "launch", "message": "The owner has not switched on \"Launch and close games and apps\" in Settings > Developer" } }`.
- Network apps are not scoped to a game: inventory and sell/buy cover every collection.
- `claim`, `notices` and `open` with `view: "overlay"` are not available (`403 network_scope`).

| Switch (id) | What it does | Routes |
|---|---|---|
| Read profile (`profile`) | Username, address and balances. | `GET /v1/me` |
| Read inventory and library (`library`) | Items across games, installed games and apps, what is running. | `GET /v1/inventory`, `GET /v1/library` |
| Launch and close games and apps (`launch`) | Start and quit games and apps, open pages in Rare. | `POST /v1/launch`, `POST /v1/close`, `POST /v1/open` |
| Send controller input (`input`) | A virtual controller, to drive Rare and games. | `POST /v1/input` |
| Request purchases and listings (`purchase`) | Ask to buy or sell. Always opens the review sheet on the device. | `POST /v1/sell`, `POST /v1/buy` |
| Screenshots (`screenshot`) | Capture the screen. | `GET /v1/screenshot` |
| Change settings (`settings`) | Power profile, brightness, volume, Wi-Fi, Do not disturb (MCP tools). | `POST /v1/mcp` |
| Friends and messages (`friends`) | See friends, invite them to a party, send a message (MCP tools). | `POST /v1/mcp` |

`GET /v1/capabilities` is always answered and lists every switch with its `enabled` state and routes, so a
tool can find out what it may do before it tries.

### MCP (`POST /v1/mcp`)

The same tools Ask Rare uses (launcher/crates/rare-agent `tools.rs`), over MCP's Streamable HTTP transport in
its single-response form: one JSON-RPC message (or a batch) per POST, the answer as JSON, `202` for a
notification. Same token and Host rules as above; no session id. `tools/list` lists only the tools whose
switch is on (and `market_search`, public data); a call to one whose switch is off answers `isError: true`
naming the switch. `wallet_send`, `wallet_swap`, `market_buy`, `market_sell` and `market_list` need
`purchase` and only open Rare Wallet's review sheet on the device: nothing is signed without a press there.
Each call shows in Settings > Developer > Recent network activity as `mcp <tool>`.

```
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"launch_game","arguments":{"id":"steam:620"}}}
```

A client config for an MCP host: URL `http://suiplay.local:47100/v1/mcp`, header
`Authorization: Bearer <the token from Settings>`.

### Routes

```
GET  /v1/capabilities
GET  /v1/me
GET  /v1/inventory
GET  /v1/library
POST /v1/launch      { "id": "steam:620" | "rare:<slug>" | "app:<id>" | <library id> }
POST /v1/close       { "id"?: "<run id>" }              (no id: what is running now)
POST /v1/open        { "view": "item" | "collection" | "trophies" | "store", "id"?: "..." }
POST /v1/input       { "events": [ ... ] }  or one event
POST /v1/sell        { "itemId": "0x..", "mode": "best-offer" | "list", "priceMist": "..."? }
POST /v1/buy         { "itemId"?: "0x..", "collection"?: "<Move type>", "kind"?: 3, "maxMist"?: "..." }
GET  /v1/screenshot  (answers image/webp)
```

`GET /v1/me` answers the same fields as for a game, with `game: null` and the balances:

```json
{ "address": "0x7aa0...d784", "username": "ghost", "network": "mainnet", "canSign": true, "game": null,
  "balances": [ { "coinType": "0x2::sui::SUI", "symbol": "SUI", "name": "Sui", "decimals": 9, "raw": "12500000000", "amount": 12.5 } ] }
```

`balances` is `null` when the chain can't be reached. Prices are not included.

`GET /v1/inventory` is the player's items in every collection (up to 180, with `floorMist` only where known).

`GET /v1/library`:

```json
{ "games": [ { "id": "steam:620", "title": "Portal 2", "source": "steam", "lastPlayed": 1761500000, "playtimeMinutes": 840, "running": false } ],
  "apps":  [ { "id": "app:netflix", "name": "Netflix", "running": false } ],
  "running": [ { "id": "steam:620", "title": "Portal 2" } ] }
```

`POST /v1/launch` answers `{ "ok": true, "id", "title" }` once Rare has started it (poll `/v1/library` for
`running`). `404 not_found` for an id that isn't in the library, isn't installed or isn't an app. `POST
/v1/close` quits a running game or app: `{ "ok": true, "id", "title" }`, or `404 not_found` when nothing runs.

### Controller input

`POST /v1/input` takes up to 16 events in order. Buttons: `a b x y lb rb lt rt up down left right start select
guide l3 r3`. Axes: `left_x left_y right_x right_y` (-1 to 1, up is positive) and `lt rt` (0 to 1).

```json
{ "events": [
  { "button": "a" },
  { "button": "down", "action": "down" },
  { "button": "down", "action": "up" },
  { "button": "b", "action": "tap", "ms": 120 },
  { "axis": "left_x", "value": 0.8 } ] }
```

`action` is `tap` (press, wait `ms`, release; default 80 ms, 10 to 1000, and at most 3000 ms of taps in one
call), `down` or `up`. Answers `{ "ok": true, "events": 5, "via": "virtual-pad" }`.

- While the switch and the network listener are on, Rare creates a virtual Xbox 360 style pad ("Xbox 360
  Controller (Rare remote)") on `/dev/uinput`. Rare's own input reads it like a held controller, so a remote
  drives the launcher and menus the same way, and games see one more pad. Turning the switch off removes it.
- With no `/dev/uinput` the call falls back to the launcher's own input (`via: "launcher"`): button presses
  move Rare's focus, sticks do nothing, and games are not reached.
- Whatever a remote leaves held (a `down` without an `up`, a stick off centre) is released after 5 seconds
  without input, and when the switch goes off.
- Rare shows a controller icon in the top bar while a remote is sending input.

### Purchases and listings

`sell` and `buy` open the same sheets a game's call does, on the device, with Rare brought to the front. The
call waits (up to 10 minutes) and answers when the owner is done, in the same shapes as above (`sold`,
`listed`, `bought`, `cancelled`, `unavailable`). Every one shows the sheet and Rare Wallet's review sheet and
PIN (or Ledger, Keystone, passkey) after it. Nothing is ever signed without a press on the device, and an
unattended request just expires as `cancelled`. One sheet is open at a time (`409 busy`).

### Screenshots

`GET /v1/screenshot` answers the screen as `image/webp`. The capture is not kept in Captures. It needs a screenshot tool on the device (the same one the
Rare + Y chord uses); without one the answer is `503 unavailable`.

### Limits

| Call | Limit |
|---|---|
| `me`, `inventory`, `library`, `capabilities` | 20 at once, then 2 a second |
| `open`, `sell`, `buy` | 5 at once, then 10 a minute; one open at a time |
| `launch`, `close` | 5 at once, then one every 2 seconds |
| `input` | 100 events at once, then 100 events a second (a call of 16 costs 16) |
| `screenshot` | 2 at once, then one every 2 seconds |
| bad tokens | 30 a minute across all callers, then everyone unauthenticated waits a minute |

A call over its limit answers `429 rate_limited` with `Retry-After`.

### Audit

Every call that reaches a switch writes one line to the launcher log (`[netapi] cap=launch launch steam:620 ok
from 192.168.0.50`), including calls refused because the switch was off. Input writes one line per burst of
input, not per call. Settings > Developer > Recent network activity lists the latest calls: when, what,
which switch, from which address, and whether it was allowed.

### Example

`tools/netapi-example/rare_netapi.py` is a small client with no dependencies (Python 3.8+) that an agent on a
laptop can import or run:

```
export RARE_HOST=suiplay.local RARE_TOKEN=<token from Settings>
python3 tools/netapi-example/rare_netapi.py caps
python3 tools/netapi-example/rare_netapi.py library
python3 tools/netapi-example/rare_netapi.py tap a
python3 tools/netapi-example/rare_netapi.py shot screen.webp
```

See `docs/SSH.md` for turning the listener on and the firewall.
