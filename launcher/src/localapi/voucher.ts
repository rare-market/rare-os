// Studio vouchers for `claim`.
//
// The envelope is @rare/sdk's (rare-sdk packages/sdk/src/server/voucher.ts): JSON with only the
// signed fields, sent as base64url by the game server; @rare/sdk passes it to `claim` as padded
// base64, and this reads either.
//
//   { "v": 1, "type": "item",   "class": "0x..", "player": "0x..", "kind": "3", "keys": ["power"], "vals": ["12"], "nonce": "..", "expiresMs": "..", "sig": "<base64>" }
//   { "v": 1, "type": "state",  "item": "0x..", "rev": "4", "keys": ["power"], "vals": ["13"], "nonce": "..", "expiresMs": "..", "sig": "<base64>" }
//   { "v": 1, "type": "trophy", "registry": "0x..", "player": "0x..", "game": "0x..", "trophyId": "2", "nonce": "..", "expiresMs": "..", "sig": "<base64>" }
//
// u64s are decimal strings, `sig` a 64-byte ed25519 signature over the spec's message bytes. The
// launcher never checks the signature: the Move package does, on chain. What the transaction also
// needs and the voucher doesn't carry (the class's marker type C and its TransferPolicy, the item's
// class) is read from the Class or Item object on chain, never taken from the voucher.

import { Transaction } from "@mysten/sui/transactions";
import { fromBase64, normalizeSuiAddress } from "@mysten/sui/utils";
import items from "./items.json";

export type Voucher =
  | { type: "item"; classId: string; player: string; kind: bigint; keys: string[]; vals: string[]; nonce: bigint; expiresMs: bigint; sig: Uint8Array }
  | { type: "state"; itemId: string; rev: bigint; keys: string[]; vals: string[]; nonce: bigint; expiresMs: bigint; sig: Uint8Array }
  | { type: "trophy"; registry: string; player: string; game: string; trophyId: bigint; nonce: bigint; expiresMs: bigint; sig: Uint8Array };

export class VoucherError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "VoucherError";
  }
}

const MAX_ATTRS = 32;
const MAX_ATTR_LEN = 256;
const U64_MAX = (1n << 64n) - 1n;

function u64(v: unknown, what: string): bigint {
  let b: bigint | null = null;
  if (typeof v === "string" && /^\d{1,20}$/.test(v)) b = BigInt(v);
  else if (typeof v === "number" && Number.isSafeInteger(v) && v >= 0) b = BigInt(v);
  if (b === null || b > U64_MAX) throw new VoucherError(`The voucher has no valid ${what}`);
  return b;
}

function addr(v: unknown, what: string): string {
  if (typeof v !== "string" || !/^0x[0-9a-fA-F]{1,64}$/.test(v)) throw new VoucherError(`The voucher has no valid ${what}`);
  return normalizeSuiAddress(v);
}

function pairs(keys: unknown, vals: unknown): { keys: string[]; vals: string[] } {
  const ok = (v: unknown): v is string[] => Array.isArray(v) && v.length <= MAX_ATTRS && v.every((x) => typeof x === "string" && x.length <= MAX_ATTR_LEN);
  if (!ok(keys) || !ok(vals)) throw new VoucherError("The voucher has no valid attributes");
  if (keys.length !== vals.length) throw new VoucherError("The voucher's attributes don't line up");
  if (new Set(keys).size !== keys.length) throw new VoucherError("The voucher names an attribute twice");
  return { keys, vals };
}

/** base64url or base64 text to bytes. */
export function b64any(text: string): Uint8Array {
  const s = text.trim().replace(/-/g, "+").replace(/_/g, "/");
  return fromBase64(s + "=".repeat((4 - (s.length % 4)) % 4));
}

function sig(v: unknown): Uint8Array {
  let b: Uint8Array;
  try {
    b = b64any(String(v ?? ""));
  } catch {
    throw new VoucherError("The voucher signature is not readable");
  }
  if (b.length !== 64) throw new VoucherError("The voucher signature is not readable");
  return b;
}

/** Reads a voucher (shape only). Throws VoucherError with a sentence the sheet can show. */
export function decodeVoucher(text: string): Voucher {
  let j: Record<string, unknown>;
  try {
    j = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(b64any(text)));
  } catch {
    throw new VoucherError("This voucher is not readable");
  }
  if (!j || typeof j !== "object" || Array.isArray(j)) throw new VoucherError("This voucher is not readable");
  if (j.v !== 1) throw new VoucherError("This voucher is from a newer version of Rare");
  const nonce = u64(j.nonce, "nonce");
  const expiresMs = u64(j.expiresMs, "expiry");
  switch (j.type) {
    case "item":
      return { type: "item", classId: addr(j.class, "item class"), player: addr(j.player, "player"), kind: u64(j.kind, "item kind"), ...pairs(j.keys, j.vals), nonce, expiresMs, sig: sig(j.sig) };
    case "state": {
      const p = pairs(j.keys, j.vals);
      if (!p.keys.length) throw new VoucherError("The voucher changes nothing");
      return { type: "state", itemId: addr(j.item, "item"), rev: u64(j.rev, "item revision"), ...p, nonce, expiresMs, sig: sig(j.sig) };
    }
    case "trophy":
      return { type: "trophy", registry: addr(j.registry, "trophy registry"), player: addr(j.player, "player"), game: addr(j.game, "trophy game"), trophyId: u64(j.trophyId, "trophy"), nonce, expiresMs, sig: sig(j.sig) };
    default:
      throw new VoucherError("This voucher is for something Rare can't claim yet");
  }
}

/** Why a well-formed voucher can't be used by this player now, or null. */
export function rejectReason(v: Voucher, player: string, now = Date.now()): string | null {
  if (v.expiresMs <= BigInt(now)) return "This voucher has expired. Ask the game for a new one.";
  if (v.type !== "state" && v.player !== normalizeSuiAddress(player)) return "This voucher is for another account";
  return null;
}

// ---------------------------------------------------------------- rare_items

type ItemsDeployment = Record<"mainnet" | "testnet", { package: string }>;

/** The rare_items package, or null while it is unpublished (items.json, filled by the deploy kit). */
export function itemsPackage(network: "mainnet" | "testnet" = "mainnet", d: ItemsDeployment = items as ItemsDeployment): string | null {
  const p = d[network]?.package;
  return p ? normalizeSuiAddress(p) : null;
}

/** `Item<C>` for a class marker type. */
export const itemType = (pkg: string, markerType: string) => `${normalizeSuiAddress(pkg)}::item::Item<${markerType}>`;

/** `0xpkg::module::Name<Inner>` -> [canonical "0xpkg::module::Name", "Inner"]. */
export function splitGeneric(type: string): [string, string] | null {
  const i = type.indexOf("<");
  if (i < 0 || !type.endsWith(">")) return null;
  const [a, m, n] = type.slice(0, i).split("::");
  if (!a || !m || !n) return null;
  return [`${normalizeSuiAddress(a)}::${m}::${n}`, type.slice(i + 1, -1)];
}

/** The chain reads a claim needs (an object's type and fields), swapped for fakes in tests. */
export type ObjectReader = (id: string) => Promise<{ type: string; json: Record<string, unknown> | null } | null>;

const idOf = (v: unknown): string | null => {
  if (typeof v === "string" && /^0x[0-9a-fA-F]+$/.test(v)) return normalizeSuiAddress(v);
  if (v && typeof v === "object") return idOf((v as Record<string, unknown>).id ?? (v as Record<string, unknown>).bytes);
  return null;
};

/** What an item voucher's class says on chain: its marker type and its TransferPolicy. */
export async function readClass(pkg: string, classId: string, read: ObjectReader): Promise<{ marker: string; policyId: string }> {
  const o = await read(classId);
  const g = o && splitGeneric(o.type);
  if (!o || !g || g[0] !== `${normalizeSuiAddress(pkg)}::class::Class`) throw new VoucherError("The voucher's item class isn't a Rare items class");
  const cap = o.json?.policy_cap as Record<string, unknown> | undefined;
  const policyId = idOf(cap?.policy_id);
  if (!policyId) throw new VoucherError("The item class has no transfer policy");
  return { marker: g[1], policyId };
}

/** What a state voucher's item says on chain: its marker type, class and current revision. */
export async function readItem(pkg: string, itemId: string, read: ObjectReader): Promise<{ marker: string; classId: string; rev: bigint }> {
  const o = await read(itemId);
  const g = o && splitGeneric(o.type);
  if (!o || !g || g[0] !== `${normalizeSuiAddress(pkg)}::item::Item`) throw new VoucherError("The voucher's item isn't a Rare item");
  const classId = idOf(o.json?.class);
  if (!classId) throw new VoucherError("The item has no class");
  return { marker: g[1], classId, rev: u64(o.json?.rev ?? "0", "item revision") };
}

/** Where the player's kiosk is, for a claim into it (a plain kiosk; a personal kiosk's cap is wrapped). */
export interface PlayerKiosk {
  kioskId: string;
  ownerCapId: string;
}

/**
 * The item claim: `mint::claim<C>` into the player's kiosk, or `mint::claim_fresh<C>` (a new kiosk,
 * cap to the player) when they have no plain kiosk.
 */
export function buildItemClaim(pkg: string, v: Extract<Voucher, { type: "item" }>, c: { marker: string; policyId: string }, player: string, kiosk: PlayerKiosk | null): Transaction {
  const tx = new Transaction();
  tx.setSender(normalizeSuiAddress(player));
  const p = normalizeSuiAddress(pkg);
  const tail = [
    tx.pure.u64(v.kind),
    tx.pure.vector("string", v.keys),
    tx.pure.vector("string", v.vals),
    tx.pure.u64(v.nonce),
    tx.pure.u64(v.expiresMs),
    tx.pure.vector("u8", Array.from(v.sig)),
    tx.object.clock(),
  ];
  if (kiosk) {
    tx.moveCall({ target: `${p}::mint::claim`, typeArguments: [c.marker], arguments: [tx.object(v.classId), tx.object(c.policyId), tx.object(kiosk.kioskId), tx.object(kiosk.ownerCapId), ...tail] });
  } else {
    tx.moveCall({ target: `${p}::mint::claim_fresh`, typeArguments: [c.marker], arguments: [tx.object(v.classId), tx.object(c.policyId), ...tail] });
  }
  return tx;
}

/**
 * The state update: `state::apply<C>` on the item, borrowed out of the player's kiosk and put back
 * (kiosk::borrow_val / return_val), or held in the wallet.
 */
export function buildStateClaim(pkg: string, v: Extract<Voucher, { type: "state" }>, it: { marker: string; classId: string }, player: string, kiosk: PlayerKiosk | null): Transaction {
  const tx = new Transaction();
  tx.setSender(normalizeSuiAddress(player));
  const p = normalizeSuiAddress(pkg);
  const type = itemType(p, it.marker);
  const rest = () => [tx.pure.vector("string", v.keys), tx.pure.vector("string", v.vals), tx.pure.u64(v.nonce), tx.pure.u64(v.expiresMs), tx.pure.vector("u8", Array.from(v.sig)), tx.object.clock()];
  if (kiosk) {
    const [item, borrow] = tx.moveCall({ target: "0x2::kiosk::borrow_val", typeArguments: [type], arguments: [tx.object(kiosk.kioskId), tx.object(kiosk.ownerCapId), tx.pure.id(v.itemId)] });
    tx.moveCall({ target: `${p}::state::apply`, typeArguments: [it.marker], arguments: [tx.object(it.classId), item, ...rest()] });
    tx.moveCall({ target: "0x2::kiosk::return_val", typeArguments: [type], arguments: [tx.object(kiosk.kioskId), item, borrow] });
  } else {
    tx.moveCall({ target: `${p}::state::apply`, typeArguments: [it.marker], arguments: [tx.object(it.classId), tx.object(v.itemId), ...rest()] });
  }
  return tx;
}

/** A line for the claim sheet. */
export function voucherTitle(v: Voucher): string {
  switch (v.type) {
    case "item":
      return "Claim an item";
    case "state":
      return "Update your item";
    case "trophy":
      return "Claim a trophy";
  }
}
