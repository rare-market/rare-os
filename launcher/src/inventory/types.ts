// Cross-game inventory: the player's collectibles from every Sui game, grouped by game.

import type { ItemCard } from "../marketplace/api";

export interface InvItem extends ItemCard {
  /** The collection's floor (cheapest all-in listing), MIST. */
  floorMist?: string | null;
}

export interface InvGame {
  slug: string;
  name: string;
  logoUrl: string;
}

export interface InvGroup {
  /** null: collectibles that belong to no listed game. */
  game: InvGame | null;
  items: InvItem[];
  /** Sum of collection floors over the items, MIST. A floor value, not an estimate. */
  floorValueMist: string;
}

export interface Inventory {
  groups: InvGroup[];
  total: number;
}

/** What the player just did, shown before the chain and the indexer catch up. */
export type InvOp =
  | { kind: "remove"; ids: string[]; why: "sold" | "sent" }
  | { kind: "list"; id: string; priceMist: string }
  | { kind: "delist"; id: string };

export interface Pending {
  key: string;
  op: InvOp;
  at: number;
}
