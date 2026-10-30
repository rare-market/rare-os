import { normalizeSuiAddress } from "@mysten/sui/utils";
import { describe, expect, it, vi } from "vitest";
import { SUI_TYPE } from "../grpc";
import { chipAmount, cleanAmount, formatAmount, formatUnits, parseUnits, SUI_GAS_RESERVE, toInput, usdToBase } from "./amount";
import { buildCoinSend, SendError, type CoinSendClient } from "./coin";
import { buildNftSend } from "./nftSend";
import { parseRecipient, recipientState, shortAddress, toDotSui } from "./recipient";

const ME = "0x" + "a1".repeat(32);
const BOB = "0x" + "b2".repeat(32);
const USDC = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";
const A = "0x1eb7c57e3f2bd0fc6cb9dcffd143ea957e4d98f805c358733f76dee0667fe0b1";

describe("amount", () => {
  const ONE = 1_000_000_000n;
  it("25% / 50% are plain fractions; Max on SUI keeps the gas reserve", () => {
    expect(chipAmount(10n * ONE, 25, true)).toBe(2_500_000_000n);
    expect(chipAmount(10n * ONE, 50, true)).toBe(5n * ONE);
    expect(chipAmount(10n * ONE, 100, true)).toBe(10n * ONE - SUI_GAS_RESERVE);
  });
  it("Max on other coins is the whole balance", () => {
    expect(chipAmount(50_000_000n, 100, false)).toBe(50_000_000n);
    expect(chipAmount(50_000_000n, 50, false)).toBe(25_000_000n);
  });
  it("never dips into the reserve on a tiny SUI balance", () => {
    expect(chipAmount(SUI_GAS_RESERVE, 100, true)).toBe(0n);
    expect(chipAmount(12_000_000n, 50, true)).toBe(2_000_000n);
  });
  it("USD to token units rounds down at the price", () => {
    expect(usdToBase("10", 4, 9)).toBe("2500000000");
    expect(usdToBase("1", 3, 6)).toBe("333333");
    expect(usdToBase("", 4, 9)).toBeNull();
    expect(usdToBase("5", undefined, 9)).toBeNull();
  });
  it("parses and formats base units", () => {
    expect(parseUnits("1.5", 9)).toBe("1500000000");
    expect(parseUnits(".25", 6)).toBe("250000");
    expect(parseUnits("1.1234567", 6)).toBe("1123456");
    expect(parseUnits("", 9)).toBeNull();
    expect(parseUnits("1e5", 9)).toBeNull();
    expect(formatUnits(1_234_567_890_000n, 9)).toBe("1,234.5678");
    expect(formatUnits(-1_500_000_000n, 9)).toBe("-1.5");
    expect(toInput(1_234_567_890_123n, 9)).toBe("1234.567890123");
    expect(cleanAmount("1,5")).toBe("1.5");
    expect(cleanAmount("1.2.3")).toBeNull();
  });
  it("formatAmount keeps small amounts readable", () => {
    expect(formatAmount(1_997_880n, 9)).toBe("0.001997");
    expect(formatAmount(2_500_000_000n, 9)).toBe("2.5");
    expect(formatAmount(0n, 9)).toBe("0");
    expect(formatAmount(1n, 9)).toBe("0.000000001");
  });
});

describe("recipient", () => {
  it("parses addresses and names and explains what is wrong", () => {
    expect(parseRecipient(A)).toEqual({ kind: "address", address: A });
    expect(parseRecipient(` ${A.toUpperCase().replace("0X", "0x")} `)).toEqual({ kind: "address", address: A });
    expect(parseRecipient("Adeniyi.SUI")).toEqual({ kind: "name", name: "adeniyi.sui", display: "adeniyi.sui" });
    expect(parseRecipient("@adeniyi")).toMatchObject({ kind: "name", name: "adeniyi.sui" });
    expect(parseRecipient("pay@adeniyi")).toMatchObject({ kind: "name", name: "pay.adeniyi.sui" });
    expect(parseRecipient("")).toEqual({ kind: "empty" });
    expect(parseRecipient("0x12")).toMatchObject({ kind: "invalid", reason: "Address should be 0x and 64 characters" });
    expect(parseRecipient("0x12zz")).toMatchObject({ kind: "invalid", reason: "Address has characters that are not hex" });
    expect(parseRecipient("bad_name.sui")).toMatchObject({ kind: "invalid", reason: "Names use letters, numbers and hyphens" });
    expect(parseRecipient("hello")).toMatchObject({ kind: "invalid", reason: "Enter a 0x address or a .sui name" });
    expect(toDotSui("@bob")).toBe("bob.sui");
    expect(toDotSui("bob")).toBeNull();
    expect(shortAddress(A)).toBe("0x1eb7...e0b1");
  });
  it("field state: address, name and invalid input", () => {
    expect(recipientState(parseRecipient(A), { status: "idle" }, "alice.sui", true)).toEqual({ s: "ready", address: A, suins: "alice.sui", typed: "address" });
    expect(recipientState(parseRecipient("@alice"), { status: "found", address: A }, null, true)).toEqual({ s: "ready", address: A, suins: "alice.sui", typed: "name" });
    expect(recipientState(parseRecipient("alice.sui"), { status: "loading" }, null, true)).toEqual({ s: "loading", name: "alice.sui" });
    expect(recipientState(parseRecipient("nobody.sui"), { status: "missing" }, null, true)).toMatchObject({ s: "missing", msg: "No address is set for nobody.sui" });
    expect(recipientState(parseRecipient("nobody.sui"), { status: "error" }, null, true)).toMatchObject({ s: "error" });
    expect(recipientState(parseRecipient("0x12"), { status: "idle" }, null, false)).toEqual({ s: "typing" });
    expect(recipientState(parseRecipient("0x12"), { status: "idle" }, null, true)).toMatchObject({ s: "invalid" });
  });
});

const coinsClient = (balance: string, coins = 3): CoinSendClient => ({
  core: {
    getBalance: vi.fn(async () => ({ balance: { balance } })),
    listCoins: vi.fn(async () => ({
      objects: Array.from({ length: coins }, (_, i) => ({ objectId: "0x" + (i + 1).toString(16).padStart(64, "0"), version: "1", digest: "4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi", balance: "1" })),
      hasNextPage: false,
      cursor: null,
    })),
  },
});
const kinds = (tx: { getData(): { commands: Array<{ $kind: string }> } }) => tx.getData().commands.map((c) => c.$kind);

describe("buildCoinSend", () => {
  it("SUI: split from gas and transfer, sender set", async () => {
    const tx = await buildCoinSend({ sender: ME, to: BOB, coinType: "0x2::sui::SUI", amount: 1_000_000_000n, client: coinsClient("5000000000") });
    expect(tx.getData().sender).toBe(ME);
    expect(kinds(tx)).toEqual(["SplitCoins", "TransferObjects"]);
    const split = tx.getData().commands[0].SplitCoins!;
    expect(split.coin).toEqual({ $kind: "GasCoin", GasCoin: true });
  });

  it("SUI max: pays with every SUI coin and sends the gas coin itself", async () => {
    const client = coinsClient("5000000000", 3);
    const tx = await buildCoinSend({ sender: ME, to: BOB, coinType: SUI_TYPE, amount: 0n, max: true, client });
    expect(kinds(tx)).toEqual(["TransferObjects"]);
    expect(tx.getData().commands[0].TransferObjects!.objects).toEqual([{ $kind: "GasCoin", GasCoin: true }]);
    expect(tx.getData().gasData.payment).toHaveLength(3);
  });

  it("other coins use coinWithBalance", async () => {
    const tx = await buildCoinSend({ sender: ME, to: BOB, coinType: USDC, amount: 2_000_000n, balance: 5_000_000n });
    expect(kinds(tx)).toEqual(["$Intent", "TransferObjects"]);
  });

  it("validates address, amount and balance", async () => {
    const base = { sender: ME, to: BOB, coinType: USDC, amount: 1n, balance: 10n };
    await expect(buildCoinSend({ ...base, to: "0x12zz" })).rejects.toMatchObject({ code: "bad-address" });
    await expect(buildCoinSend({ ...base, to: ME })).rejects.toMatchObject({ code: "same-address" });
    await expect(buildCoinSend({ ...base, amount: 0n })).rejects.toMatchObject({ code: "bad-amount" });
    await expect(buildCoinSend({ ...base, amount: 11n })).rejects.toThrow("Not enough balance");
    await expect(buildCoinSend({ ...base, balance: 0n, max: true })).rejects.toThrow("Nothing to send");
    await expect(buildCoinSend({ ...base, coinType: SUI_TYPE, amount: 10n })).rejects.toMatchObject({ code: "fee-reserve" });
    await expect(buildCoinSend({ ...base, to: "0x12zz" })).rejects.toBeInstanceOf(SendError);
  });
});

describe("buildNftSend", () => {
  it("wraps the ported builder with sender set and address checks", () => {
    const id = normalizeSuiAddress("0x5");
    const tx = buildNftSend({ sender: ME, to: BOB, items: [{ from: null, item: { nftId: id, nftType: "0x9::a::B", isLocked: false } }] });
    expect(tx.getData().sender).toBe(ME);
    expect(kinds(tx)).toEqual(["TransferObjects"]);
    expect(() => buildNftSend({ sender: ME, to: BOB, items: [] })).toThrow("Pick at least one item");
    expect(() => buildNftSend({ sender: ME, to: "nope", items: [] })).toThrow("Enter a valid Sui address");
  });
});
