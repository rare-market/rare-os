// Rare Store shapes, exactly as rare.market's API sends them (rare-store docs/OFFCHAIN.md) and as
// the launcher's Rust side (src-tauri/src/store) answers. Amounts are strings in base units.

export interface StoreConfig {
  /** Current package id (calls). Null until the store is deployed. */
  package: string | null;
  /** Original package id (types, events). */
  origin: string | null;
  store: string | null;
  storeVersion: string | null;
  policy: string | null;
  policyVersion: string | null;
  usdcType: string | null;
  uploadsOpen: boolean;
}

export interface StorePrice {
  /** micro-USD */
  usd: string;
  sale_usd: string | null;
  sale_ends_ms: number | null;
  accept_sui: boolean;
}

export interface StoreGame {
  slug: string;
  kind: "link" | "uploaded";
  name: string;
  studio: { id: string | null; name: string };
  genre: string | null;
  description: string;
  status: "live" | "beta" | null;
  platforms: string[];
  images: { logo: string | null; banner: string | null; cover: string | null };
  links: { website?: string; x?: string; play?: string; steam?: string; epic?: string };
  price: StorePrice | null;
  game_id: string | null;
  featured: boolean;
  sales: number;
  created_ms: number | null;
}

export interface ApiBuild {
  platform: string;
  sha256: string;
  size: string;
}

export interface StoreGameDetail extends StoreGame {
  page: { about: string; media: Array<{ kind: "image" | "video"; url: string }>; tags: string[]; requirements: string | null } | null;
  latest: null | { version: number; label: string; edition: number; published_ms: number; builds: ApiBuild[] };
  edition: number | null;
  resale: null | { enabled: boolean; royalty_bps: number; min_hold_ms: number; min_royalty_mist: string };
  launch_check: boolean;
  listed: boolean;
  blocked: boolean;
}

export interface Licence {
  id: string;
  type: "licence" | "resale";
  game_id: string;
  slug: string;
  name: string;
  image_url: string;
  edition: number;
  version: number;
  purchased_ms: number;
  number: number;
  kiosk_id: string | null;
  refundable_until_ms: number | null;
  tradable_from_ms: number | null;
}

export interface SuiPrice {
  price_e8: string;
  conf_e8: string;
  publish_time: number;
  update_b64: string;
  /** true: the on-chain PriceInfoObject is within the Store's max age; buy_sui needs no Pyth update in the PTB. */
  fresh?: boolean;
  age_s?: number;
}

export type Shelf = "featured" | "new" | "top" | "sale" | "all";

/** One platform build as the chain records it (Game.editions[edition].builds). */
export interface ChainBuild {
  platform: string;
  /** hex, lowercase */
  sha256: string;
  size: bigint;
  path: string;
}

export interface ChainRelease {
  version: number;
  label: string;
  edition: number;
  builds: ChainBuild[];
  publishedMs: number;
}

/** The Game object fields the launcher needs, read over gRPC. */
export interface ChainGame {
  id: string;
  slug: string;
  name: string;
  priceUsd: bigint;
  salePriceUsd: bigint;
  saleEndsMs: number;
  acceptSui: boolean;
  listed: boolean;
  blocked: boolean;
  launchCheck: boolean;
  edition: number;
  latest: ChainRelease | null;
  editionsTable: string;
  resale: { enabled: boolean; royaltyBps: number; minHoldMs: number; minRoyaltyMist: bigint; royaltyTo: string };
}

/** An installed store game (src-tauri/src/store install.json). */
export interface Installed {
  id: string;
  slug: string;
  name: string;
  version: number;
  label: string;
  edition: number;
  licenceId: string;
  platform: string;
  sha256: string;
  size: number;
  runtime: "native" | "proton";
  exec: string;
  args: string[];
  launchCheck: boolean;
  images: { logo?: string | null; banner?: string | null; cover?: string | null };
  installedMs: number;
  checkedMs: number;
  owner: string;
  dir: string;
}

export interface Progress {
  slug: string;
  phase: "download" | "verify" | "unpack" | "done" | "error";
  done: number;
  total: number;
  message?: string | null;
}

export interface InstallRequest {
  slug: string;
  name: string;
  platform: string;
  address: string;
  message: string;
  signature: string;
  expectedSha256: string;
  expectedSize: number;
  version: number;
  label: string;
  edition: number;
  launchCheck: boolean;
  images: { logo: string | null; banner: string | null; cover: string | null };
}
