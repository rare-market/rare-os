// Mainnet dry run of the Store's transactions, simulate only: LIVE=1 npx vitest run src/store/tx.live.test.ts
// Nothing is signed and nothing is executed.
//
// Always: the Pyth SUI/USD update sequence buy_sui starts with, from rare.market's
// /v1/store/price/sui (or, before that route is live, a recorded Hermes update), simulated on
// mainnet. Once /v1/store/config has a package: buy_sui on a listed uploaded game, claim_free on a
// free one, and buy_usdc when the sender holds enough USDC, each built with the launcher's own
// builders and simulated with the sender below.

import { readFileSync } from "node:fs";
import { Transaction } from "@mysten/sui/transactions";
import { toBase64 } from "@mysten/sui/utils";
import { describe, expect, it } from "vitest";
import { dryRun } from "../trade/chain";
import { grpcClient } from "../wallet/grpc";
import { canSell, storeApi } from "./api";
import { currentPriceUsd, parseGame } from "./chain";
import { addSuiUsdUpdateFrom, updateBytes } from "./pyth";
import { buildBuySui, buildBuyUsdc, buildClaimFree, idsOf } from "./tx";
import type { SuiPrice } from "./types";

const LIVE = !!process.env.LIVE;
// The demo wallet the screenshots watch: holds SUI for gas.
const SENDER = process.env.SENDER ?? "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

const get = async <T,>(p: string): Promise<T> => {
  const r = await fetch(`https://rare.market${p}`);
  if (!r.ok) throw new Error(`rare.market ${r.status} ${p}`);
  return (await r.json()) as T;
};
const api = storeApi(get);

async function suiPrice(): Promise<SuiPrice & { recorded?: boolean }> {
  try {
    return await api.suiPrice();
  } catch {
    const hex = readFileSync(new URL("./fixtures/pyth-update.hex", import.meta.url), "utf8").trim();
    const bytes = Uint8Array.from(hex.match(/../g)!.map((h) => parseInt(h, 16)));
    return { price_e8: "0", conf_e8: "0", publish_time: 0, update_b64: toBase64(bytes), recorded: true };
  }
}

describe.skipIf(!LIVE)("Rare Store on mainnet (dry run)", () => {
  it("the Pyth update sequence simulates", async () => {
    const p = await suiPrice();
    const tx = new Transaction();
    addSuiUsdUpdateFrom(tx, updateBytes(p.update_b64));
    const r = await dryRun(tx, SENDER);
    console.log(`  pyth update (${p.recorded ? "recorded update" : "/v1/store/price/sui"}): ok=${r.ok}${r.error ? ` error="${r.error}"` : ""} gas=${r.gasMist}`);
    expect(r.ok).toBe(true);
  }, 60_000);

  it("buys simulate once the store is deployed", async () => {
    const cfg = await api.config().catch(() => null);
    if (!canSell(cfg)) {
      console.log("  /v1/store/config has no package yet: buy dry runs skipped");
      return;
    }
    const ids = idsOf(cfg);
    const { items } = await api.games("all", { kind: "uploaded", limit: 24 });
    let ran = 0;
    for (const g of items.filter((x) => x.game_id)) {
      const obj = await grpcClient.core.getObject({ objectId: g.game_id!, include: { content: true } });
      const chain = parseGame(obj.object.content);
      if (!chain.listed || chain.blocked) continue;
      const price = currentPriceUsd(chain);
      if (price === 0n) {
        const r = await dryRun(buildClaimFree(ids, { sender: SENDER, gameId: chain.id }), SENDER);
        console.log(`  claim_free ${g.slug}: ok=${r.ok} ${r.error ?? ""}`);
        expect(r.ok).toBe(true);
        ran++;
        continue;
      }
      if (chain.acceptSui) {
        const { tx, mist } = buildBuySui(ids, { sender: SENDER, gameId: chain.id, priceUsd: price, price: await api.suiPrice() });
        const r = await dryRun(tx, SENDER);
        console.log(`  buy_sui ${g.slug} (${mist} MIST): ok=${r.ok} ${r.error ?? ""}`);
        expect(r.ok).toBe(true);
        ran++;
      }
      const usdc = await grpcClient.core.getBalance({ owner: SENDER, coinType: ids.usdcType }).then((b) => BigInt(b.balance.balance)).catch(() => 0n);
      if (usdc >= price) {
        const r = await dryRun(buildBuyUsdc(ids, { sender: SENDER, gameId: chain.id, priceUsd: price }), SENDER);
        console.log(`  buy_usdc ${g.slug}: ok=${r.ok} ${r.error ?? ""}`);
        expect(r.ok).toBe(true);
        ran++;
      }
      if (ran >= 3) break;
    }
    console.log(`  ${ran} store purchases simulated`);
  }, 120_000);
});
