// rare.market friends API types (contract: market api/src/friends, /v1/friends, /v1/presence,
// /v1/players). A player is their Sui address, shown by SuiNS name and NFT picture.

export const FRIENDS_BASE = "https://rare.market/v1";

export type GameKind = "web" | "steam" | "other";

/** What someone is playing: the launcher's game id, its title, and how to start it elsewhere. */
export interface GameRef {
  id: string;
  title: string;
  kind: GameKind;
  appid?: number;
  url?: string;
  /** An invite value a web game takes in its URL, when the game supports one. */
  join?: string;
}

export interface Presence {
  state: "online" | "away" | "offline";
  game: GameRef | null;
  since: number | null;
  lastSeen: number | null;
}

export interface Person {
  address: string;
  /** What to show: the Rare username, else the SuiNS name. */
  name: string | null;
  /** The Rare username, picked by the player. */
  username?: string | null;
  /** The default SuiNS name, when the player also owns one. */
  suins?: string | null;
  avatarUrl: string | null;
}

export interface Friend extends Person {
  since: number;
  presence: Presence;
}

export interface Pending extends Person {
  at: number;
}

export interface Privacy {
  showPlaying: boolean;
  appearOffline: boolean;
  visibility: "friends" | "everyone";
}

export interface PartyMember extends Person {
  state: "invited" | "joined";
  presence: Presence;
}

export interface Party {
  id: string;
  host: string;
  game: GameRef | null;
  createdAt: number;
  members: PartyMember[];
}

export interface Message {
  id: string;
  partyId: string;
  from: string;
  text: string;
  at: number;
}

export interface Overview {
  me: { address: string; settings: Privacy; presence: Presence };
  friends: Friend[];
  received: Pending[];
  sent: Pending[];
  blocked: Pending[];
  parties: Party[];
}

/** Live events on the notify stream (`?social=` token). */
export type SocialEvent =
  | { event: "presence"; data: { address: string; presence: Presence } }
  | { event: "friend"; data: { change: "request" | "accepted" | "cancelled" | "removed" | "sync"; address: string } }
  | { event: "party"; data: { change: "invite" | "update" | "game" | "closed" | "left"; party?: Party; id?: string; from?: string } }
  | { event: "chat"; data: Message };

export type GraphOp = "request" | "accept" | "decline" | "cancel" | "remove" | "block" | "unblock";
