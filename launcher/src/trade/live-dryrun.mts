// Live check of the Trade tab's transaction building against mainnet, with nothing signed or sent:
// a router swap (quote, unsigned build, gRPC dry run) and a perps account transaction (unsigned
// build, gRPC dry run). Run from launcher/: npx tsx src/trade/live-dryrun.mts [sender]
// The sender only needs SUI for the simulated gas; its keys are never involved.

import { Transaction } from "@mysten/sui/transactions";
import { SUI_TYPE, USDC_TYPE, fromBase } from "./amount";
import { dryRun, gasFromSpentSui } from "./chain";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import type { Reply } from "./http";
import { buildSwap, quoteSwap, type KeyStore, type SwapDeps } from "./swap";

const HOST = "https://rare.market/v1/trade";
const sender = process.argv[2] ?? "0x51af2aac22f6e51624eb626403d57b59aaa1aa97c43ea24107f61f547566893a";

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

let failed = false;

// 1. Swap 0.5 SUI to USDC through the router.
{
  const q = await quoteSwap({ coinIn: SUI_TYPE, coinOut: USDC_TYPE, amountIn: 500_000_000n, owner: sender }, deps);
  console.log(`swap quote  0.5 SUI -> ${fromBase(q.amountOut, 6)} USDC, ${q.hops} hops via ${q.venues.join(", ")}, fee ${q.feeBps} bps`);
  const b = await buildSwap(q, sender, 100, deps);
  console.log(`swap build  ${b.tx.getData().commands.length} commands, min out ${b.minOut != null ? fromBase(b.minOut, 6) : "?"} USDC`);
  const d = await dryRun(b.tx, sender);
  const usdc = d.changes.find((c) => c.coinType === USDC_TYPE);
  console.log(`swap dry run ${d.ok ? "ok" : `FAILED: ${d.error} ${d.raw ?? ""}`}, receives ${usdc ? fromBase(usdc.amount, 6) : "?"} USDC, gas ${d.gasMist != null ? fromBase(d.gasMist, 9) : "?"} SUI`);
  if (!d.ok || !usdc || usdc.amount <= 0n) failed = true;
}

// 2. Perps: the account transaction the first order needs.
{
  const r = await call<{ tx?: string; error?: string }>("POST", "/ext/perps/tx", { address: sender, req: { kind: "create-account" } });
  if (r.status !== 200 || !r.body?.tx) {
    console.log(`perps build FAILED: ${r.body?.error ?? r.status}`);
    failed = true;
  } else {
    const tx = Transaction.from(r.body.tx);
    console.log(`perps build create-account, ${tx.getData().commands.length} commands`);
    const d = await dryRun(tx, sender);
    console.log(`perps dry run ${d.ok ? "ok" : `FAILED: ${d.error}`}, gas ${d.gasMist != null ? fromBase(d.gasMist, 9) : "?"} SUI`);
    if (!d.ok) failed = true;
  }
}

process.exit(failed ? 1 : 0);
