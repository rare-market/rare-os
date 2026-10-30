// Gas sponsorship from rare.market ("Rare covers the fee"). For a few actions (Playtron rescue
// moves, trophy claims, a first swap, Walrus uploads) the rare.market sponsor wallet pays the
// network fee when the wallet has too little SUI for it:
//
//   1. GET  /v1/sponsor/status?address=   what's on offer for this address (cached ~30 s)
//   2. POST /v1/sponsor                    send the transaction kind; get back the full transaction
//                                          (gas owner = sponsor) and the sponsor's signature
//   3. the person signs those exact bytes in Rare Wallet's review
//   4. POST /v1/sponsor/execute            the server adds the sponsor signature and submits
//
// Game budgets (category "game:<slug>", rare.market api/src/sponsor/game.ts): a studio funds a
// budget and its players' fees come out of it, for what its policy covers (the game's own
// packages, rare_items claims of its classes, its trophies). The launcher asks for every
// transaction a web game has Rare Wallet sign and execute, and for Local API sell, buy and claim
// from a game (`sponsorForGame`); any "no" (503, 429, 403, budget_exhausted) means the player
// pays as before, and the review sheet says who pays.
//
// Every expected "no" (not funded, disabled, busy, refused by policy, rate limited, network down)
// comes back as a typed result, never a throw, so callers fall back to the wallet paying its own
// fee. A kind that touches the gas coin is never sent: the sponsor pays with its own coin.
//
// The server lives in the rare.market API; the launcher reaches it through the Rust rare_get /
// rare_post proxy (the API sends no CORS headers).

import { bcs } from "@mysten/sui/bcs";
import { Transaction } from "@mysten/sui/transactions";
import { fromBase64, normalizeSuiAddress, toBase64 } from "@mysten/sui/utils";
import { grpcClient } from "./grpc";

/** Rare's own categories (Rare's budget and limits). */
export type SponsorCategory = "rescue" | "trophy" | "trade" | "walrus";
/** A game's budget pays: "game:<slug>". */
export type GameCategory = `game:${string}`;
/** Any category a sponsorship can be asked for. */
export type AnyCategory = SponsorCategory | GameCategory;

const GAME_SLUG = /^[a-z0-9][a-z0-9-]{0,63}$/;
/** "game:<slug>" for a catalog slug the API accepts, else null. */
export function gameCategory(slug: string | null | undefined): GameCategory | null {
  return slug && GAME_SLUG.test(slug) ? `game:${slug}` : null;
}

/** A game's gas budget in /v1/sponsor/status?game= (the API's SponsorStatus.game). */
export interface GameGasStatus {
  slug: string;
  /** The game's budget pays its players' fees right now. */
  covered: boolean;
  availableMist: string;
  paused: boolean;
  playerDailyTx?: number;
  playerDailyMist?: string;
  you?: { todayTx: number; todayMist: string; eligible: boolean };
}

export interface SponsorStatus {
  enabled: boolean;
  funded: boolean;
  /** the sponsor wallet (gas owner of sponsored transactions) */
  address: string;
  balanceMist: string;
  dailyBudgetMist: string;
  spentTodayMist: string;
  remainingTodayMist: string;
  categories: Record<SponsorCategory, boolean>;
  maxUserSuiMist: string;
  you?: { sponsored: number; gasMist: string; tradeUsed: boolean; eligible: Record<SponsorCategory, boolean> };
  /** With ?game=: that game's budget. */
  game?: GameGasStatus;
}

export interface Sponsorship {
  ok: true;
  /** base64 TransactionData, gas owner = sponsor: the person signs exactly these */
  bytes: string;
  sponsorSignature: string;
  digest: string;
  sponsor: string;
  category: string;
  gasBudget: bigint;
  estimatedGasMist: bigint;
  /** ms epoch; ask again after this */
  expiresAt: number;
}

export type SponsorFailReason =
  /** not funded, disabled, or the status said no */
  | "unavailable"
  | "busy"
  /** policy said no; message says why */
  | "refused"
  /** the game's gas budget is used up (budget_exhausted) */
  | "exhausted"
  /** the game's budget doesn't cover this player now (status said no; nothing was sent) */
  | "not-covered"
  | "limited"
  | "simulation"
  /** the kind uses the gas coin: never sent */
  | "gas-coin"
  /** the kind could not be built here */
  | "build"
  /** the server's answer did not match what was asked */
  | "mismatch"
  | "network"
  | "expired"
  | "already-executed"
  | "failed";

export interface SponsorFail {
  ok: false;
  reason: SponsorFailReason;
  code: string;
  message: string;
}

export type SponsorResult = Sponsorship | SponsorFail;

export interface Executed {
  ok: true;
  digest: string;
  gasUsedMist: bigint;
}
export type ExecuteResult = Executed | SponsorFail;

type Reply<T> = { status: number; body: T };

export interface SponsorDeps {
  /** GET rare.market{path}; throws on network errors and non-2xx */
  get: <T>(path: string) => Promise<T>;
  /** POST JSON to rare.market{path}; any status comes back, network failures throw */
  post: <T>(path: string, body: unknown) => Promise<Reply<T>>;
  /** builds transaction kinds (object versions, pure args) */
  client: unknown;
  now: () => number;
}

export const defaultSponsorDeps: SponsorDeps = {
  get: async (p) => (await import("../lib/bridge")).native.rareGet(p),
  post: async (p, b) => (await import("../lib/bridge")).native.rarePost(p, b),
  client: grpcClient,
  now: () => Date.now(),
};

/** 0.01 SUI: the fee assumed when there's no estimate. */
export const DEFAULT_FEE_MIST = 10_000_000n;
/** What a wallet needs on top of the estimate before it's "enough SUI for the fee". */
const MARGIN_MIST = 5_000_000n;

/** True when the wallet's SUI won't cover the fee (estimate plus a margin; 0.01 SUI without one). */
export function needsSponsor(suiBalanceMist: bigint, estimatedGasMist?: bigint | null): boolean {
  const est = estimatedGasMist != null && estimatedGasMist > 0n ? estimatedGasMist : DEFAULT_FEE_MIST;
  const margin = est / 2n > MARGIN_MIST ? est / 2n : MARGIN_MIST;
  return suiBalanceMist < est + margin;
}

// ---------------------------------------------------------------- status

const STATUS_MS = 30_000;
const statusCache = new Map<string, { at: number; value: Promise<SponsorStatus | null> }>();
const sponsorAddresses = new Set<string>();

/** Sponsor status for an address, cached ~30 s. Null when the API can't be reached. */
export function sponsorStatus(address: string, d: SponsorDeps = defaultSponsorDeps): Promise<SponsorStatus | null> {
  const key = normalizeSuiAddress(address);
  const hit = statusCache.get(key);
  if (hit && d.now() - hit.at < STATUS_MS) return hit.value;
  const value = d
    .get<{ data?: SponsorStatus }>(`/v1/sponsor/status?address=${key}`)
    .then((r) => {
      const s = r?.data;
      if (!s || typeof s.address !== "string") return null;
      sponsorAddresses.add(normalizeSuiAddress(s.address));
      return s;
    })
    .catch(() => null);
  statusCache.set(key, { at: d.now(), value });
  return value;
}

/** Forget cached status (after a sponsored transaction, so "first trade used" shows). */
export function clearSponsorStatus(address?: string) {
  if (address) statusCache.delete(normalizeSuiAddress(address));
  else statusCache.clear();
}

/** Sponsor wallets seen in status answers: a gas owner among these is "covered by Rare". */
export function knownSponsors(): string[] {
  return [...sponsorAddresses];
}

/** Whether the status offers this category to this address right now. */
export function offers(s: SponsorStatus | null, category: SponsorCategory): boolean {
  if (!s || !s.enabled || !s.funded) return false;
  if (!s.categories?.[category]) return false;
  if (s.you?.eligible && !s.you.eligible[category]) return false;
  if (category === "trade" && s.you?.tradeUsed) return false;
  try {
    if (BigInt(s.remainingTodayMist) <= 0n) return false;
  } catch {
    /* missing budget figure: let the server decide */
  }
  return true;
}

/**
 * Should Rare offer to cover the fee? Only when the wallet is short of SUI for it and the sponsor
 * offers this category to this address. Never throws.
 */
export async function sponsorOffer(
  address: string,
  category: SponsorCategory,
  o: { suiBalanceMist: bigint | (() => Promise<bigint>); estimatedGasMist?: bigint | null },
  d: SponsorDeps = defaultSponsorDeps,
): Promise<SponsorStatus | null> {
  try {
    const bal = typeof o.suiBalanceMist === "function" ? await o.suiBalanceMist() : o.suiBalanceMist;
    if (!needsSponsor(bal, o.estimatedGasMist)) return null;
    const s = await sponsorStatus(address, d);
    if (!offers(s, category)) return null;
    // the server sponsors only wallets that hold less than this
    try {
      if (s!.maxUserSuiMist && bal > BigInt(s!.maxUserSuiMist)) return null;
    } catch {
      /* unparsable: let the server decide */
    }
    return s;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------- the gas coin check

/** Any use of the gas coin: a GasCoin argument, or a coin intent that falls back to it. */
export function usesGasCoin(node: unknown): boolean {
  if (Array.isArray(node)) return node.some(usesGasCoin);
  if (!node || typeof node !== "object") return false;
  const o = node as Record<string, unknown>;
  if (o.$kind === "GasCoin" || o.GasCoin === true) return true;
  const intent = o.$Intent as { name?: string; data?: { type?: unknown } } | undefined;
  if (intent && intent.data?.type === "gas") return true;
  return Object.values(o).some(usesGasCoin);
}

// ---------------------------------------------------------------- sponsor + execute

const fail = (reason: SponsorFailReason, code: string, message: string): SponsorFail => ({ ok: false, reason, code, message });

type ErrBody = { error?: { code?: string; message?: string } | string } | null;
function errorParts(body: unknown): { code: string; message: string } {
  const b = body as ErrBody;
  if (b && typeof b.error === "object" && b.error) return { code: b.error.code ?? "error", message: b.error.message ?? "" };
  if (b && typeof b.error === "string") return { code: b.error, message: b.error };
  return { code: "error", message: "" };
}

function reasonFor(status: number, code: string): SponsorFailReason {
  if (code === "budget_exhausted") return "exhausted";
  if (code === "sponsor_busy") return "busy";
  if (code === "sponsor_unavailable" || status === 503) return "unavailable";
  if (code === "not_sponsored" || status === 403) return "refused";
  if (status === 429) return "limited";
  if (code === "simulation_failed" || status === 422) return "simulation";
  if (code === "sponsorship_expired" || status === 410) return "expired";
  if (code === "already_executed" || status === 409) return "already-executed";
  return "failed";
}

const PLAIN: Partial<Record<SponsorFailReason, string>> = {
  unavailable: "Rare can't cover fees right now.",
  busy: "Rare's fee cover is busy. Try again in a moment.",
  limited: "Rare has covered as many fees as it can for now.",
  exhausted: "This game's gas budget is used up.",
  simulation: "This would fail right now.",
  expired: "The fee cover ran out of time. Try again.",
  network: "Couldn't reach rare.market.",
};

/** Kind bytes (base64) of a transaction, refusing any that touch the gas coin. */
export async function sponsorKind(tx: Transaction, sender: string, d: SponsorDeps = defaultSponsorDeps): Promise<{ kind: string } | SponsorFail> {
  tx.setSenderIfNotSet(sender);
  if (usesGasCoin(tx.getData().commands)) return fail("gas-coin", "gas_coin", "This transaction pays from the gas coin, so Rare can't cover its fee.");
  let kind: Uint8Array;
  try {
    kind = await tx.build({ client: d.client as never, onlyTransactionKind: true });
  } catch (e) {
    return fail("build", "build_failed", e instanceof Error ? e.message : String(e));
  }
  // intents resolve during the build: check what will actually be sent too
  if (usesGasCoin(Transaction.fromKind(kind).getData().commands)) return fail("gas-coin", "gas_coin", "This transaction pays from the gas coin, so Rare can't cover its fee.");
  return { kind: toBase64(kind) };
}

/** The sponsored bytes match the kind, the sender and the sponsor the server named. */
function checkSponsored(bytes: string, kind: string, sender: string, sponsor: string): string | null {
  try {
    const data = bcs.TransactionData.parse(fromBase64(bytes));
    const v1 = data.V1;
    if (!v1) return "unknown transaction version";
    if (normalizeSuiAddress(v1.sender) !== normalizeSuiAddress(sender)) return "sender changed";
    if (normalizeSuiAddress(v1.gasData.owner) !== normalizeSuiAddress(sponsor)) return "gas owner is not the sponsor";
    if (normalizeSuiAddress(v1.gasData.owner) === normalizeSuiAddress(sender)) return "gas owner is the sender";
    if (toBase64(bcs.TransactionKind.serialize(v1.kind).toBytes()) !== kind) return "transaction changed";
    return null;
  } catch (e) {
    return e instanceof Error ? e.message : "unreadable transaction";
  }
}

/**
 * Ask rare.market to pay the fee for `tx` (sender = `sender`). Resolves with the full sponsored
 * transaction to sign, or a typed failure for the caller to fall back on. Never throws.
 */
export async function sponsorTransaction(tx: Transaction, sender: string, category: AnyCategory, d: SponsorDeps = defaultSponsorDeps): Promise<SponsorResult> {
  const k = await sponsorKind(tx, sender, d).catch((e: unknown) => fail("build", "build_failed", e instanceof Error ? e.message : String(e)));
  if ("ok" in k) return k;
  return sponsorKindBytes(k.kind, sender, category, d);
}

/** As sponsorTransaction, for kind bytes already built (and checked with usesGasCoin). */
export async function sponsorKindBytes(kind: string, sender: string, category: AnyCategory, d: SponsorDeps = defaultSponsorDeps): Promise<SponsorResult> {
  let r: Reply<{ data?: Record<string, unknown> } | null>;
  try {
    r = await d.post("/v1/sponsor", { sender: normalizeSuiAddress(sender), kind, category });
  } catch (e) {
    return fail("network", "network", PLAIN.network!);
  }
  if (r.status !== 200 || !r.body?.data) {
    const { code, message } = errorParts(r.body);
    const reason = reasonFor(r.status, code);
    return fail(reason, code, reason === "refused" && message ? message : PLAIN[reason] ?? (message || `Sponsor error (${r.status})`));
  }
  const s = r.body.data as Record<string, string | number>;
  const bytes = String(s.bytes ?? "");
  const sponsor = String(s.sponsor ?? "");
  const bad = !bytes || !sponsor || !s.sponsorSignature || !s.digest ? "incomplete answer" : checkSponsored(bytes, kind, sender, sponsor);
  if (bad) return fail("mismatch", "mismatch", `The sponsored transaction didn't match: ${bad}.`);
  sponsorAddresses.add(normalizeSuiAddress(sponsor));
  const big = (v: unknown) => (v != null && /^\d+$/.test(String(v)) ? BigInt(String(v)) : 0n);
  return {
    ok: true,
    bytes,
    sponsorSignature: String(s.sponsorSignature),
    digest: String(s.digest),
    sponsor,
    category: String(s.category ?? category),
    gasBudget: big(s.gasBudget),
    estimatedGasMist: big(s.estimatedGasMist),
    expiresAt: Number(s.expiresAt) || d.now() + 110_000,
  };
}

/** Hand the person's signature over the sponsored bytes to rare.market, which submits. Never throws. */
export async function executeSponsored(digest: string, userSignature: string, d: SponsorDeps = defaultSponsorDeps): Promise<ExecuteResult> {
  let r: Reply<{ data?: { digest?: string; ok?: boolean; error?: string; gasUsedMist?: string } } | null>;
  try {
    r = await d.post("/v1/sponsor/execute", { digest, signature: userSignature });
  } catch {
    return fail("network", "network", PLAIN.network!);
  }
  if (r.status !== 200 || !r.body?.data) {
    const { code, message } = errorParts(r.body);
    const reason = reasonFor(r.status, code);
    return fail(reason, code, PLAIN[reason] ?? (message || `Sponsor error (${r.status})`));
  }
  const x = r.body.data;
  if (!x.ok) return fail("failed", "execution_failed", x.error || "The network rejected this transaction.");
  return { ok: true, digest: x.digest ?? digest, gasUsedMist: /^\d+$/.test(String(x.gasUsedMist ?? "")) ? BigInt(String(x.gasUsedMist)) : 0n };
}

/** Seconds to spare before a sponsorship's expiry; less than this and a fresh one is asked for. */
const EXPIRY_SLACK_MS = 5_000;

export interface SponsoredRun {
  /** the transaction whose kind is sponsored (asked again if the first sponsorship expires) */
  tx: Transaction;
  sender: string;
  category: AnyCategory;
  /** signs the exact sponsored bytes; returns the base64 signature */
  sign: (bytes: Uint8Array) => Promise<string>;
  /** a sponsorship already fetched (the one the review showed) */
  first?: Sponsorship;
}

export type SponsoredOutcome = (Executed & { bytes: string; signature: string; sponsor: string }) | SponsorFail;

/**
 * Sponsor, sign, execute. An expired sponsorship (before signing or at execute) is asked for again
 * once. Signer errors (rejected, device gone) are thrown as they are.
 */
export async function runSponsored(o: SponsoredRun, d: SponsorDeps = defaultSponsorDeps): Promise<SponsoredOutcome> {
  let s: SponsorResult | undefined = o.first;
  for (let attempt = 0; attempt < 2; attempt++) {
    if (!s || (s.ok && s.expiresAt - d.now() < EXPIRY_SLACK_MS)) s = await sponsorTransaction(o.tx, o.sender, o.category, d);
    if (!s.ok) return s;
    const signature = await o.sign(fromBase64(s.bytes));
    const e = await executeSponsored(s.digest, signature, d);
    if (e.ok) {
      clearSponsorStatus(o.sender);
      return { ...e, bytes: s.bytes, signature, sponsor: s.sponsor };
    }
    if (e.reason !== "expired" || attempt > 0) return e;
    s = undefined;
  }
  return fail("expired", "sponsorship_expired", PLAIN.expired!);
}

// ---------------------------------------------------------------- game budgets

const gameStatusCache = new Map<string, { at: number; value: Promise<SponsorStatus | null> }>();

/** Sponsor status with a game's budget for an address, cached ~30 s. Null when the API can't be reached. */
export function gameGasStatus(slug: string, address: string, d: SponsorDeps = defaultSponsorDeps): Promise<SponsorStatus | null> {
  const key = `${slug}|${normalizeSuiAddress(address)}`;
  const hit = gameStatusCache.get(key);
  if (hit && d.now() - hit.at < STATUS_MS) return hit.value;
  const value = d
    .get<{ data?: SponsorStatus }>(`/v1/sponsor/status?address=${normalizeSuiAddress(address)}&game=${encodeURIComponent(slug)}`)
    .then((r) => {
      const s = r?.data;
      if (!s || typeof s.address !== "string") return null;
      sponsorAddresses.add(normalizeSuiAddress(s.address));
      return s;
    })
    .catch(() => null);
  gameStatusCache.set(key, { at: d.now(), value });
  return value;
}

export function clearGameGasStatus() {
  gameStatusCache.clear();
}

/** Why a game's budget won't pay, from its status; null when it would (the server still decides). */
export function gameNotCovered(s: SponsorStatus | null, slug: string): SponsorFail | null {
  if (!s) return fail("network", "network", PLAIN.network!);
  if (!s.enabled || !s.funded) return fail("unavailable", "sponsor_unavailable", PLAIN.unavailable!);
  const g = s.game;
  if (!g || g.slug !== slug) return fail("not-covered", "not_covered", "This game doesn't cover fees.");
  try {
    if (BigInt(g.availableMist) <= 0n) return fail("exhausted", "budget_exhausted", PLAIN.exhausted!);
  } catch {
    /* no figure: let the server decide */
  }
  if (g.paused || !g.covered) return fail("not-covered", "not_covered", "This game doesn't cover fees right now.");
  if (g.you && !g.you.eligible) return fail("limited", "player_limit", "This game has covered as many of your fees as it can today.");
  return null;
}

/** Kind bytes (base64) of full TransactionData bytes a game built, refusing any that touch the gas coin. */
export function kindOfBytes(bytes: Uint8Array): { kind: string } | SponsorFail {
  try {
    const data = bcs.TransactionData.parse(bytes);
    if (!data.V1) return fail("build", "build_failed", "unknown transaction version");
    const kind = bcs.TransactionKind.serialize(data.V1.kind).toBytes();
    if (usesGasCoin(Transaction.fromKind(kind).getData().commands)) return fail("gas-coin", "gas_coin", "This transaction pays from the gas coin, so its fee can't be covered.");
    return { kind: toBase64(kind) };
  } catch (e) {
    return fail("build", "build_failed", e instanceof Error ? e.message : String(e));
  }
}

/**
 * Ask the game's budget to pay for `tx` (a Transaction, or the full bytes a web game built). Checks the
 * game's status first (cached) and asks only when it would cover this player. Never throws.
 * Resolves with the sponsorship and the Transaction it came from (kept to ask again on expiry).
 */
export async function sponsorForGame(
  tx: Transaction | Uint8Array,
  sender: string,
  slug: string,
  d: SponsorDeps = defaultSponsorDeps,
): Promise<(Sponsorship & { source: Transaction; category: GameCategory }) | SponsorFail> {
  const category = gameCategory(slug);
  if (!category) return fail("not-covered", "not_covered", "This game doesn't cover fees.");
  const no = gameNotCovered(await gameGasStatus(slug, sender, d), slug);
  if (no) return no;
  let source: Transaction;
  let r: SponsorResult;
  if (tx instanceof Uint8Array) {
    const k = kindOfBytes(tx);
    if ("ok" in k) return k;
    source = Transaction.fromKind(k.kind);
    source.setSender(normalizeSuiAddress(sender));
    r = await sponsorKindBytes(k.kind, sender, category, d);
  } else {
    source = tx;
    r = await sponsorTransaction(tx, sender, category, d);
  }
  if (!r.ok) {
    // the budget may have changed: look again next time
    if (r.reason === "exhausted" || r.reason === "limited") clearGameGasStatus();
    return r;
  }
  return { ...r, source, category };
}

/** What the review sheet says when a game's budget didn't pay and the player does. Null: say nothing. */
export function gameFeeNote(f: SponsorFail, game: string): string | null {
  if (f.reason === "exhausted") return `${game}'s gas budget is used up, so you pay this fee.`;
  if (f.reason === "limited") return `${game} has covered as many of your fees as it can today, so you pay this one.`;
  return null;
}
