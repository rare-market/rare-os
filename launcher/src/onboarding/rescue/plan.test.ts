import { describe, expect, it } from "vitest";
import { buildBatch, planMoves, formatAmount } from "./plan";
import { SUI_TYPE } from "./scan";
import { usesGasCoin } from "../../wallet/sponsor";
import type { RescueCoin, RescueKiosk, RescueScan } from "./types";

const A = (n: number) => `0x${n.toString(16).padStart(64, "0")}`;
const FROM = A(0xf00);
const TO = A(0xabc);
const USDC = `${A(0xdba3)}::usdc::USDC`;

function coin(coinType: string, symbol: string, n: number, each: bigint, address = 0n): RescueCoin {
  const objectIds = Array.from({ length: n }, (_, i) => A(0x10000 + i + (symbol === "SUI" ? 0 : 0x8000)));
  const coinRaw = each * BigInt(n);
  return {
    coinType,
    symbol,
    name: symbol,
    decimals: symbol === "SUI" ? 9 : 6,
    iconUrl: null,
    raw: coinRaw + address,
    coinRaw,
    addressRaw: address,
    amount: 0,
    objectIds,
    objectBalances: objectIds.map(() => each),
  };
}

function scan(over: Partial<RescueScan> = {}): RescueScan {
  return {
    address: FROM,
    scannedAt: 0,
    defaultName: null,
    coins: [],
    nfts: [],
    kiosks: [],
    staked: [],
    names: [],
    soulbound: [],
    truncated: false,
    ...over,
  };
}

const kiosk = (id: number, personal: boolean, items: RescueKiosk["items"] = []): RescueKiosk => ({
  kioskId: A(id),
  capId: A(id + 1),
  personal,
  personalPackage: personal ? A(0xdfb4) : null,
  items,
  profits: 0n,
});

describe("planMoves", () => {
  it("orders kiosks, stakes, names, collectibles, then coins", () => {
    const p = planMoves(
      scan({
        coins: [coin(SUI_TYPE, "SUI", 3, 1_000_000_000n), coin(USDC, "USDC", 2, 5_000_000n)],
        nfts: [{ objectId: A(1), type: `${A(9)}::m::N`, name: "N", image: null, collection: "C" }],
        kiosks: [kiosk(0x100, false), kiosk(0x200, false)],
        staked: [{ objectId: A(2), principal: 5_000_000_000n, poolId: A(3), activationEpoch: 1 }],
        names: [{ objectId: A(4), domain: "me.sui", expiresMs: 0, subname: false }],
      }),
      TO,
    );
    expect(p.batches.map((b) => b.title)).toEqual(["Kiosks", "Staked SUI", "SuiNS names", "Collectibles", "Coins"]);
    expect(p.batches[0].lines).toHaveLength(2);
    expect(p.batches.at(-1)!.lines.at(-1)!.detail).toMatch(/All SUI that's left/);
    expect(p.empty).toBe(false);
    expect(p.feeNote).toBeNull();
  });

  it("is empty when nothing can move, and lists soulbound items as stuck", () => {
    const p = planMoves(scan({ soulbound: [{ objectId: A(5), type: `${A(0xd7b0)}::suiplay_nft::SuiPlay0x1PreorderNFT`, name: "Eternal", image: null, collection: "x" }] }), TO);
    expect(p.empty).toBe(true);
    expect(p.stuck[0].reason).toMatch(/stay with the wallet/);
  });

  it("moves personal kiosk items one by one and leaves locked or exclusive ones", () => {
    const item = (n: number, extra: object) => ({ objectId: A(n), type: `${A(9)}::m::N`, name: `N${n}`, image: null, collection: "C", listed: null, locked: false, transferable: true, ...extra });
    const p = planMoves(scan({ kiosks: [kiosk(0x300, true, [item(1, {}), item(2, { locked: true }), item(3, { listed: 2_000_000_000n }), item(4, { listed: 1n, exclusive: true })])] }), TO);
    expect(p.batches).toHaveLength(1);
    expect(p.batches[0].lines.map((l) => l.label)).toEqual(["N1", "N3"]);
    expect(p.batches[0].lines[1].detail).toMatch(/delisted first/);
    expect(p.stuck.map((s) => s.label)).toEqual(["N2", "N4"]);
    const tx = buildBatch(scan({ kiosks: [kiosk(0x300, true, [item(1, {}), item(3, { listed: 2_000_000_000n })])] }), p, 1);
    const calls = tx.getData().commands.map((c) => (c.MoveCall ? `${c.MoveCall.module}::${c.MoveCall.function}` : c.$kind));
    expect(calls).toEqual(["personal_kiosk::borrow_val", "kiosk::take", "kiosk::delist", "kiosk::take", "personal_kiosk::return_val", "TransferObjects"]);
  });

  it("splits large coin sets under the input limit and keeps big SUI coins for gas", () => {
    const s = scan({ coins: [coin(SUI_TYPE, "SUI", 900, 10_000_000n), coin(USDC, "USDC", 1500, 1n)] });
    const p = planMoves(s, TO);
    expect(p.batches.length).toBeGreaterThanOrEqual(3);
    for (const b of p.batches) {
      const tx = buildBatch(s, p, b.index);
      const inputs = tx.getData().inputs.length;
      expect(inputs).toBeLessThanOrEqual(1010);
      for (const c of tx.getData().commands) if (c.TransferObjects) expect(c.TransferObjects.objects.length).toBeLessThanOrEqual(400);
    }
    const last = buildBatch(s, p, p.batches.length);
    const cmds = last.getData().commands;
    expect(cmds.at(-1)!.TransferObjects!.objects[0].$kind).toBe("GasCoin");
    // 8 coins of 0.01 SUI stay out of explicit transfers to pay fees
    const all = p.batches.flatMap((b) => buildBatch(s, p, b.index).getData().inputs.filter((i) => i.UnresolvedObject).map((i) => i.UnresolvedObject!.objectId));
    expect(all.filter((id) => s.coins[0].objectIds.includes(id))).toHaveLength(900 - 8);
  });

  it("sends SUI like any coin when gas is sponsored", () => {
    const s = scan({ coins: [coin(SUI_TYPE, "SUI", 2, 1_000n)] });
    const p = planMoves(s, TO, "sponsored");
    const tx = buildBatch(s, p, 1, "sponsored");
    expect(tx.getData().commands.some((c) => c.TransferObjects?.objects.some((o) => o.$kind === "GasCoin"))).toBe(false);
    expect(p.feeNote).toBeNull();
  });

  it("never touches the gas coin in any sponsored batch (rare.market refuses those)", () => {
    const kiosk = { kioskId: A(0x501), capId: A(0x502), personal: true, personalPackage: A(0x503), profits: 5n, items: [] } as unknown as RescueKiosk;
    const s = scan({ coins: [coin(SUI_TYPE, "SUI", 3, 1_000n, 500n), coin(USDC, "USDC", 2, 10n, 7n)], kiosks: [kiosk], nfts: [{ objectId: A(1), type: "x", name: "N", image: null, collection: "C" }] });
    const p = planMoves(s, TO, "sponsored");
    for (const b of p.batches) expect(usesGasCoin(buildBatch(s, p, b.index, "sponsored").getData().commands)).toBe(false);
    // own gas does: the last batch sends the gas coin itself
    const own = planMoves(s, TO);
    expect(usesGasCoin(buildBatch(s, own, own.batches.length).getData().commands)).toBe(true);
  });

  it("warns when the wallet can't pay its own fees", () => {
    const p = planMoves(scan({ coins: [coin(SUI_TYPE, "SUI", 1, 1_000n)], nfts: [{ objectId: A(1), type: "x", name: "N", image: null, collection: "C" }] }), TO);
    expect(p.feeNote).toMatch(/needs a little SUI/);
  });
});

describe("formatAmount", () => {
  it("formats base units", () => {
    expect(formatAmount(1_500_000_000n, 9)).toBe("1.5");
    expect(formatAmount(12_345_678_900_000n, 9)).toBe("12,345.6789");
    expect(formatAmount(1n, 9)).toBe("0.0001");
    expect(formatAmount(0n, 9)).toBe("0");
  });
});
