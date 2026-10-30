// Send to a friend, dry run only (never signed, never submitted):
//   LIVE=1 npx vitest run src/friends/send.live.test.ts
// A friend is their SuiNS name: the name resolves over gRPC (zero GraphQL), then the same
// builders Rare Wallet's send pages use build SUI, a coin and an NFT transfer to that address, and
// a mainnet fullnode simulates each one. The sender is a funded demo address; no key is involved.

import { describe, expect, it } from "vitest";
import { dryRun } from "../trade/chain";
import type { ItemCard } from "../lib/rare";
import { buildCoinSend, buildNftSend, nftSendability } from "../wallet/tx";
import { resolveRecipient } from "../wallet/tx/recipient";
import { balances } from "../wallet";

const SENDER = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const FRIEND = process.env.FRIEND ?? "walrus.sui";
const SUI = "0x2::sui::SUI";

const mem = new Map<string, string>();
(globalThis as { localStorage?: unknown }).localStorage = {
  getItem: (k: string) => mem.get(k) ?? null,
  setItem: (k: string, v: string) => void mem.set(k, String(v)),
  removeItem: (k: string) => void mem.delete(k),
  clear: () => mem.clear(),
};

async function friendAddress(): Promise<string> {
  const r = await resolveRecipient(FRIEND);
  expect(r.s, `${FRIEND} did not resolve: ${JSON.stringify(r)}`).toBe("ready");
  return (r as { address: string }).address;
}

describe.skipIf(!process.env.LIVE)(`mainnet: send to ${FRIEND} (dry run)`, () => {
  it("sends SUI to a SuiNS name", async () => {
    const to = await friendAddress();
    const tx = await buildCoinSend({ sender: SENDER, to, coinType: SUI, amount: 10_000_000n });
    const d = await dryRun(tx, SENDER);
    expect(d.ok, `${d.error} ${d.raw ?? ""}`).toBe(true);
    // The sender's SUI goes down by the amount plus gas.
    const sui = d.changes.find((c) => c.coinType.endsWith("::sui::SUI"));
    expect(sui && sui.amount <= -10_000_000n).toBe(true);
  }, 60_000);

  it("sends another coin to a SuiNS name", async () => {
    const to = await friendAddress();
    const held = (await balances(SENDER)).filter((b) => !b.coinType.endsWith("::sui::SUI") && b.raw > 0n);
    expect(held.length, "the demo address holds no other coin").toBeGreaterThan(0);
    const coin = held[0];
    const amount = coin.raw / 10n || 1n;
    const tx = await buildCoinSend({ sender: SENDER, to, coinType: coin.coinType, amount });
    const d = await dryRun(tx, SENDER);
    expect(d.ok, `${coin.coinType}: ${d.error} ${d.raw ?? ""}`).toBe(true);
  }, 60_000);

  it("sends an NFT to a SuiNS name", async () => {
    const to = await friendAddress();
    const items = ((await (await fetch(`https://rare.market/v1/accounts/${SENDER}/items?limit=40`)).json()) as { data: ItemCard[] }).data;
    const can = await nftSendability(SENDER, items.map((c) => ({ objectId: c.id, type: c.type, kiosk: !!c.kioskId })));
    const pick = [...can.values()].find((s) => s.ok);
    expect(pick?.ok, "no sendable NFT on the demo address").toBe(true);
    if (!pick || !pick.ok) return;
    const tx = buildNftSend({ sender: SENDER, to, items: [pick.send] });
    const d = await dryRun(tx, SENDER);
    expect(d.ok, `${d.error} ${d.raw ?? ""}`).toBe(true);
  }, 60_000);
});
