import { Transaction } from "@mysten/sui/transactions";
import { toBase64 } from "@mysten/sui/utils";
import { describe, expect, it } from "vitest";
import { SignRejected } from "../../sign";
import { errorText, mapGameRequest, normOrigin, replyValue, silentConnectAccount } from "./webgame";

describe("web game requests", () => {
  it("maps connect", () => {
    expect(mapGameRequest({ method: "connect", params: {} })).toEqual({ ok: true, call: { kind: "connect" } });
  });

  it("maps a transaction JSON to a Transaction and keeps the execute flag", () => {
    const tx = new Transaction();
    tx.setSender("0x" + "1".repeat(64));
    const json = tx.serialize();
    const m = mapGameRequest({ method: "signAndExecuteTransaction", params: { transaction: json } });
    expect(m.ok).toBe(true);
    if (m.ok && m.call.kind === "transaction") {
      expect(m.call.execute).toBe(true);
      expect(m.call.tx).toBeInstanceOf(Transaction);
    }
    const s = mapGameRequest({ method: "signTransaction", params: { transaction: json } });
    expect(s.ok && s.call.kind === "transaction" && s.call.execute).toBe(false);
  });

  it("keeps built base64 bytes as bytes", () => {
    const bytes = new Uint8Array([1, 2, 3]);
    const m = mapGameRequest({ method: "signTransaction", params: { transaction: toBase64(bytes) } });
    expect(m.ok && m.call.kind === "transaction" && m.call.tx instanceof Uint8Array).toBe(true);
  });

  it("maps a personal message", () => {
    const m = mapGameRequest({ method: "signPersonalMessage", params: { message: toBase64(new TextEncoder().encode("hi")) } });
    expect(m.ok && m.call.kind === "message" && new TextDecoder().decode(m.call.message)).toBe("hi");
  });

  it("rejects missing params and bad JSON", () => {
    expect(mapGameRequest({ method: "signTransaction", params: {} }).ok).toBe(false);
    expect(mapGameRequest({ method: "signPersonalMessage", params: {} }).ok).toBe(false);
    expect(mapGameRequest({ method: "signTransaction", params: { transaction: "{not json" } }).ok).toBe(false);
  });

  it("builds replies", () => {
    const r = { bytes: "AA==", signature: "sig", digest: "D1" };
    expect(replyValue({ kind: "message", message: new Uint8Array() }, r)).toEqual({ bytes: "AA==", signature: "sig" });
    expect(replyValue({ kind: "transaction", tx: new Uint8Array(), execute: false }, r)).toEqual({ bytes: "AA==", signature: "sig" });
    expect(replyValue({ kind: "transaction", tx: new Uint8Array(), execute: true }, r)).toMatchObject({ digest: "D1" });
  });

  it("words errors for the game", () => {
    expect(errorText(new SignRejected())).toBe("User rejected the request");
    expect(errorText(new Error("boom"))).toBe("boom");
    expect(normOrigin("https://Game.example/")).toBe("https://game.example");
  });
});

import { decodeMessage, siteRisks } from "./site";

describe("message and site checks", () => {
  it("decodes text and falls back to hex", () => {
    expect(decodeMessage(new TextEncoder().encode("Sign in to Game")).text).toBe("Sign in to Game");
    const bin = decodeMessage(new Uint8Array([0xff, 0xfe, 0x01]));
    expect(bin.text).toBeNull();
    expect(bin.hex).toBe("fffe01");
  });
  it("flags http and look-alike hosts", () => {
    expect(siteRisks("https://game.example").length).toBe(0);
    expect(siteRisks("http://game.example")[0].danger).toBe(true);
    expect(siteRisks("https://xn--gme-6na.example")[0].id).toBe("site-lookalike");
  });
});

import { waitText, wrongMessage } from "./unlockCopy";

describe("unlock copy", () => {
  it("counts tries before a wait", () => {
    expect(wrongMessage(1)).toBe("Wrong PIN. 4 tries before a wait.");
    expect(wrongMessage(4)).toBe("Wrong PIN. 1 try before a wait.");
  });
  it("words the wait", () => {
    expect(waitText(30_000)).toBe("30 s");
    expect(waitText(120_000)).toBe("2 min");
  });
});

describe("silent connect for approved sites", () => {
  const a = { address: "0xa" };
  const b = { address: "0xb" };
  it("answers an approved origin with the selected account, no sheet", () => {
    expect(silentConnectAccount(["https://play.example.com"], "https://play.example.com/", [a, b], b)).toBe(b);
    expect(silentConnectAccount(["http://localhost:3000"], "HTTP://localhost:3000", [a, b], null)).toBe(a);
  });
  it("needs the sheet for a new origin or when there is no account", () => {
    expect(silentConnectAccount(["https://play.example.com"], "https://other.example.com", [a], a)).toBeNull();
    expect(silentConnectAccount(["https://play.example.com"], "https://play.example.com", [], null)).toBeNull();
    expect(silentConnectAccount([], "https://play.example.com", [a], a)).toBeNull();
  });
  it("falls back to the first account when the selected one is gone", () => {
    expect(silentConnectAccount(["https://g.example"], "https://g.example", [a, b], { address: "0xc" })).toBe(a);
  });
});
