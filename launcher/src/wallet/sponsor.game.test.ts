// Game gas budgets (category "game:<slug>"), against a mocked rare.market sponsor API: the shapes
// and codes of market api/src/sponsor (game.ts, service.ts, public/schemas.ts SponsorStatus.game).
import { Transaction } from "@mysten/sui/transactions";
import { toBase58, toBase64 } from "@mysten/sui/utils";
import { afterEach, describe, expect, it } from "vitest";
import { clearGameGasStatus, gameCategory, gameFeeNote, gameNotCovered, kindOfBytes, sponsorForGame, type SponsorDeps, type SponsorStatus } from "./sponsor";
import { rejectRequest, signAndExecuteForGame, useSignRequests, type SignResult } from "./sign";
import { describeReview } from "./review/describe";
import type { Insights } from "./review/analyze";

const A = (n: number) => `0x${n.toString(16).padStart(64, "0")}`;
const USER = A(0xabc);
const SPONSOR = A(0x5905);
const ref = (n: number) => ({ objectId: A(n), version: "7", digest: toBase58(new Uint8Array(32).fill(n % 250)) });

function gameTx(): Transaction {
  const tx = new Transaction();
  tx.setSender(USER);
  tx.moveCall({ target: `${A(0x9a)}::game::play`, arguments: [tx.objectRef(ref(0x77))] });
  return tx;
}

async function sponsoredBytes(kindB64: string): Promise<string> {
  const tx = Transaction.fromKind(kindB64);
  tx.setSender(USER);
  tx.setGasOwner(SPONSOR);
  tx.setGasPayment([ref(0x99)]);
  tx.setGasPrice(1000);
  tx.setGasBudget(5_000_000);
  return toBase64(await tx.build());
}

const status = (game: Partial<NonNullable<SponsorStatus["game"]>> | null, over: Partial<SponsorStatus> = {}): SponsorStatus => ({
  enabled: true,
  funded: true,
  address: SPONSOR,
  balanceMist: "5000000000",
  dailyBudgetMist: "1000000000",
  spentTodayMist: "0",
  remainingTodayMist: "1000000000",
  categories: { rescue: true, trophy: true, trade: true, walrus: false },
  maxUserSuiMist: "50000000",
  ...(game ? { game: { slug: "act", covered: true, availableMist: "2000000000", paused: false, you: { todayTx: 0, todayMist: "0", eligible: true }, ...game } } : {}),
  ...over,
});

type Post = (body: Record<string, string>) => Promise<{ status: number; body: unknown }> | { status: number; body: unknown };

function deps(s: SponsorStatus | null, post: Post): SponsorDeps & { gets: string[]; posts: Array<Record<string, string>> } {
  const gets: string[] = [];
  const posts: Array<Record<string, string>> = [];
  let t = 1_000_000;
  return {
    gets,
    posts,
    get: async <T,>(path: string) => {
      gets.push(path);
      if (!s) throw new Error("rare.market 503");
      return { data: s } as T;
    },
    post: async <T,>(_path: string, body: unknown) => {
      posts.push(body as Record<string, string>);
      return (await post(body as Record<string, string>)) as { status: number; body: T };
    },
    client: undefined,
    now: () => (t += 1000),
  };
}

const grant = async (b: Record<string, string>) => ({
  status: 200,
  body: { data: { bytes: await sponsoredBytes(b.kind), sponsorSignature: "c3Bvbg==", digest: "D1", sponsor: SPONSOR, category: b.category, gasBudget: "5000000", estimatedGasMist: "1200000", expiresAt: 9_999_999_999_999 } },
});
const err = (status: number, code: string, message = code) => ({ status, body: { error: { code, message } } });

afterEach(() => clearGameGasStatus());

describe("game categories", () => {
  it("game:<slug> for slugs the API accepts", () => {
    expect(gameCategory("pawtato-land")).toBe("game:pawtato-land");
    expect(gameCategory(null)).toBeNull();
    expect(gameCategory("Bad Slug")).toBeNull();
    expect(gameCategory("-x")).toBeNull();
    expect(gameCategory("a".repeat(65))).toBeNull();
  });
  it("reads the game's budget from /v1/sponsor/status?game=", () => {
    expect(gameNotCovered(status({}), "act")).toBeNull();
    expect(gameNotCovered(status({ availableMist: "0", covered: false }), "act")).toMatchObject({ reason: "exhausted", code: "budget_exhausted" });
    expect(gameNotCovered(status({ paused: true }), "act")).toMatchObject({ reason: "not-covered" });
    expect(gameNotCovered(status({ you: { todayTx: 50, todayMist: "1", eligible: false } }), "act")).toMatchObject({ reason: "limited" });
    expect(gameNotCovered(status(null), "act")).toMatchObject({ reason: "not-covered" });
    expect(gameNotCovered(status({}, { funded: false }), "act")).toMatchObject({ reason: "unavailable" });
    expect(gameNotCovered(null, "act")).toMatchObject({ reason: "network" });
  });
});

describe("sponsorForGame", () => {
  it("asks with category game:<slug> and gets the sponsored transaction", async () => {
    const d = deps(status({}), grant);
    const r = await sponsorForGame(gameTx(), USER, "act", d);
    expect(r.ok).toBe(true);
    expect(d.gets[0]).toBe(`/v1/sponsor/status?address=${USER}&game=act`);
    expect(d.posts[0].category).toBe("game:act");
    if (r.ok) expect(r.category).toBe("game:act");
  });
  it("takes the full bytes a web game built and sends only their kind", async () => {
    const tx = gameTx();
    tx.setGasOwner(USER);
    tx.setGasPayment([ref(0x55)]);
    tx.setGasPrice(1000);
    tx.setGasBudget(3_000_000);
    const bytes = await tx.build();
    const kind = kindOfBytes(bytes);
    expect("kind" in kind).toBe(true);
    const d = deps(status({}), grant);
    const r = await sponsorForGame(bytes, USER, "act", d);
    expect(r.ok).toBe(true);
    expect(d.posts[0].kind).toBe((kind as { kind: string }).kind);
  });
  it("budget_exhausted (429): the player pays, with a note", async () => {
    const d = deps(status({}), () => err(429, "budget_exhausted", "This game's gas budget is used up"));
    const r = await sponsorForGame(gameTx(), USER, "act", d);
    expect(r).toMatchObject({ ok: false, reason: "exhausted" });
    if (!r.ok) expect(gameFeeNote(r, "ACT")).toBe("ACT's gas budget is used up, so you pay this fee.");
  });
  it("503, 403 and rate limits fall back quietly (or with the limit note)", async () => {
    for (const [answer, reason, note] of [
      [err(503, "sponsor_unavailable"), "unavailable", null],
      [err(403, "not_sponsored", "Not covered by ACT's gas"), "refused", null],
      [err(429, "rate_limited"), "limited", "ACT has covered as many of your fees as it can today, so you pay this one."],
    ] as const) {
      const r = await sponsorForGame(gameTx(), USER, "act", deps(status({}), () => answer));
      expect(r).toMatchObject({ ok: false, reason });
      if (!r.ok) expect(gameFeeNote(r, "ACT")).toBe(note);
      clearGameGasStatus();
    }
  });
  it("doesn't ask when the status says the budget won't pay", async () => {
    const d = deps(status({ availableMist: "0", covered: false }), grant);
    expect(await sponsorForGame(gameTx(), USER, "act", d)).toMatchObject({ ok: false, reason: "exhausted" });
    expect(d.posts).toHaveLength(0);
    clearGameGasStatus();
    const off = deps(null, grant);
    expect(await sponsorForGame(gameTx(), USER, "act", off)).toMatchObject({ ok: false, reason: "network" });
    expect(off.posts).toHaveLength(0);
  });
  it("refuses a kind that touches the gas coin before asking", async () => {
    const tx = new Transaction();
    tx.setSender(USER);
    const [c] = tx.splitCoins(tx.gas, [1]);
    tx.transferObjects([c], A(0xdef));
    const d = deps(status({}), grant);
    expect(await sponsorForGame(tx, USER, "act", d)).toMatchObject({ ok: false, reason: "gas-coin" });
    expect(d.posts).toHaveLength(0);
  });
});

describe("signAndExecuteForGame: the review sheet says who pays", () => {
  const pending = () => useSignRequests.getState().requests.at(-1)!;
  const settle = (p: Promise<SignResult>) => {
    rejectRequest(pending().id);
    return p.catch(() => null);
  };
  it("the game's budget pays: the review gets the sponsored bytes and the game as payer", async () => {
    const p = signAndExecuteForGame(gameTx(), { slug: "act", name: "ACT" }, { account: USER }, undefined, deps(status({}), grant));
    await new Promise((r) => setTimeout(r, 50));
    expect(pending().sponsored).toMatchObject({ category: "game:act", payer: "ACT", sponsor: SPONSOR });
    expect(pending().tx).toBeInstanceOf(Uint8Array);
    await settle(p);
  });
  it("budget used up: the player pays and the review says why", async () => {
    const p = signAndExecuteForGame(gameTx(), { slug: "act", name: "ACT" }, { account: USER }, undefined, deps(status({}), () => err(429, "budget_exhausted")));
    await new Promise((r) => setTimeout(r, 50));
    expect(pending().sponsored).toBeUndefined();
    expect(pending().feeNote).toBe("ACT's gas budget is used up, so you pay this fee.");
    await settle(p);
  });
  it("refused: the fallback runs (Rare's trophy cover for claims) with no note", async () => {
    let fell: unknown = null;
    const r = await signAndExecuteForGame(gameTx(), { slug: "act", name: "ACT" }, { account: USER }, async (o) => ((fell = o), { bytes: "", signature: "" }), deps(status({}), () => err(403, "not_sponsored")));
    expect(r).toEqual({ bytes: "", signature: "" });
    expect(fell).toMatchObject({ account: USER, feeNote: undefined });
  });
  it("no slug: never asks", async () => {
    const d = deps(status({}), grant);
    await signAndExecuteForGame(gameTx(), { slug: null, name: "Some game" }, { account: USER }, async () => ({ bytes: "", signature: "" }), d);
    expect(d.gets).toHaveLength(0);
  });
});

describe("review rows", () => {
  const ins = { ok: true, feePaidBySigner: false, fee: { net: 0n }, send: [], receive: [], objectsOut: [], objectsIn: [] } as unknown as Insights;
  it("Network fee: Paid by <game> or Paid by Rare", () => {
    expect(describeReview(ins, {}, { coveredByRare: true, paidBy: "ACT" }).find((r) => r.kind === "fee")?.text).toBe("Paid by ACT");
    expect(describeReview(ins, {}, { coveredByRare: true }).find((r) => r.kind === "fee")?.text).toBe("Paid by Rare");
  });
});
