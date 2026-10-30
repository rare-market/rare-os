// rare.market notify API types (contract: /v1/notify on rare.market, CORS open).

/** Chain kinds come from rare.market; `followed_trade` (Trade > Traders) and the friend kinds
 * (friends/live.ts, from the stream's friend events) are made here with notifyLocal. */
export type NoticeKind =
  | "sold"
  | "bought"
  | "offer_received"
  | "coin_received"
  | "nft_received"
  | "airdrop"
  | "perp_liquidation"
  | "price_alert"
  | "followed_trade"
  | "friend_request"
  | "friend_online"
  | "party_invite"
  | "party_message"
  | "os_update"
  | "download_done"
  | "emu_update"
  | "trophy"
  | "battery";

export const NOTICE_KINDS: NoticeKind[] = [
  "sold",
  "bought",
  "offer_received",
  "coin_received",
  "nft_received",
  "airdrop",
  "perp_liquidation",
  "price_alert",
  "followed_trade",
  "friend_request",
  "friend_online",
  "party_invite",
  "party_message",
  "os_update",
  "download_done",
  "emu_update",
  "trophy",
  "battery",
];

export type NoticeAction =
  | { kind: "item"; itemId: string; slug: string | null }
  | { kind: "collection"; slug: string }
  | { kind: "position"; marketId: string; symbol: string }
  | { kind: "coin"; coinType: string }
  | { kind: "tx"; digest: string }
  | { kind: "trader"; address: string }
  | { kind: "friends"; address?: string }
  | { kind: "party"; partyId: string }
  | { kind: "os_update" }
  | { kind: "downloads" }
  | { kind: "emulators" }
  | { kind: "trophies" }
  | { kind: "battery" };

export interface Notice {
  id: string;
  kind: NoticeKind;
  addr: string | null;
  title: string;
  body: string | null;
  imageUrl: string | null;
  amount: { raw: string; decimals: number; symbol: string; coinType: string; usd: number | null } | null;
  item: { id: string; name: string; imageUrl: string | null; collection: { slug: string; name: string } | null } | null;
  action: NoticeAction;
  digest: string | null;
  cpTs: number | null;
  createdAt: number;
  sentAt: number;
}

export interface PriceAlert {
  id: string;
  coinType: string;
  symbol: string;
  dir: "above" | "below";
  target: number;
  createdAt: number;
  firedAt: number | null;
  active: boolean;
}

export const NOTIFY_BASE = "https://rare.market/v1/notify";
