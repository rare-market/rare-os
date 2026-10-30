// The kinds of notice that have a switch in Settings, in the order shown.

import type { NoticeKind } from "./types";

export const KINDS: Array<{ kind: NoticeKind; title: string; body: string }> = [
  { kind: "sold", title: "Sold", body: "One of your items sold, or someone took your listing." },
  { kind: "bought", title: "Bought", body: "You bought an item, or your offer was accepted." },
  { kind: "offer_received", title: "Offers", body: "A new offer on an item or collection you hold." },
  { kind: "coin_received", title: "Coins received", body: "Someone sent you coins, above $1." },
  { kind: "nft_received", title: "NFTs received", body: "Someone sent you an item from a collection on rare.market." },
  { kind: "airdrop", title: "Airdrops", body: "An item minted straight to you." },
  { kind: "perp_liquidation", title: "Perps liquidation", body: "A perps position is near its liquidation price." },
  { kind: "price_alert", title: "Price alerts", body: "A price alert you set was hit." },
  { kind: "followed_trade", title: "Followed traders", body: "A trader you follow made a trade." },
  { kind: "friend_request", title: "Friend requests", body: "Someone wants to be your friend." },
  { kind: "friend_online", title: "Friends online", body: "A friend came online or started a game." },
  { kind: "party_invite", title: "Party invites", body: "A friend invited you to their party." },
  { kind: "party_message", title: "Party messages", body: "A new message in your party chat." },
  { kind: "os_update", title: "Rare OS updates", body: "An update is ready and installs when you restart." },
  { kind: "download_done", title: "Downloads", body: "A Steam download finished." },
  { kind: "emu_update", title: "Emulator updates", body: "A newer version of an emulator is available." },
  { kind: "trophy", title: "Trophies", body: "A trophy was earned." },
];
