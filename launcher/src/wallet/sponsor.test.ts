import { Transaction } from "@mysten/sui/transactions";
import { toBase58, toBase64 } from "@mysten/sui/utils";
import { describe, expect, it, vi } from "vitest";
import {
  clearSponsorStatus,
  executeSponsored,
  needsSponsor,
  runSponsored,
  sponsorOffer,
  sponsorTransaction,
  usesGasCoin,
  type SponsorDeps,
  type SponsorStatus,
} from "./sponsor";
import { describeReview } from "./review/describe";
import type { Insights } from "./review/analyze";

const A = (n: number) => `0x${n.toString(16).padStart(64, "0")}`;
const USER = A(0xabc);
const SPONSOR = A(0x5905);
const ref = (n: number) => ({ objectId: A(n), version: "7", digest: toBase58(new Uint8Array(32).fill(n % 250)) });

/** A transfer with every input resolved, so it builds without a node. */
function transfer(): Transaction {
  const tx = new Transaction();
  tx.setSender(USER);
  tx.transferObjects([tx.objectRef(ref(0x77))], tx.pure.address(A(0xdef)));
  return tx;
}

/** What the server answers: the kind wrapped in full TransactionData with the sponsor as gas owner. */
async function sponsoredBytes(kindB64: string, o: { owner?: string; kind?: string } = {}): Promise<string> {
  const tx = Transaction.fromKind(o.kind ?? kindB64);
  tx.setSender(USER);
  tx.setGasOwner(o.owner ?? SPONSOR);
  tx.setGasPayment([ref(0x99)]);
  tx.setGasPrice(1000);
  tx.setGasBudget(5_000_000);
  return toBase64(await tx.build());
}

const STATUS: SponsorStatus = {
  enabled: true,
  funded: true,
  address: SPONSOR,
  balanceMist: "5000000000",
  dailyBudgetMist: "1000000000",
  spentTodayMist: "0",
  remainingTodayMist: "1000000000",
  categories: { rescue: true, trophy: true, trade: true, walrus: false },
  maxUserSuiMist: "50000000",
  you: { sponsored: 0, gasMist: "0", tradeUsed: false, eligible: { rescue: true, trophy: true, trade: true, walrus: false } },
};

type Handler = (path: string, body: Record<string, string>) => Promise<{ status: number; body: unknown }> | { status: number; body: unknown };

function deps(post: Handler, status: SponsorStatus | null = STATUS): SponsorDeps & { posts: Array<{ path: string; body: Record<string, string> }> } {
  const posts: Array<{ path: string; body: Record<string, string> }> = [];
  let t = 1_000_000;
  return {
    posts,
    get: async <T,>() => {
      if (!status) throw new Error("rare.market 503");
      return { data: status } as T;
    },
    post: async <T,>(path: string, body: unknown) => {
      posts.push({ path, body: body as Record<string, string> });
      return (await post(path, body as Record<string, string>)) as { status: number; body: T };
    },
    client: undefined,
    now: () => (t += 1000),
  };
}

const ok = (data: unknown) => ({ status: 200, body: { data } });
const err = (status: number, code: string, message: string) => ({ status, body: { error: { code, message } } });

async function grant(body: Record<string, string>, over: Record<string, unknown> = {}) {
  return ok({
    bytes: await sponsoredBytes(body.kind),
    sponsorSignature: "c3BvbnNvcg==",
    digest: `d${Math.random()}`,
    sponsor: SPONSOR,
    category: body.category,
    gasBudget: "5000000",
    estimatedGasMist: "1200000",
    expiresAt: Date.now() + 120_000,
    ...over,
  });
}

describe("needsSponsor", () => {
  it("uses the estimate plus a margin, 0.01 SUI without one", () => {
    expect(needsSponsor(0n)).toBe(true);
    expect(needsSponsor(14_000_000n)).toBe(true); // 0.01 + 0.005 margin
    expect(needsSponsor(16_000_000n)).toBe(false);
    expect(needsSponsor(3_000_000n, 1_000_000n)).toBe(true); // 1M + 5M floor
    expect(needsSponsor(7_000_000n, 1_000_000n)).toBe(false);
    expect(needsSponsor(100_000_000n, 80_000_000n)).toBe(true); // 80M + 40M
  });
});

describe("sponsorOffer", () => {
  it("offers only when SUI is short and the category is eligible", async () => {
    clearSponsorStatus();
    expect(await sponsorOffer(USER, "trade", { suiBalanceMist: 0n }, deps(() => ok({})))).not.toBeNull();
    clearSponsorStatus();
    expect(await sponsorOffer(USER, "trade", { suiBalanceMist: 1_000_000_000n }, deps(() => ok({})))).toBeNull();
    clearSponsorStatus();
    expect(await sponsorOffer(USER, "walrus", { suiBalanceMist: 0n }, deps(() => ok({})))).toBeNull();
    clearSponsorStatus();
    const used = { ...STATUS, you: { ...STATUS.you!, tradeUsed: true } };
    expect(await sponsorOffer(USER, "trade", { suiBalanceMist: 0n }, deps(() => ok({}), used))).toBeNull();
  });

  it("not funded or unreachable: no offer, no throw", async () => {
    clearSponsorStatus();
    expect(await sponsorOffer(USER, "trade", { suiBalanceMist: 0n }, deps(() => ok({}), { ...STATUS, funded: false }))).toBeNull();
    clearSponsorStatus();
    expect(await sponsorOffer(USER, "trade", { suiBalanceMist: 0n }, deps(() => ok({}), null))).toBeNull();
  });
});

describe("sponsorTransaction", () => {
  it("success: sends the kind and checks the answer", async () => {
    const d = deps((_p, b) => grant(b));
    const r = await sponsorTransaction(transfer(), USER, "trade", d);
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.sponsor).toBe(SPONSOR);
    expect(r.gasBudget).toBe(5_000_000n);
    expect(d.posts).toHaveLength(1);
    expect(d.posts[0].path).toBe("/v1/sponsor");
    expect(d.posts[0].body.sender).toBe(USER);
    expect(d.posts[0].body.category).toBe("trade");
    expect(usesGasCoin(Transaction.fromKind(d.posts[0].body.kind).getData().commands)).toBe(false);
  });

  it("503 not funded: unavailable, for the caller to fall back", async () => {
    const r = await sponsorTransaction(transfer(), USER, "trade", deps(() => err(503, "sponsor_unavailable", "sponsor unavailable: not funded")));
    expect(r).toMatchObject({ ok: false, reason: "unavailable", code: "sponsor_unavailable" });
  });

  it("403 refused: falls back with the server's reason", async () => {
    const r = await sponsorTransaction(transfer(), USER, "trade", deps(() => err(403, "not_sponsored", "first trade already used")));
    expect(r).toMatchObject({ ok: false, reason: "refused", message: "first trade already used" });
  });

  it("busy, rate limited and network errors are results, not throws", async () => {
    expect(await sponsorTransaction(transfer(), USER, "trade", deps(() => err(503, "sponsor_busy", "busy")))).toMatchObject({ ok: false, reason: "busy" });
    expect(await sponsorTransaction(transfer(), USER, "trade", deps(() => err(429, "limit_reached", "limit")))).toMatchObject({ ok: false, reason: "limited" });
    const down = deps(() => {
      throw new Error("offline");
    });
    expect(await sponsorTransaction(transfer(), USER, "trade", down)).toMatchObject({ ok: false, reason: "network" });
  });

  it("refuses an answer whose gas owner or transaction differ", async () => {
    const wrongOwner = await sponsorTransaction(transfer(), USER, "trade", deps(async (_p, b) => ok({ ...((await grant(b)).body.data as object), bytes: await sponsoredBytes(b.kind, { owner: A(0xbad) }) })));
    expect(wrongOwner).toMatchObject({ ok: false, reason: "mismatch" });
    const other = new Transaction();
    other.transferObjects([other.objectRef(ref(0x78))], other.pure.address(A(0xbad)));
    const otherKind = toBase64(await other.build({ onlyTransactionKind: true }));
    const swapped = await sponsorTransaction(transfer(), USER, "trade", deps(async (_p, b) => ok({ ...((await grant(b)).body.data as object), bytes: await sponsoredBytes(b.kind, { kind: otherKind }) })));
    expect(swapped).toMatchObject({ ok: false, reason: "mismatch" });
  });

  it("gas-coin kinds are never sent", async () => {
    const d = deps((_p, b) => grant(b));
    const split = new Transaction();
    split.setSender(USER);
    const [c] = split.splitCoins(split.gas, [1000]);
    split.transferObjects([c], A(0xdef));
    expect(await sponsorTransaction(split, USER, "trade", d)).toMatchObject({ ok: false, reason: "gas-coin" });

    const sendGas = new Transaction();
    sendGas.setSender(USER);
    sendGas.transferObjects([sendGas.gas], A(0xdef));
    expect(await sponsorTransaction(sendGas, USER, "rescue", d)).toMatchObject({ ok: false, reason: "gas-coin" });

    const coin = new Transaction();
    coin.setSender(USER);
    coin.transferObjects([coin.coin({ balance: 1000 })], A(0xdef)); // SUI falls back to the gas coin
    expect(await sponsorTransaction(coin, USER, "trade", d)).toMatchObject({ ok: false, reason: "gas-coin" });

    expect(d.posts).toHaveLength(0);
  });
});

describe("executeSponsored", () => {
  it("passes the digest and signature, maps failures", async () => {
    const d = deps(() => ok({ digest: "D1", ok: true, gasUsedMist: "1500000" }));
    expect(await executeSponsored("D1", "sig", d)).toEqual({ ok: true, digest: "D1", gasUsedMist: 1_500_000n });
    expect(d.posts[0]).toEqual({ path: "/v1/sponsor/execute", body: { digest: "D1", signature: "sig" } });
    expect(await executeSponsored("D1", "sig", deps(() => err(410, "sponsorship_expired", "expired")))).toMatchObject({ ok: false, reason: "expired" });
    expect(await executeSponsored("D1", "sig", deps(() => err(409, "already_executed", "done")))).toMatchObject({ ok: false, reason: "already-executed" });
    expect(await executeSponsored("D1", "sig", deps(() => ok({ digest: "D1", ok: false, error: "MoveAbort", gasUsedMist: "1" })))).toMatchObject({ ok: false, reason: "failed", message: "MoveAbort" });
  });
});

describe("runSponsored", () => {
  it("sponsors, signs the exact bytes, executes", async () => {
    let granted = "";
    const d = deps(async (p, b) => {
      if (p === "/v1/sponsor") {
        const g = await grant(b);
        granted = (g.body.data as { bytes: string }).bytes;
        return g;
      }
      return ok({ digest: b.digest, ok: true, gasUsedMist: "100" });
    });
    const sign = vi.fn(async (bytes: Uint8Array) => {
      expect(toBase64(bytes)).toBe(granted);
      return "user-sig";
    });
    const r = await runSponsored({ tx: transfer(), sender: USER, category: "trade", sign }, d);
    expect(r).toMatchObject({ ok: true, signature: "user-sig", sponsor: SPONSOR });
    expect(sign).toHaveBeenCalledTimes(1);
  });

  it("expired at execute: asks again once", async () => {
    let executes = 0;
    const d = deps(async (p, b) => {
      if (p === "/v1/sponsor") return grant(b);
      return ++executes === 1 ? err(410, "sponsorship_expired", "expired") : ok({ digest: b.digest, ok: true, gasUsedMist: "100" });
    });
    const sign = vi.fn(async () => "sig");
    const r = await runSponsored({ tx: transfer(), sender: USER, category: "trade", sign }, d);
    expect(r.ok).toBe(true);
    expect(d.posts.filter((x) => x.path === "/v1/sponsor")).toHaveLength(2);
    expect(sign).toHaveBeenCalledTimes(2);
  });

  it("expired twice: gives up with a typed result", async () => {
    const d = deps(async (p, b) => (p === "/v1/sponsor" ? grant(b) : err(410, "sponsorship_expired", "expired")));
    const r = await runSponsored({ tx: transfer(), sender: USER, category: "trade", sign: async () => "sig" }, d);
    expect(r).toMatchObject({ ok: false, reason: "expired" });
    expect(d.posts.filter((x) => x.path === "/v1/sponsor")).toHaveLength(2);
  });

  it("a sponsorship already past its time is replaced before signing", async () => {
    const d = deps(async (p, b) => (p === "/v1/sponsor" ? grant(b) : ok({ digest: b.digest, ok: true, gasUsedMist: "1" })));
    const first = await sponsorTransaction(transfer(), USER, "trade", d);
    if (!first.ok) throw new Error("expected a grant");
    const r = await runSponsored({ tx: transfer(), sender: USER, category: "trade", sign: async () => "sig", first: { ...first, expiresAt: 0 } }, d);
    expect(r.ok).toBe(true);
    expect(d.posts.filter((x) => x.path === "/v1/sponsor")).toHaveLength(2);
  });

  it("not funded: nothing is signed", async () => {
    const sign = vi.fn(async () => "sig");
    const r = await runSponsored({ tx: transfer(), sender: USER, category: "rescue", sign }, deps(() => err(503, "sponsor_unavailable", "sponsor unavailable: not funded")));
    expect(r).toMatchObject({ ok: false, reason: "unavailable" });
    expect(sign).not.toHaveBeenCalled();
  });
});

describe("review fee line", () => {
  const ins = { send: [], receive: [], objectsOut: [], objectsIn: [], fee: { computation: 1n, storage: 1n, rebate: 0n, net: 2n }, feePaidBySigner: false } as unknown as Insights;
  it("says Paid by Rare when Rare's sponsor pays", () => {
    expect(describeReview(ins, {}, { coveredByRare: true }).find((r) => r.kind === "fee")).toMatchObject({ text: "Paid by Rare" });
    expect(describeReview(ins, {}).find((r) => r.kind === "fee")).toMatchObject({ sub: "Paid by someone else" });
  });
});
