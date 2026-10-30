// Wallet NFTs. The grid comes from Rare Marketplace (names, images, collections, listings);
// the detail screen adds the chain's view over gRPC: type, owner, and whether the type has
// `store` (without it the object is bound to this account and can't be sent).

import { normalizeSuiAddress } from "@mysten/sui/utils";
import { native } from "../../lib/bridge";
import type { ItemCard, Page } from "../../lib/rare";
import { grpcClient } from "../grpc";
import { hasStore as chainHasStore } from "../tx/nftSend";

export type { ItemCard };

/** rare.market GET /v1/items/<id> (market/web/lib/api/types.ts Item, trimmed). */
export interface RareItem extends ItemCard {
  description?: string;
  media?: Array<{ url: string; kind: "image" | "video" }>;
  attributes?: Array<{ type: string; value: string; pct?: number; floorMist?: string | null }>;
  owner?: string | null;
  lastSaleMist?: string | null;
}

export type ChainOwner =
  | { kind: "address"; address: string }
  /** inside another object: a kiosk (dynamic field) or wrapped */
  | { kind: "object"; id: string }
  | { kind: "shared" }
  | { kind: "immutable" }
  | { kind: "unknown" };

export interface NftDetail {
  id: string;
  name: string;
  imageUrl: string | null;
  description: string;
  collection: ItemCard["collection"];
  attributes: NonNullable<RareItem["attributes"]>;
  rarity: ItemCard["rarity"] | null;
  listing: ItemCard["listing"] | null;
  kioskId: string | null;
  /** full Move type from the chain (rare.market's when the chain read failed) */
  type: string | null;
  owner: ChainOwner;
  inKiosk: boolean;
  /** undefined when the node did not say */
  hasStore: boolean | undefined;
  canSend: boolean;
  /** plain reason when canSend is false */
  cantSend?: string;
  /** rare.market had no data for it (shown from chain display only) */
  rareMissing: boolean;
}

/** Injectable reads, so tests run without Tauri or the network. */
export interface NftDeps {
  rareGet<T>(path: string): Promise<T>;
  getObject(o: { objectId: string; include: { display: true } }): Promise<{ object: { objectId: string; type: string; owner: unknown; display?: { output?: Record<string, unknown> | null } | null } }>;
  hasStore(type: string): Promise<boolean | undefined>;
}

const defaults = (): NftDeps => ({
  rareGet: (p) => native.rareGet(p),
  getObject: (o) => grpcClient.core.getObject(o) as never,
  hasStore: (t) => chainHasStore(t),
});

/** The wallet's NFTs for the grid, from Rare Marketplace. */
export async function walletNfts(address: string, opts: { limit?: number; deps?: Pick<NftDeps, "rareGet"> } = {}): Promise<Page<ItemCard>> {
  const get = opts.deps?.rareGet ?? defaults().rareGet;
  return get<Page<ItemCard>>(`/v1/accounts/${normalizeSuiAddress(address)}/items?limit=${opts.limit ?? 60}`);
}

function mapOwner(o: unknown): ChainOwner {
  const v = o as { $kind?: string; AddressOwner?: string; ConsensusAddressOwner?: { owner: string }; ObjectOwner?: string } | null;
  switch (v?.$kind) {
    case "AddressOwner":
      return { kind: "address", address: normalizeSuiAddress(v.AddressOwner!) };
    case "ConsensusAddressOwner":
      return { kind: "address", address: normalizeSuiAddress(v.ConsensusAddressOwner!.owner) };
    case "ObjectOwner":
      return { kind: "object", id: v.ObjectOwner! };
    case "Shared":
      return { kind: "shared" };
    case "Immutable":
      return { kind: "immutable" };
    default:
      return { kind: "unknown" };
  }
}

/**
 * rare.market item data plus the chain's view. `me` is the viewing account: an item it does not
 * hold directly or in a kiosk can't be sent from here. Throws only when both sources fail.
 */
export async function nftDetail(id: string, me?: string, deps: NftDeps = defaults()): Promise<NftDetail> {
  const objectId = normalizeSuiAddress(id);
  const [rare, chain] = await Promise.all([
    deps.rareGet<RareItem>(`/v1/items/${objectId}`).catch(() => null),
    deps.getObject({ objectId, include: { display: true } }).then((r) => r.object).catch(() => null),
  ]);
  if (!rare && !chain) throw new Error("Could not load this item");
  const type = chain?.type ?? rare?.type ?? null;
  const owner = chain ? mapOwner(chain.owner) : { kind: "unknown" as const };
  const display = chain?.display?.output ?? {};
  const str = (k: string) => (typeof display[k] === "string" ? (display[k] as string) : undefined);
  // a kiosk item is owned by a dynamic field (Field<Wrapper<kiosk::Item>, ID>) owned by the kiosk
  let kioskId = rare?.kioskId ?? null;
  let inKiosk = false;
  if (owner.kind === "object") {
    const field = await deps.getObject({ objectId: owner.id, include: { display: true } }).then((r) => r.object).catch(() => null);
    const parent = field ? mapOwner(field.owner) : null;
    if (field && /::kiosk::Item\b/.test(field.type) && parent?.kind === "object") {
      inKiosk = true;
      kioskId = normalizeSuiAddress(parent.id);
    } else inKiosk = !field && !!kioskId;
  }
  const store = type ? await deps.hasStore(type).catch(() => undefined) : undefined;

  let cantSend: string | undefined;
  if (!chain) cantSend = "Could not read it on chain. Try again in a moment";
  else if (owner.kind === "shared" || owner.kind === "immutable") cantSend = "It is shared on chain and has no owner to send it";
  else if (owner.kind === "address" && me && owner.address !== normalizeSuiAddress(me)) cantSend = "It is not in this wallet";
  else if (owner.kind === "object" && !inKiosk) cantSend = "It is held inside another object";
  else if (owner.kind === "address" && store === false) cantSend = "Can't send this one. It is bound to this account by its creator";

  return {
    id: objectId,
    name: rare?.name ?? str("name") ?? "Untitled",
    imageUrl: rare?.imageUrl ?? str("image_url") ?? str("img_url") ?? str("image") ?? null,
    description: rare?.description ?? str("description") ?? "",
    collection: rare?.collection ?? null,
    attributes: rare?.attributes ?? [],
    rarity: rare?.rarity ?? null,
    listing: rare?.listing ?? null,
    kioskId,
    type,
    owner,
    inKiosk,
    hasStore: store,
    canSend: !cantSend,
    ...(cantSend ? { cantSend } : {}),
    rareMissing: !rare,
  };
}
