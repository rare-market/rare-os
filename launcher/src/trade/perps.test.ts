import { Transaction } from "@mysten/sui/transactions";
import { describe, expect, it, vi } from "vitest";
import type { Reply } from "./http";
import { buildPerpTx, fundingPct, fundingPlan, liqText, openRequest, perpPaths, perpPx, priceAtReturn } from "./perps";

vi.mock("../lib/bridge", () => ({ isTauri: false }));

const ADDR = `0x${"B2".repeat(32)}`;
const MARKET = `0x${"c3".repeat(32)}`;

/** What the perps API answers: Transaction JSON v2 with the sender set. */
async function serverTx(sender: string) {
  const tx = new Transaction();
  const [coin] = tx.splitCoins(tx.gas, [1_000]);
  tx.transferObjects([coin], sender);
  tx.setSender(sender);
  return tx.toJSON();
}

describe("buildPerpTx", () => {
  it("posts the request for the wallet and returns the unsigned transaction", async () => {
    const json = await serverTx(ADDR.toLowerCase());
    const call = vi.fn(async (): Promise<Reply<unknown>> => ({ status: 200, body: { tx: json } }));
    const tx = await buildPerpTx(ADDR, openRequest({ marketId: MARKET, side: "long", marginUsd: 10, leverage: 5, shortBy: 0 }), call as never);
    expect(call).toHaveBeenCalledWith("POST", "/ext/perps/tx", {
      address: ADDR.toLowerCase(),
      req: { kind: "open", marketId: MARKET, side: "long", marginUsd: 10, leverage: 5, slippage: 0.01 },
    });
    const data = tx.getData();
    expect(data.sender).toBe(ADDR.toLowerCase());
    expect(data.commands.map((c) => c.$kind)).toEqual(["SplitCoins", "TransferObjects"]);
  });

  it("builds close orders", async () => {
    const call = vi.fn(async (): Promise<Reply<unknown>> => ({ status: 200, body: { tx: await serverTx(ADDR.toLowerCase()) } }));
    await buildPerpTx(ADDR, { kind: "close", marketId: MARKET }, call as never);
    expect((call.mock.calls[0] as unknown[])[2]).toEqual({ address: ADDR.toLowerCase(), req: { kind: "close", marketId: MARKET } });
  });

  it("passes the API's reason on", async () => {
    const call = vi.fn(async (): Promise<Reply<unknown>> => ({ status: 422, body: { error: "Set up your perps account first." } }));
    await expect(buildPerpTx(ADDR, { kind: "close", marketId: MARKET }, call as never)).rejects.toThrow("Set up your perps account first.");
  });
});

describe("funding", () => {
  it("folds the shortfall into the order as a deposit", () => {
    const plan = fundingPlan({ availableUsd: 4 }, 10, 0.05, 50);
    expect(plan).toEqual({ shortBy: 6.25, blocked: false, needsAccount: false });
    expect(openRequest({ marketId: MARKET, side: "short", marginUsd: 10, leverage: 3, shortBy: plan.shortBy })).toMatchObject({ depositUsd: 6.25, side: "short" });
  });

  it("blocks when the wallet cannot cover it", () => {
    expect(fundingPlan({ availableUsd: 0 }, 10, 0.05, 2).blocked).toBe(true);
    expect(fundingPlan(null, 10, 0, 100).needsAccount).toBe(true);
    expect(fundingPlan({ availableUsd: 100 }, 10, 0.05, 0)).toEqual({ shortBy: 0, blocked: false, needsAccount: false });
  });

  it("prints funding rates", () => {
    expect(fundingPct(0.0000027839562203562616)).toBe("0.000278%");
    expect(fundingPct(0.0002, 8)).toBe("0.1600%");
  });
});

describe("numbers", () => {
  it("prints perps prices", () => {
    expect(perpPx(343.17069)).toBe("$343.17");
    expect(perpPx(2.90502)).toBe("$2.905");
    expect(perpPx(0.14604001378161055)).toBe("$0.14604");
    expect(perpPx(0)).toBe("");
    expect(perpPx(null)).toBe("");
  });

  it("says when a liquidation price is out of reach", () => {
    expect(liqText(8.187e-15, 79_935.97)).toBe("Near $0");
    expect(liqText(27_303.06, 79_935.97)).toBe("$27,303.06");
    expect(liqText(null, 1)).toBe("");
  });

  it("finds the price for a return on margin", () => {
    expect(priceAtReturn({ side: "long", entryPrice: 100, size: 1, marginUsd: 10 }, 50)).toBe(105);
    expect(priceAtReturn({ side: "short", entryPrice: 100, size: 1, marginUsd: 10 }, 50)).toBe(95);
    expect(priceAtReturn({ side: "long", entryPrice: 100, size: 0, marginUsd: 10 }, 50)).toBeNull();
  });

  it("builds query paths", () => {
    expect(perpPaths.preview(MARKET, "long", 10, 5)).toBe(`/ext/perps/preview?market=${MARKET}&side=long&margin=10&leverage=5`);
    expect(perpPaths.account(ADDR)).toBe(`/ext/perps/account?address=${ADDR.toLowerCase()}`);
  });
});
