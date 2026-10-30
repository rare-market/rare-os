// Swaps through the Trade API's DEX router, the way wallet clients do it (developer API
// /swap): quote for the trader's wallet, then the unsigned transaction for that quote, which
// Rare Wallet signs and submits. Nothing here signs a trade.
//
// The API wants a key. The launcher gets a free one on its own: a device-local identity key
// (holds nothing, never signs a trade) answers the API's sign-in challenge once, and the key is
// kept on the device. Trades still go out from the person's own wallet (`owner`).

import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { Transaction } from "@mysten/sui/transactions";
import { normalizeType, SLIPPAGE_BPS } from "./amount";
import { gasFromSpentSui } from "./chain";
import { tradeCall, errorOf, TradeApiError, type Reply } from "./http";

export interface SwapQuote {
  id: string;
  coinIn: string;
  coinOut: string;
  amountIn: bigint;
  /** what the wallet receives after every fee, base units */
  amountOut: bigint;
  feeBps: number;
  hops: number;
  /** exchanges on the route */
  venues: string[];
  expiresAt: number;
}

export interface SwapBuild {
  /** the unsigned transaction, sender set; gas is resolved when it is built */
  tx: Transaction;
  minOut: bigint | null;
  gasEstimateMist: bigint | null;
  expiresAt: number;
}

type Call = <T>(method: "GET" | "POST", path: string, body?: unknown, auth?: string) => Promise<Reply<T>>;

export interface KeyStore {
  get(): string | null;
  set(key: string | null): void;
}

const STORE_KEY = "rare.trade.apikey";
export const localKeyStore: KeyStore = {
  get: () => {
    try {
      return localStorage.getItem(STORE_KEY);
    } catch {
      return null;
    }
  },
  set: (k) => {
    try {
      if (k) localStorage.setItem(STORE_KEY, k);
      else localStorage.removeItem(STORE_KEY);
    } catch {
      /* private mode: a new key next time */
    }
  },
};

export interface SwapDeps {
  call: Call;
  keys: KeyStore;
  identity: () => Ed25519Keypair;
  /** gas payment fix-up before the wallet sees it (chain read) */
  gas: (tx: Transaction) => Promise<Transaction>;
}

const defaults: SwapDeps = { call: tradeCall, keys: localKeyStore, identity: () => new Ed25519Keypair(), gas: (tx) => gasFromSpentSui(tx) };

let pendingKey: Promise<string> | null = null;

/** The device's API key, created on first use. */
export async function apiKey(d: SwapDeps = defaults): Promise<string> {
  const have = d.keys.get();
  if (have) return have;
  pendingKey ??= (async () => {
    const id = d.identity();
    const address = id.toSuiAddress();
    const ch = await d.call<{ data?: { challengeId: string; message: string } }>("POST", "/auth/challenge", { address });
    if (ch.status !== 200 || !ch.body?.data) throw errorOf(ch.status, ch.body);
    const { signature } = await id.signPersonalMessage(new TextEncoder().encode(ch.body.data.message));
    const k = await d.call<{ data?: { key: string } }>("POST", "/auth/keys", {
      address,
      challengeId: ch.body.data.challengeId,
      signature,
      name: "Rare Launcher",
      scopes: ["quote", "build"],
    });
    if (k.status !== 200 || !k.body?.data?.key) throw errorOf(k.status, k.body);
    d.keys.set(k.body.data.key);
    return k.body.data.key;
  })().finally(() => {
    pendingKey = null;
  });
  return pendingKey;
}

/** POST with the device key; a key the API no longer knows is replaced once. */
async function keyed<T>(path: string, body: unknown, d: SwapDeps): Promise<T> {
  for (let attempt = 0; attempt < 2; attempt++) {
    const key = await apiKey(d);
    const r = await d.call<{ data?: T }>("POST", path, body, key);
    if (r.status === 401 && attempt === 0) {
      d.keys.set(null);
      continue;
    }
    if (r.status !== 200 || !r.body?.data) throw errorOf(r.status, r.body);
    return r.body.data;
  }
  throw new TradeApiError(401, "authentication_required", "Could not sign in to the swap service.");
}

/** The exchanges in a router answer: its own hops, or the aggregator's providers. */
export function routeVenues(route: unknown): string[] {
  const r = route as { paths?: Array<{ hops?: Array<{ venue?: unknown }> }>; providers?: unknown[] } | null;
  const names = [...(r?.paths ?? []).flatMap((p) => (p.hops ?? []).map((h) => h.venue)), ...(r?.providers ?? [])].filter(
    (v): v is string => typeof v === "string" && !!v,
  );
  return [...new Set(names.map((n) => n.toLowerCase()))];
}

export async function quoteSwap(p: { coinIn: string; coinOut: string; amountIn: bigint; owner?: string | null }, d: SwapDeps = defaults): Promise<SwapQuote> {
  if (p.amountIn <= 0n) throw new TradeApiError(400, "invalid_amount", "Choose an amount first.");
  const coinIn = normalizeType(p.coinIn);
  const coinOut = normalizeType(p.coinOut);
  const q = await keyed<Record<string, unknown>>("/swap/quote", { coinIn, coinOut, amountIn: p.amountIn.toString(), ...(p.owner ? { owner: p.owner } : {}) }, d);
  const out = String(q.amountOut ?? "");
  if (typeof q.id !== "string" || !/^\d+$/.test(out) || BigInt(out) <= 0n) throw new TradeApiError(502, "invalid_quote", "No route for this pair right now.");
  return {
    id: q.id,
    coinIn,
    coinOut,
    amountIn: p.amountIn,
    amountOut: BigInt(out),
    feeBps: Number(q.feeBps ?? 0),
    hops: Number(q.hops ?? 1) || 1,
    venues: routeVenues(q.route),
    expiresAt: Number(q.expiresAt ?? Date.now() + 30_000),
  };
}

/** A transaction kind from the router with the trader as sender, ready for the wallet. */
export function swapTransaction(kind: string, sender: string): Transaction {
  const tx = Transaction.fromKind(kind);
  tx.setSender(sender);
  return tx;
}

/**
 * The unsigned swap for a quote. The router dry-runs it for `owner` before answering.
 * `sponsored`: Rare pays the fee, so the kind is used as it comes (no gas-coin fix-up: a sponsored
 * transaction must never touch the gas coin, which belongs to the sponsor).
 */
export async function buildSwap(q: SwapQuote, owner: string, slippageBps = SLIPPAGE_BPS, d: SwapDeps = defaults, o: { sponsored?: boolean } = {}): Promise<SwapBuild> {
  const b = await keyed<Record<string, unknown>>("/swap/build", { id: q.id, owner, slippageBps, mode: "transaction" }, d);
  if (typeof b.kind !== "string" || !b.kind) throw new TradeApiError(502, "invalid_build", "Could not build this swap.");
  const sim = (b.simulation ?? {}) as { status?: string; gasEstimateMist?: string };
  if (sim.status === "failed") throw new TradeApiError(422, "simulation_failed", "This swap would fail right now. Get a new quote.");
  const gas = b.gasEstimateMist ?? sim.gasEstimateMist;
  return {
    tx: o.sponsored ? swapTransaction(b.kind, owner) : await d.gas(swapTransaction(b.kind, owner)),
    minOut: typeof b.minOut === "string" && /^\d+$/.test(b.minOut) ? BigInt(b.minOut) : null,
    gasEstimateMist: gas != null && /^\d+$/.test(String(gas)) ? BigInt(String(gas)) : null,
    expiresAt: Number(b.expiresAt ?? q.expiresAt),
  };
}

/** Exchange names for the route line. */
const VENUES: Record<string, string> = {
  cetus: "Cetus",
  aftermath: "Aftermath",
  bluefin: "Bluefin",
  turbos: "Turbos",
  deepbook: "DeepBook",
  deepbookv3: "DeepBook",
  flowx: "FlowX",
  flowxv3: "FlowX",
  kriya: "Kriya",
  kriyav3: "Kriya",
  momentum: "Momentum",
  haedal: "Haedal",
  obric: "Obric",
  steamm: "Steamm",
  magma: "Magma",
  bluemove: "BlueMove",
};
export function venueName(v: string): string {
  const k = v.toLowerCase();
  const base = k.replace(/[_-].*$/, "").replace(/v\d+$/, "");
  const amm = /^(.+?)(dlmm|clmm)$/.exec(base);
  if (!VENUES[k] && !VENUES[base] && amm && VENUES[amm[1]]) return `${VENUES[amm[1]]} ${amm[2].toUpperCase()}`;
  return VENUES[k] ?? VENUES[base] ?? base.charAt(0).toUpperCase() + base.slice(1);
}
