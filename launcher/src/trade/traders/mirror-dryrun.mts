// Live check of Mirror against mainnet with nothing signed or sent: read Top traders, take a top
// trader's latest buy of a coin, plan the mirror at 1 SUI, then router quote, unsigned build and a
// gRPC dry run for `sender`. Run from launcher/: npx tsx src/trade/traders/mirror-dryrun.mts [sender]
// The sender only needs SUI for the simulated swap and gas; its keys are never involved.

import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { SUI_TYPE, fromBase, normalizeType } from "../amount";
import { dryRun, gasFromSpentSui } from "../chain";
import type { Reply } from "../http";
import { buildSwap, quoteSwap, type KeyStore, type SwapDeps } from "../swap";
import { traderName, traderPaths, type BoardExtras, type LeaderRow, type TradesPage } from "./api";
import { topTraders } from "./board";
import { mirrorPlan } from "./mirror";

const HOST = "https://rare.market/v1/trade";
const sender = process.argv[2] ?? "0x51af2aac22f6e51624eb626403d57b59aaa1aa97c43ea24107f61f547566893a";

const get = async <T,>(path: string): Promise<T> => {
  const r = await fetch(`${HOST}${path}`);
  if (!r.ok) throw new Error(`${path}: ${r.status}`);
  return (await r.json()) as T;
};
const call = (async (method: string, path: string, body?: unknown, auth?: string): Promise<Reply<unknown>> => {
  const r = await fetch(`${HOST}${path}`, {
    method,
    headers: { "content-type": "application/json", ...(auth ? { authorization: `Bearer ${auth}` } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return { status: r.status, body: await r.json().catch(() => null) };
}) as SwapDeps["call"];
let key: string | null = null;
const keys: KeyStore = { get: () => key, set: (k) => void (key = k) };
const deps: SwapDeps = { call, keys, identity: () => new Ed25519Keypair(), gas: (tx) => gasFromSpentSui(tx) };

const [board, extras] = await Promise.all([get<{ rows: LeaderRow[] }>(traderPaths.board("7d")), get<BoardExtras>(traderPaths.extras("7d")).catch(() => undefined)]);
const top = topTraders(board.rows, extras, { sort: "pnl" });
console.log(`top traders 7d: ${top.length} pass the minimum (of ${board.rows.length}); first ${top[0] ? traderName(top[0].row.user) : "none"}`);

let done = false;
for (const t of top.slice(0, 15)) {
  const page = await get<TradesPage>(traderPaths.trades(t.address, 25));
  const buy = page.items.find((x) => x.buy && normalizeType(x.coin) !== SUI_TYPE);
  if (!buy) continue;
  const meta = await get<{ decimals: number; symbol: string }>(`/ext/token/${encodeURIComponent(normalizeType(buy.coin))}`);
  const plan = mirrorPlan(buy, { buySui: 1, sellPct: 100 }, meta.decimals, null);
  if (!plan.ok) continue;
  console.log(`mirror ${traderName(t.row.user)}: bought $${buy.usd.toFixed(2)} of ${meta.symbol}; plan ${fromBase(plan.amountIn, 9)} SUI -> ${meta.symbol}`);
  try {
    const q = await quoteSwap({ coinIn: plan.coinIn, coinOut: plan.coinOut, amountIn: plan.amountIn, owner: sender }, deps);
    console.log(`quote       ${fromBase(q.amountOut, meta.decimals)} ${meta.symbol}, ${q.hops} hops via ${q.venues.join(", ")}, fee ${q.feeBps} bps`);
    const b = await buildSwap(q, sender, 100, deps);
    const d = await dryRun(b.tx, sender);
    const got = d.changes.find((c) => normalizeType(c.coinType) === normalizeType(plan.coinOut));
    console.log(`dry run     ${d.ok ? "ok" : `FAILED: ${d.error}`}, receives ${got ? fromBase(got.amount, meta.decimals) : "?"} ${meta.symbol}, gas ${d.gasMist != null ? fromBase(d.gasMist, 9) : "?"} SUI`);
    if (d.ok && got && got.amount > 0n) {
      done = true;
      break;
    }
  } catch (e) {
    console.log(`skip ${meta.symbol}: ${e instanceof Error ? e.message : String(e)}`);
  }
}
console.log(done ? "mirror dry run ok (nothing signed or sent)" : "mirror dry run FAILED");
process.exit(done ? 0 : 1);
