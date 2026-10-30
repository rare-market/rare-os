import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { Transaction } from "@mysten/sui/transactions";
import { toBase64 } from "@mysten/sui/utils";
import { verifyPersonalMessageSignature } from "@mysten/sui/verify";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SUI_TYPE, USDC_TYPE } from "./amount";
import type { Reply } from "./http";
import { apiKey, buildSwap, quoteSwap, routeVenues, swapTransaction, type KeyStore, type SwapDeps } from "./swap";

const OWNER = `0x${"a1".repeat(32)}`;

async function kindB64() {
  const t = new Transaction();
  t.moveCall({ target: "0x2::coin::zero", typeArguments: [SUI_TYPE] });
  return toBase64(await t.build({ onlyTransactionKind: true }));
}

function memKeys(start: string | null = null): KeyStore & { value: string | null } {
  const s = { value: start, get: () => s.value, set: (k: string | null) => void (s.value = k) };
  return s;
}

type Handler = (path: string, body: unknown, auth?: string) => Reply<unknown> | Promise<Reply<unknown>>;
function deps(handler: Handler, keys = memKeys()): SwapDeps & { calls: Array<{ path: string; body: unknown; auth?: string }> } {
  const calls: Array<{ path: string; body: unknown; auth?: string }> = [];
  const identity = new Ed25519Keypair();
  return {
    calls,
    keys,
    identity: () => identity,
    gas: async (tx) => tx,
    call: (async (_m: string, path: string, body?: unknown, auth?: string) => {
      calls.push({ path, body, auth });
      return handler(path, body, auth);
    }) as SwapDeps["call"],
  };
}

const ok = (data: unknown): Reply<unknown> => ({ status: 200, body: { version: "1", data } });

describe("apiKey", () => {
  let messages: string[];
  beforeEach(() => {
    messages = [];
  });

  it("signs the API's challenge with the device identity and keeps the key", async () => {
    const d = deps(async (path, body) => {
      if (path === "/auth/challenge") {
        const message = `Rare Launcher swap key for ${(body as { address: string }).address}`;
        messages.push(message);
        return ok({ challengeId: "c-1", message });
      }
      if (path === "/auth/keys") {
        const b = body as { address: string; signature: string; challengeId: string; scopes: string[] };
        // The signature must verify for the identity address over the exact challenge text.
        await verifyPersonalMessageSignature(new TextEncoder().encode(messages[0]), b.signature, { address: b.address });
        expect(b.challengeId).toBe("c-1");
        expect(b.scopes).toEqual(["quote", "build"]);
        return ok({ key: "yk_test" });
      }
      throw new Error(path);
    });
    expect(await apiKey(d)).toBe("yk_test");
    expect(d.keys.get()).toBe("yk_test");
    // Second call uses the stored key.
    expect(await apiKey(d)).toBe("yk_test");
    expect(d.calls.length).toBe(2);
  });

  it("surfaces the API's error message", async () => {
    const d = deps(() => ({ status: 429, body: { error: { code: "challenge_limit", message: "Wait before requesting another challenge." } } }));
    await expect(apiKey(d)).rejects.toThrow(/Too many requests/);
  });
});

describe("quoteSwap", () => {
  it("asks for the trader's wallet with long coin types and maps the answer", async () => {
    const d = deps(
      () =>
        ok({
          id: "mvpva0a1-lw",
          amountOut: "90459",
          feeBps: 20,
          hops: 2,
          route: { providers: ["AFTERMATH", "FLOWXV3"] },
          expiresAt: 123,
        }),
      memKeys("yk"),
    );
    const q = await quoteSwap({ coinIn: "0x2::sui::SUI", coinOut: USDC_TYPE, amountIn: 100_000_000n, owner: OWNER }, d);
    expect(d.calls[0]).toEqual({
      path: "/swap/quote",
      body: { coinIn: SUI_TYPE, coinOut: USDC_TYPE, amountIn: "100000000", owner: OWNER },
      auth: "yk",
    });
    expect(q).toMatchObject({ id: "mvpva0a1-lw", amountOut: 90_459n, amountIn: 100_000_000n, feeBps: 20, hops: 2, venues: ["aftermath", "flowxv3"], expiresAt: 123 });
  });

  it("replaces a key the API no longer knows, once", async () => {
    let n = 0;
    const keys = memKeys("old");
    const d = deps((path) => {
      if (path === "/swap/quote") return n++ === 0 ? { status: 401, body: { error: { message: "bad key" } } } : ok({ id: "q1", amountOut: "5" });
      if (path === "/auth/challenge") return ok({ challengeId: "c", message: "m" });
      return ok({ key: "new" });
    }, keys);
    const q = await quoteSwap({ coinIn: SUI_TYPE, coinOut: USDC_TYPE, amountIn: 1n, owner: OWNER }, d);
    expect(q.amountOut).toBe(5n);
    expect(keys.value).toBe("new");
    expect(d.calls.at(-1)?.auth).toBe("new");
  });

  it("refuses empty amounts and empty routes", async () => {
    const d = deps(() => ok({ id: "q", amountOut: "0" }), memKeys("k"));
    await expect(quoteSwap({ coinIn: SUI_TYPE, coinOut: USDC_TYPE, amountIn: 0n, owner: OWNER }, d)).rejects.toThrow(/amount/);
    await expect(quoteSwap({ coinIn: SUI_TYPE, coinOut: USDC_TYPE, amountIn: 1n, owner: OWNER }, d)).rejects.toThrow(/No route/);
  });
});

describe("buildSwap", () => {
  const quote = { id: "q-1", coinIn: SUI_TYPE, coinOut: USDC_TYPE, amountIn: 1n, amountOut: 2n, feeBps: 20, hops: 1, venues: [], expiresAt: 0 };

  it("turns the router's transaction kind into the trader's unsigned transaction", async () => {
    const kind = await kindB64();
    const d = deps(() => ok({ ok: true, kind, minOut: "89533", expiresAt: 99, simulation: { status: "succeeded", gasEstimateMist: "4723677" } }), memKeys("k"));
    const b = await buildSwap(quote, OWNER, 100, d);
    expect(d.calls[0].body).toEqual({ id: "q-1", owner: OWNER, slippageBps: 100, mode: "transaction" });
    const data = b.tx.getData();
    expect(data.sender).toBe(OWNER);
    expect(data.commands).toHaveLength(1);
    expect(data.commands[0].MoveCall?.function).toBe("zero");
    expect(b.minOut).toBe(89_533n);
    expect(b.gasEstimateMist).toBe(4_723_677n);
    expect(b.expiresAt).toBe(99);
  });

  it("stops when the router's dry run failed", async () => {
    const d = deps(async () => ok({ kind: await kindB64(), simulation: { status: "failed" } }), memKeys("k"));
    await expect(buildSwap(quote, OWNER, 100, d)).rejects.toThrow(/would fail/);
  });

  it("explains a declined build", async () => {
    const d = deps(() => ({ status: 422, body: { error: { code: "build_declined", message: "The swap could not pass its dry run. Request a new quote." } } }), memKeys("k"));
    await expect(buildSwap(quote, OWNER, 100, d)).rejects.toThrow(/dry run/);
  });
});

describe("helpers", () => {
  it("reads venues from both route shapes", () => {
    expect(routeVenues({ paths: [{ hops: [{ venue: "cetus" }, { venue: "turbos" }] }, { hops: [{ venue: "cetus" }] }] })).toEqual(["cetus", "turbos"]);
    expect(routeVenues(null)).toEqual([]);
  });

  it("sets the sender on a bare kind", async () => {
    const tx = swapTransaction(await kindB64(), OWNER);
    expect(tx.getData().sender).toBe(OWNER);
    expect(tx.getData().gasData.payment).toBeNull();
  });
});

vi.mock("../lib/bridge", () => ({ isTauri: false }));

import { venueName } from "./swap";
describe("venueName", () => {
  it("names exchanges plainly", () => {
    expect(venueName("cetus_tide")).toBe("Cetus");
    expect(venueName("FLOWXV3")).toBe("FlowX");
    expect(venueName("deepbookv3")).toBe("DeepBook");
    expect(venueName("newdex")).toBe("Newdex");
    expect(venueName("cetusdlmm")).toBe("Cetus DLMM");
  });
});
