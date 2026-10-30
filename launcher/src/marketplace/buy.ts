// Buying. The confirm sheet builds the purchase with rare.market's own buy builder (ported in
// ./venues from the market repo), then hands it to Rare Wallet's signAndExecute, which shows the
// review sheet, asks for the PIN or the device and submits. Watch accounts can't sign: for them
// the sheet hands off to a phone (QR of the rare.market item page).

import type { Transaction } from "@mysten/sui/transactions";
import { native } from "../lib/bridge";
import type { RareAccount } from "../wallet";
import { grpcClient } from "../wallet/grpc";
import { canSignWithWallet } from "../wallet/market";
import { ownedKiosks, type NftChainReader } from "../wallet/tx/nft/context";
import type { ItemCard, Listing } from "./api";
import { api as storeApi } from "../store/api";
import { useStore } from "../store/state";
import { buildCartBuy, configureResale, loadTradeContext, normType, resaleLicenceType, type ApiListing, type RareGet } from "./venues";

export interface Quote {
  venue: string;
  priceMist: bigint;
  feeMist: bigint;
  royaltyMist: bigint;
  commissionMist: bigint;
  totalMist: bigint;
}

const big = (v: unknown): bigint => {
  try {
    return v == null || v === "" ? 0n : BigInt(String(v));
  } catch {
    return 0n;
  }
};

/** What the buyer pays, split the way rare.market's API prices a listing (all-in = price + fees + royalty). */
export function quoteOf(l: Listing): Quote {
  const price = big(l.priceMist);
  const fee = big(l.feeMist);
  const commission = big(l.commissionMist);
  const total = l.allInMist ? big(l.allInMist) : price + fee + commission + big(l.extra?.royaltyMist);
  // Royalty is whatever the all-in price carries beyond price and fees (kiosk policies vary).
  const royalty = l.extra?.royaltyMist != null ? big(l.extra.royaltyMist) : total - price - fee - commission;
  return { venue: l.venue, priceMist: price, feeMist: fee, royaltyMist: royalty < 0n ? 0n : royalty, commissionMist: commission, totalMist: total };
}

/** True for any account Rare Wallet can sign with. Watch accounts never can. */
export function canBuyInApp(account: RareAccount | null): boolean {
  return canSignWithWallet(account);
}

/**
 * Rare Store ResaleLicence: the resale rule needs the store package/origin (the store config the
 * launcher already holds, fetched only if the Store tab has not loaded it) and the licence's Game.
 * Returns the game id for a resale licence, undefined for any other item.
 */
async function resaleGameFor(itemId: string, nftType: string, get: RareGet): Promise<string | undefined> {
  const config = useStore.getState().config ?? (await storeApi.config().catch(() => null));
  if (config?.origin) configureResale({ origin: config.origin, pkg: config.package ?? config.origin });
  else return undefined;
  const t = resaleLicenceType();
  if (!t || normType(nftType) !== t) return undefined;
  const raw = await get<{ licence?: { game_id?: string | null } } | { game_id?: string | null }>(`/v1/store/licences/${encodeURIComponent(itemId)}`);
  const gameId = (raw as { licence?: { game_id?: string | null } }).licence?.game_id ?? (raw as { game_id?: string | null }).game_id;
  if (!gameId) throw new Error("Could not read this game licence.");
  return gameId;
}

/**
 * The one place a purchase transaction gets built: rare.market's `buildCartBuy` for one line.
 * The buyer's kiosks and the type's transfer policies come from rare.market's API; when the
 * kiosk list can't be read there it falls back to the chain (gRPC).
 */
export async function buildBuyTransaction(item: ItemCard, listing: Listing, sender: string, get: RareGet = native.rareGet): Promise<Transaction> {
  const l = listing as ApiListing;
  if (!l.nftType) throw new Error("This listing is still being indexed. Try again in a minute.");
  const core = (grpcClient as unknown as { core: NftChainReader }).core;
  const ctx = await loadTradeContext(get, sender, [l.nftType], (s) => ownedKiosks(core, s).then((r) => r.kiosks));
  const resaleGame = await resaleGameFor(item.id, l.nftType, get);
  return buildCartBuy([{ itemId: item.id, listing: l, resaleGame }], { sender, ...ctx });
}
