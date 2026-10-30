import { describe, expect, it } from "vitest";
import { verifyPersonalMessageSignature } from "@mysten/sui/verify";
import { bodyHash, deviceKey, linkRequest, requestText, streamText } from "./sign";

const ADDR = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const enc = new TextEncoder();

describe("friends signatures", () => {
  it("signs each request over method, path, address, key, time, nonce and body hash", async () => {
    const k = deviceKey(ADDR);
    const body = JSON.stringify({ address: "0x2" });
    const h = await k.headers("POST", "/v1/friends/request", body, 1_800_000_000_000);
    expect(h["x-rare-address"]).toBe(ADDR);
    expect(h["x-rare-key"]).toBe(k.key);
    expect(h["x-rare-nonce"]).toMatch(/^[A-Za-z0-9_-]{12,64}$/);
    const text = requestText({ method: "POST", path: "/v1/friends/request", address: ADDR, key: k.key, ts: 1_800_000_000_000, nonce: h["x-rare-nonce"], bodyHash: bodyHash(body) });
    // The server's check: the device key's own address signed exactly this text.
    await expect(verifyPersonalMessageSignature(enc.encode(text), h["x-rare-signature"], { address: k.key })).resolves.toBeTruthy();
    await expect(verifyPersonalMessageSignature(enc.encode(text.replace("request", "block")), h["x-rare-signature"], { address: k.key })).rejects.toThrow();
  });

  it("uses a fresh nonce every time", async () => {
    const k = deviceKey(ADDR);
    const a = await k.headers("GET", "/v1/friends", "");
    const b = await k.headers("GET", "/v1/friends", "");
    expect(a["x-rare-nonce"]).not.toBe(b["x-rare-nonce"]);
  });

  it("hashes the body as the server does (sha256 hex of the raw JSON)", () => {
    expect(bodyHash("")).toBe("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
  });

  it("builds a stream token the server can read", async () => {
    const k = deviceKey(ADDR);
    const t = await k.streamToken(1_800_000_000_000);
    const j = JSON.parse(atob(t.replace(/-/g, "+").replace(/_/g, "/")));
    expect(j).toMatchObject({ a: ADDR, k: k.key, t: 1_800_000_000_000 });
    await expect(verifyPersonalMessageSignature(enc.encode(streamText({ address: ADDR, key: k.key, ts: j.t })), j.s, { address: k.key })).resolves.toBeTruthy();
  });

  it("asks the wallet to sign the exact link text the server rebuilds", () => {
    const k = deviceKey(ADDR);
    const r = linkRequest(k, Date.UTC(2026, 9, 26));
    expect(r.message).toBe(
      [
        "Rare friends",
        "",
        "Let this device handle your friends, parties and presence in Rare.",
        "",
        `Address: ${ADDR}`,
        `Device key: ${k.key}`,
        "Issued: 2026-10-26T00:00:00.000Z",
        "Expires: 2027-01-24T00:00:00.000Z",
        "",
        "This does not move funds, send a transaction or cost gas.",
      ].join("\n"),
    );
    expect(k.key).not.toBe(ADDR);
  });
});
