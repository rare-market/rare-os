// The launcher's own friends client against the deployed API, with throwaway keys:
//   LIVE=1 npx vitest run src/friends/api.live.test.ts
// Two fresh wallets link device keys (the wallet signature Rare Wallet would make), then the
// launcher's api.ts sends a request, accepts it, beats presence and chats, and cleans up.

import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { afterAll, describe, expect, it } from "vitest";

const mem = new Map<string, string>();
(globalThis as { localStorage?: unknown }).localStorage = {
  getItem: (k: string) => mem.get(k) ?? null,
  setItem: (k: string, v: string) => void mem.set(k, String(v)),
  removeItem: (k: string) => void mem.delete(k),
  clear: () => mem.clear(),
};

describe.skipIf(!process.env.LIVE)("rare.market friends API from the launcher client", async () => {
  const api = await import("./api");
  const { deviceKey, linkRequest, markLinked } = await import("./sign");
  const wa = Ed25519Keypair.generate();
  const wb = Ed25519Keypair.generate();
  const link = async (w: Ed25519Keypair) => {
    const k = deviceKey(w.toSuiAddress());
    const { message } = linkRequest(k);
    const { signature } = await w.signPersonalMessage(new TextEncoder().encode(message));
    const r = await fetch("https://rare.market/v1/friends/link", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ address: k.address, message, signature }) });
    expect(r.status).toBe(200);
    markLinked(k, (await r.json()).expiresAt);
    return k;
  };
  const ka = await link(wa);
  const kb = await link(wb);

  afterAll(async () => {
    await api.graph(ka, "remove", kb.address).catch(() => null);
    for (const k of [ka, kb]) {
      for (const p of (await api.overview(k).catch(() => null))?.parties ?? []) await api.leaveParty(k, p.id).catch(() => null);
      await api.heartbeat(k, { state: "offline", game: null }).catch(() => null);
      await api.unlink(k).catch(() => null);
    }
  });

  it("requests, accepts, sees presence, parties and chats", async () => {
    await api.graph(ka, "request", kb.address);
    expect((await api.overview(kb)).received.map((x) => x.address)).toContain(ka.address);
    await api.graph(kb, "accept", ka.address);
    await api.heartbeat(ka, { state: "online", game: { id: "web:act", title: "ACT", kind: "web", url: "https://play.actuniverse.com/" } });
    const seen = (await api.overview(kb)).friends.find((f) => f.address === ka.address);
    expect(seen?.presence.game?.id).toBe("web:act");
    const p = await api.createParty(ka, null, [kb.address]);
    await api.joinParty(kb, p.id);
    await api.postMessage(kb, p.id, "hi from the launcher");
    expect((await api.messages(ka, p.id)).at(-1)?.text).toBe("hi from the launcher");
    const code = await api.makeCode(ka);
    expect(code.url).toContain(code.code);
  }, 60_000);
});
