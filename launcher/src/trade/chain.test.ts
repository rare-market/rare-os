import { Inputs, Transaction } from "@mysten/sui/transactions";
import { describe, expect, it, vi } from "vitest";
import { gasFromSpentSui, plainChainError } from "./chain";

const ME = `0x${"d4".repeat(32)}`;
const COIN = { objectId: `0x${"e5".repeat(32)}`, version: "7", digest: "Gj9jDVxbq7hU7DPdBcrcB1PLYjK3Eh7X6eo9kkJ2G3F" };
const OTHER = { objectId: `0x${"f6".repeat(32)}`, version: "9", digest: "Gj9jDVxbq7hU7DPdBcrcB1PLYjK3Eh7X6eo9kkJ2G3F" };

function client(types: Record<string, string>) {
  return {
    core: {
      getObjects: vi.fn(async ({ objectIds }: { objectIds: string[] }) => ({ objects: objectIds.map((id) => ({ objectId: id, type: types[id] })) })),
    },
  } as never;
}

/** A router-style kind: an object input, a pure, the wallet's SUI coin split for the trade. */
function routerTx() {
  const tx = new Transaction();
  const pool = tx.object(Inputs.ObjectRef(OTHER));
  const amount = tx.pure.u64(500);
  const sui = tx.object(Inputs.ObjectRef(COIN));
  const [part] = tx.splitCoins(sui, [amount]);
  tx.moveCall({ target: "0x2::pay::keep", arguments: [pool, part] });
  tx.transferObjects([sui], ME);
  tx.setSender(ME);
  return tx;
}

describe("gasFromSpentSui", () => {
  it("pays gas from the SUI coin the trade spends and points the trade at the gas coin", async () => {
    const out = await gasFromSpentSui(routerTx(), client({ [COIN.objectId]: "0x2::coin::Coin<0x2::sui::SUI>", [OTHER.objectId]: "0xabc::pool::Pool" }));
    const d = out.getData();
    expect(d.gasData.payment).toEqual([COIN]);
    // pool, amount, recipient: the coin input is gone.
    expect(d.inputs).toHaveLength(3);
    expect(d.inputs[0].Object?.ImmOrOwnedObject?.objectId).toBe(OTHER.objectId);
    expect(d.commands[0].SplitCoins?.coin).toEqual({ $kind: "GasCoin", GasCoin: true });
    expect(d.commands[0].SplitCoins?.amounts[0]).toMatchObject({ Input: 1 });
    expect(d.commands[1].MoveCall?.arguments[0]).toMatchObject({ Input: 0 });
    expect(d.commands[2].TransferObjects?.objects[0]).toEqual({ $kind: "GasCoin", GasCoin: true });
    // Inputs after the removed one move down by one.
    expect(d.commands[2].TransferObjects?.address).toMatchObject({ Input: 2 });
    expect(d.sender).toBe(ME);
  });

  it("leaves transactions without a SUI coin input alone", async () => {
    const tx = routerTx();
    const out = await gasFromSpentSui(tx, client({ [COIN.objectId]: "0x2::coin::Coin<0xdee::deep::DEEP>", [OTHER.objectId]: "0xabc::pool::Pool" }));
    expect(out).toBe(tx);
  });

  it("keeps a gas payment that is already set", async () => {
    const tx = routerTx();
    tx.setGasPayment([OTHER]);
    const c = client({});
    expect(await gasFromSpentSui(tx, c)).toBe(tx);
  });
});

describe("plainChainError", () => {
  it("says what went wrong in plain words", () => {
    expect(plainChainError(new Error("Unable to perform gas selection due to insufficient SUI balance"))).toMatch(/need SUI for the network fee/);
    expect(plainChainError("MoveAbort(... abort code: 30)")).toMatch(/margin/);
    expect(plainChainError({ kind: "MoveAbort" })).toMatch(/would fail/);
  });
});
