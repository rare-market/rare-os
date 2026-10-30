// What a read-only scan of a Playtron (GameOS) wallet finds, and the plan to move it.
//
// The scan only reads public chain state over gRPC. The plan is a list of unsigned
// transactions with the Playtron address as sender; they are signed on Playtron's own
// surface (GameOS, or Playtron's sign-in page), never inside Rare.

export interface RescueCoin {
  coinType: string;
  symbol: string;
  name: string;
  decimals: number;
  iconUrl: string | null;
  /** total in base units: coin objects plus address balance */
  raw: bigint;
  /** held as Coin<T> objects */
  coinRaw: bigint;
  /** held in the address balance (no object) */
  addressRaw: bigint;
  amount: number;
  /** Coin<T> objects, largest first */
  objectIds: string[];
  /** balance of each entry of objectIds, same order */
  objectBalances?: bigint[];
}

export interface RescueItem {
  objectId: string;
  type: string;
  name: string;
  image: string | null;
  /** collection name from Display or the type's module */
  collection: string;
}

export interface RescueKioskItem extends RescueItem {
  /** listed for sale, price in MIST */
  listed: bigint | null;
  /** locked by the collection's transfer policy (only leaves through its rules) */
  locked: boolean;
  /** the object type has `store` (false only for odd key-only items placed in a kiosk) */
  transferable: boolean;
  /** listed through a marketplace's purchase cap: only that marketplace can cancel it */
  exclusive?: boolean;
}

export interface RescueKiosk {
  kioskId: string;
  /** the owned cap object: KioskOwnerCap, or PersonalKioskCap for a personal kiosk */
  capId: string;
  /** personal kiosks are bound to their owner; their cap can't be sent */
  personal: boolean;
  /** package of the personal kiosk module, when personal */
  personalPackage: string | null;
  items: RescueKioskItem[];
  /** sale proceeds held in the kiosk, MIST */
  profits: bigint;
}

export interface RescueStake {
  objectId: string;
  /** MIST */
  principal: bigint;
  poolId: string;
  activationEpoch: number;
}

export interface RescueName {
  objectId: string;
  /** e.g. alice.sui */
  domain: string;
  expiresMs: number;
  subname: boolean;
}

export interface RescueScan {
  address: string;
  scannedAt: number;
  /** SuiNS name pointed at this address (reverse record), if any */
  defaultName: string | null;
  coins: RescueCoin[];
  nfts: RescueItem[];
  kiosks: RescueKiosk[];
  staked: RescueStake[];
  names: RescueName[];
  /** owned objects whose type has `key` but not `store`: only their own package can move them */
  soulbound: RescueItem[];
  /** owned objects the scan didn't page through (more than the cap) */
  truncated: boolean;
}

export type MoveKind = "kiosk" | "stake" | "name" | "nft" | "coin";

/** One entry of what a batch moves, for the review list. */
export interface MoveLine {
  kind: MoveKind;
  /** object id, kiosk id or coin type */
  id: string;
  label: string;
  /** e.g. "12 items inside", "1.5 SUI", "listed for 3 SUI: delisted first" */
  detail?: string;
}

export interface MoveBatch {
  /** 1-based, in the order to sign */
  index: number;
  title: string;
  lines: MoveLine[];
}

export interface StuckLine {
  id: string;
  label: string;
  /** why Rare can't move it, in plain words */
  reason: string;
}

export interface MovePlan {
  from: string;
  to: string;
  batches: MoveBatch[];
  /** things that stay behind */
  stuck: StuckLine[];
  /** true when nothing movable is left */
  empty: boolean;
  /** set when the wallet pays its own fees and holds too little SUI for every step */
  feeNote: string | null;
}

/** Who pays gas for the move transactions. Playtron sponsors some users' gas; others pay their own. */
export type GasMode = "own" | "sponsored";
