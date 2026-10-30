// The Local API's methods, launcher side (docs/LOCAL-API.md). Rust has already checked the caller
// (token, origin, rate limits) and the parameters' shape; here every call is scoped to the game
// that made it: its items are the ones in its catalog collections, nothing else. Sell and buy
// reuse the overlay's data (overlay/data.ts) and builders (overlay/trade.ts, marketplace/buy.ts);
// nothing here signs: the sheets do, through Rare Wallet's review.

import { normalizeSuiAddress } from "@mysten/sui/utils";
import type { ItemCard, ItemDetail, Listing, Page } from "../marketplace/api";
import { currentNetwork } from "../lib/network";
import { canonAddr } from "../sui/chain";
import { typeMatches } from "../sui/ownership";
import type { SuiGame } from "../lib/rare";
import type { Library } from "../lib/types";
import { trophyConfig } from "../trophies/config";
import { loadItems, type OverlayItem, type RareGet } from "../overlay/data";
import { resolveGame, type OverlayGame } from "../overlay/game";
import { decodeVoucher, itemsPackage, itemType, readClass, readItem, rejectReason, VoucherError, voucherTitle, type ObjectReader, type Voucher } from "./voucher";

/** A refusal with a code games can branch on (Rust maps it to an HTTP status). */
export class ApiFail extends Error {
  constructor(
    public code: "bad_request" | "not_this_game" | "not_found" | "no_account" | "watch_only" | "busy" | "unavailable" | "failed",
    message: string,
  ) {
    super(message);
    this.name = "ApiFail";
  }
}

export interface Account {
  address: string;
  kind: string;
  label?: string | null;
}

/** Everything the methods read, swapped for fakes in tests. */
export interface Deps {
  get: RareGet;
  account: () => Promise<Account | null>;
  canSign: (a: Account | null) => boolean;
  games: () => Promise<SuiGame[]>;
  library: () => Library | null;
  username: (address: string) => Promise<string | null>;
  /** An object's type and fields over gRPC (claims read the item class from chain). */
  object: ObjectReader;
}

const enc = encodeURIComponent;
const big = (v: string | null | undefined): bigint | null => {
  try {
    return v ? BigInt(v) : null;
  } catch {
    return null;
  }
};
const sameAddr = (a: string | null | undefined, b: string | null | undefined) => !!a && !!b && canonAddr(a) === canonAddr(b);
const allIn = (l: Listing) => big(l.allInMist) ?? big(l.priceMist) ?? 0n;

export async function gameFor(gameId: string | null, d: Deps): Promise<OverlayGame> {
  return resolveGame(gameId, d.library(), await d.games());
}

/** Whether a Move type is one of the game's items. */
export function owns(game: Pick<OverlayGame, "collections">, type: string | null | undefined): boolean {
  return !!type && game.collections.some((c) => typeMatches(type, c.nftType));
}

async function signer(d: Deps): Promise<Account> {
  const a = await d.account();
  if (!a) throw new ApiFail("no_account", "Add a Rare Wallet account first");
  return a;
}

// ---------------------------------------------------------------- me, inventory

export interface Me {
  address: string;
  username: string | null;
  network: "mainnet" | "testnet";
  canSign: boolean;
  game: { id: string | null; slug: string | null; title: string };
}

export async function me(gameId: string | null, d: Deps): Promise<Me> {
  const [a, game] = await Promise.all([signer(d), gameFor(gameId, d)]);
  const username = await d.username(a.address).catch(() => null);
  return { address: normalizeSuiAddress(a.address), username, network: currentNetwork(), canSign: d.canSign(a), game: { id: gameId, slug: game.slug, title: game.title } };
}

export async function inventory(gameId: string | null, d: Deps): Promise<{ items: OverlayItem[] }> {
  const a = await signer(d);
  const game = await gameFor(gameId, d);
  if (!game.collections.length) return { items: [] };
  return { items: (await loadItems(d.get, a.address, game)).items };
}

// ---------------------------------------------------------------- sell

export type Unavailable = { status: "unavailable"; reason: string };

export interface SellSheet {
  game: OverlayGame;
  item: OverlayItem;
  mode: "best-offer" | "list";
  priceMist: bigint | null;
}

/** The player's item from this game, ready for the sell sheet; or why it can't be sold here. */
export async function prepareSell(gameId: string | null, p: { itemId: string; mode?: string; priceMist?: string | null }, d: Deps): Promise<SellSheet | Unavailable> {
  const a = await signer(d);
  if (!d.canSign(a)) throw new ApiFail("watch_only", "This Rare Wallet account is watch only");
  const game = await gameFor(gameId, d);
  const id = normalizeSuiAddress(p.itemId);
  const mine = game.collections.length ? (await loadItems(d.get, a.address, game)).items : [];
  const item = mine.find((x) => normalizeSuiAddress(x.id) === id);
  if (!item) {
    const card = await d.get<ItemCard>(`/v1/items/${enc(id)}`).catch(() => null);
    if (card && !owns(game, card.type)) throw new ApiFail("not_this_game", "That item isn't from this game");
    return { status: "unavailable", reason: card ? "You don't hold this item" : "That item doesn't exist" };
  }
  return { game, item, mode: p.mode === "list" ? "list" : "best-offer", priceMist: big(p.priceMist ?? null) };
}

// ---------------------------------------------------------------- buy

export interface BuySheetData {
  game: OverlayGame;
  item: ItemCard;
  listing: Listing;
}

/** Checks a pick against maxMist and the player's own listings. */
function buyable(card: ItemCard, me: string | null, max: bigint | null): string | null {
  if (!card.listing) return "That item isn't for sale";
  if (me && sameAddr(card.listing.seller, me)) return "That's your own listing";
  if (max !== null && allIn(card.listing) > max) return "It costs more than maxMist";
  return null;
}

/** The cheapest listing in some of the game's collections (by all-in price), or null. */
async function cheapest(collections: string[], me: string | null, max: bigint | null, d: Deps, pick?: (c: ItemCard) => Promise<boolean>): Promise<ItemCard | null> {
  const lists = await Promise.all(
    collections.map(async (type) => {
      const slug = (await d.get<{ slug?: string | null }>(`/v1/collections/${enc(type)}`).catch(() => null))?.slug;
      if (!slug) return [] as ItemCard[];
      return (await d.get<Page<ItemCard>>(`/v1/collections/${enc(slug)}/items?sort=price_asc&limit=24`).catch(() => ({ data: [] as ItemCard[] }))).data;
    }),
  );
  const cards = lists
    .flat()
    .filter((c) => !buyable(c, me, max))
    .sort((x, y) => (allIn(x.listing!) < allIn(y.listing!) ? -1 : allIn(x.listing!) > allIn(y.listing!) ? 1 : 0));
  if (!pick) return cards[0] ?? null;
  for (const c of cards.slice(0, 12)) if (await pick(c)) return c;
  return null;
}

/** What `buy` will offer the player: one item, or the cheapest of a collection or kind under maxMist. */
export async function prepareBuy(gameId: string | null, p: { itemId?: string | null; collection?: string | null; kind?: number | null; maxMist?: string | null }, d: Deps): Promise<BuySheetData | Unavailable> {
  const a = await d.account();
  const me = a?.address ?? null;
  const game = await gameFor(gameId, d);
  const max = big(p.maxMist ?? null);
  if (p.itemId) {
    const card = await d.get<ItemCard>(`/v1/items/${enc(normalizeSuiAddress(p.itemId))}`).catch(() => null);
    if (!card) return { status: "unavailable", reason: "That item doesn't exist" };
    if (!owns(game, card.type)) throw new ApiFail("not_this_game", "That item isn't from this game");
    const why = buyable(card, me, max);
    return why ? { status: "unavailable", reason: why } : { game, item: card, listing: card.listing! };
  }
  let collections = game.collections.map((c) => c.nftType);
  if (p.collection) {
    if (!owns(game, p.collection)) throw new ApiFail("not_this_game", "That collection isn't from this game");
    collections = collections.filter((c) => typeMatches(c, p.collection!) || typeMatches(p.collection!, c));
  }
  if (!collections.length) return { status: "unavailable", reason: "This game has no items on Rare yet" };
  const kind = p.kind ?? null;
  // rare_items kinds: the item's `kind` attribute (Display attributes come from the item's attrs).
  const ofKind =
    kind === null
      ? undefined
      : async (c: ItemCard) => {
          const det = await d.get<ItemDetail>(`/v1/items/${enc(c.id)}`).catch(() => null);
          return !!det?.attributes?.some((t) => t.type.toLowerCase() === "kind" && t.value === String(kind));
        };
  const card = await cheapest(collections, me, max, d, ofKind);
  if (!card) return { status: "unavailable", reason: max !== null ? "Nothing listed at or under maxMist" : "Nothing is listed right now" };
  return { game, item: card, listing: card.listing! };
}

// ---------------------------------------------------------------- claim

export type Rejected = { status: "rejected"; reason: string };

export interface ClaimSheetData {
  game: OverlayGame | null;
  voucher: Voucher;
  title: string;
  /** Read from chain for item and state vouchers: the class's marker type, its policy, the item's class. */
  chain: { marker: string; policyId?: string; classId?: string } | null;
}

/**
 * Reads a voucher and scopes it: it must be for the player's account and not expired, an item or
 * state voucher must be for one of the game's item types (`Item<C>`, read from the Class or Item
 * object on chain), and the package it needs must be published. `gameId` null: a claim link the
 * player opened, not a game.
 */
export async function prepareClaim(gameId: string | null, text: string, d: Deps, now = Date.now()): Promise<ClaimSheetData | Unavailable | Rejected> {
  let v: Voucher;
  try {
    v = decodeVoucher(text);
  } catch (e) {
    if (e instanceof VoucherError) throw new ApiFail("bad_request", e.message);
    throw e;
  }
  const a = await signer(d);
  if (!d.canSign(a)) throw new ApiFail("watch_only", "This Rare Wallet account is watch only");
  const why = rejectReason(v, a.address, now);
  if (why) return { status: "rejected", reason: why };
  const game = gameId ? await gameFor(gameId, d) : null;
  if (v.type === "trophy") {
    const cfg = trophyConfig();
    if (!cfg || cfg.network !== "mainnet") return { status: "unavailable", reason: "Rare Trophies isn't live yet" };
    if (normalizeSuiAddress(cfg.registry) !== v.registry) return { status: "rejected", reason: "The voucher is for a different trophy registry" };
    return { game, voucher: v, title: voucherTitle(v), chain: null };
  }
  const pkg = itemsPackage("mainnet");
  if (!pkg) return { status: "unavailable", reason: "Rare items aren't live yet" };
  let chain: ClaimSheetData["chain"];
  try {
    if (v.type === "item") {
      const c = await readClass(pkg, v.classId, d.object);
      chain = { marker: c.marker, policyId: c.policyId, classId: v.classId };
    } else {
      const it = await readItem(pkg, v.itemId, d.object);
      if (it.rev !== v.rev) return { status: "rejected", reason: "This item changed since the voucher was made. Ask the game for a new one." };
      chain = { marker: it.marker, classId: it.classId };
    }
  } catch (e) {
    if (e instanceof VoucherError) return { status: "rejected", reason: e.message };
    throw e;
  }
  if (game && !owns(game, itemType(pkg, chain.marker))) throw new ApiFail("not_this_game", "That item class isn't from this game");
  return { game, voucher: v, title: voucherTitle(v), chain };
}
