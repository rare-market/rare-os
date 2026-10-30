import { normalizeSuiAddress, toBase64 } from "@mysten/sui/utils";
import { describe, expect, it, vi } from "vitest";
import { attestRequest, buildClaimTx, claimSteam, parseAttestation, type Attestation, type ClaimEnv } from "./attest";
import type { ChainGame, SteamGame } from "./types";

const PKG = normalizeSuiAddress("0xabc");
const REG = normalizeSuiAddress("0x111");
const GAME = normalizeSuiAddress("0x222");
const PROFILE = normalizeSuiAddress("0x333");
const PLAYER = normalizeSuiAddress("0x444");
const SIG = new Uint8Array(64).fill(9);
const NOW = 1_800_000_000_000;

const steam: SteamGame = {
  appid: 480,
  name: "Spacewar",
  steamId: "76561197972611406",
  total: 3,
  unlocked: 2,
  lastUnlock: 1_700_000_000,
  source: "stats",
  achievements: [
    { apiName: "ACH_WIN_ONE_GAME", name: "Winner", description: "", iconUrl: null, iconGrayUrl: null, hidden: false, unlocked: true, unlockTime: 1_700_000_000, globalPercent: null },
    { apiName: "ACH_TRAVEL_FAR", name: "Far", description: "", iconUrl: null, iconGrayUrl: null, hidden: false, unlocked: true, unlockTime: null, globalPercent: null },
    { apiName: "ACH_WIN_100", name: "Champion", description: "", iconUrl: null, iconGrayUrl: null, hidden: false, unlocked: false, unlockTime: null, globalPercent: null },
  ],
};

const def = (trophyId: number, key: string, tier: 0 | 1 | 2 | 3 = 0) => ({ trophyId, key, name: key, description: "", imageUrl: "", tier, points: 15, hidden: false, inPlatinumSet: true, unlocked: 1 });
const chainGame: ChainGame = { id: GAME, slug: "steam:480", name: "Spacewar", attested: true, paused: false, players: 10, platinum: 3, defs: [def(0, "ACH_WIN_ONE_GAME"), def(1, "ACH_TRAVEL_FAR", 1), def(2, "ACH_WIN_100", 2), def(3, "PLATINUM", 3)] };

const reply = (trophyId: number, over: Record<string, unknown> = {}) => ({
  status: 200,
  body: { registry: REG, game: GAME, trophyId: String(trophyId), nonce: String(77 + trophyId), expiresMs: String(NOW + 600_000), signature: toBase64(SIG), ...over },
});

describe("attestRequest", () => {
  it("is the JSON the attestation service expects", () => {
    expect(attestRequest("0x444", 480, steam.steamId, steam.achievements[0])).toEqual({
      player: PLAYER,
      game: "steam:480",
      achievement: "ACH_WIN_ONE_GAME",
      evidence: { unlockTime: 1_700_000_000, steamId: "76561197972611406" },
    });
  });
});

describe("parseAttestation", () => {
  const expect_ = { registry: REG, game: GAME, trophyId: 1, nowMs: NOW };
  it("reads decimal strings and the base64 signature", () => {
    const a = parseAttestation(reply(1), expect_);
    expect(a.trophyId).toBe(1n);
    expect(a.nonce).toBe(78n);
    expect(a.expiresMs).toBe(BigInt(NOW + 600_000));
    expect(a.signature).toEqual(SIG);
  });
  it("rejects anything that does not match this claim", () => {
    expect(() => parseAttestation(reply(2), expect_)).toThrow(/different trophy/);
    expect(() => parseAttestation(reply(1, { registry: "0x999" }), expect_)).toThrow(/different registry/);
    expect(() => parseAttestation(reply(1, { game: "0x999" }), expect_)).toThrow(/different game/);
    expect(() => parseAttestation(reply(1, { expiresMs: String(NOW - 1) }), expect_)).toThrow(/expired/);
    expect(() => parseAttestation(reply(1, { signature: toBase64(new Uint8Array(10)) }), expect_)).toThrow(/signature/);
    expect(() => parseAttestation(reply(1, { nonce: "-1" }), expect_)).toThrow(/nonce/);
  });
  it("passes the service's error sentence through", () => {
    expect(() => parseAttestation({ status: 403, body: { error: "Achievement not found for this Steam account" } }, expect_)).toThrow("Achievement not found for this Steam account");
    expect(() => parseAttestation({ status: 500, body: null }, expect_)).toThrow(/500/);
  });
});

type Cmd = { $kind: string; MoveCall?: { package: string; module: string; function: string; arguments: Array<{ $kind: string; Input?: number; Result?: number; NestedResult?: [number, number] }> } };
const calls = (tx: ReturnType<typeof buildClaimTx>) => (tx.getData().commands as Cmd[]).map((c) => `${c.MoveCall!.module}::${c.MoveCall!.function}`);
const att = (id: number): Attestation => ({ registry: REG, game: GAME, trophyId: BigInt(id), nonce: BigInt(100 + id), expiresMs: BigInt(NOW + 1), signature: SIG });

describe("buildClaimTx", () => {
  it("creates and shares the profile around the mints on a first claim", () => {
    const tx = buildClaimTx({ pkg: PKG, registry: REG, game: GAME, player: PLAYER, profileId: null, attestations: [att(0), att(1)] });
    expect(calls(tx)).toEqual(["profile::create", "game::mint_attested", "game::mint_attested", "profile::share"]);
    const cmds = tx.getData().commands as Cmd[];
    expect(cmds.every((c) => c.MoveCall!.package === PKG)).toBe(true);
    // mint_attested(reg, game, profile, trophy_id, nonce, expires_ms, signature, clock): the profile is create's result
    const mint = cmds[1].MoveCall!.arguments;
    expect(mint).toHaveLength(8);
    expect(mint[2]).toMatchObject({ $kind: "Result", Result: 0 });
    expect(cmds[3].MoveCall!.arguments[0]).toMatchObject({ $kind: "Result", Result: 0 });
    // inputs: registry, game, player address, then per mint: id, nonce, expiry, signature, clock
    const inputs = tx.getData().inputs as Array<{ $kind: string; UnresolvedObject?: { objectId: string }; Object?: { SharedObject?: { objectId: string } } }>;
    const objs = inputs.flatMap((i) => (i.UnresolvedObject ? [i.UnresolvedObject.objectId] : i.Object?.SharedObject ? [i.Object.SharedObject.objectId] : [])).map((id) => normalizeSuiAddress(id));
    expect(objs).toEqual([REG, GAME, normalizeSuiAddress("0x6")]);
  });

  it("passes the shared profile straight in when the player has one", () => {
    const tx = buildClaimTx({ pkg: PKG, registry: REG, game: GAME, player: PLAYER, profileId: PROFILE, attestations: [att(0)] });
    expect(calls(tx)).toEqual(["game::mint_attested"]);
    const inputs = tx.getData().inputs as Array<{ $kind: string; UnresolvedObject?: { objectId: string } }>;
    expect(inputs.filter((i) => i.$kind === "UnresolvedObject").map((i) => normalizeSuiAddress(i.UnresolvedObject!.objectId))).toContain(PROFILE);
    expect(tx.getData().sender).toBe(PLAYER);
  });

  it("refuses an empty claim", () => {
    expect(() => buildClaimTx({ pkg: PKG, registry: REG, game: GAME, player: PLAYER, profileId: null, attestations: [] })).toThrow();
  });
});

describe("claimSteam", () => {
  const env = (over: Partial<ClaimEnv> = {}): ClaimEnv & { attest: ReturnType<typeof vi.fn>; submit: ReturnType<typeof vi.fn> } =>
    ({
      config: { network: "testnet", package: PKG, registry: REG },
      player: PLAYER,
      recheck: vi.fn(async () => steam),
      attest: vi.fn(async (b: { achievement: string }) => reply(chainGame.defs.find((d) => d.key === b.achievement)!.trophyId)),
      submit: vi.fn(async () => ({ digest: "D1" })),
      now: () => NOW,
      ...over,
    }) as never;

  it("re-checks the files, attests each unlock and submits one transaction", async () => {
    const e = env();
    const r = await claimSteam(e, 480, ["ACH_WIN_ONE_GAME", "ACH_TRAVEL_FAR"], chainGame, null);
    expect(e.recheck).toHaveBeenCalledWith(480);
    expect(e.attest).toHaveBeenCalledTimes(2);
    expect(e.attest.mock.calls[1][0]).toEqual({ player: PLAYER, game: "steam:480", achievement: "ACH_TRAVEL_FAR", evidence: { unlockTime: null, steamId: steam.steamId } });
    expect(e.submit).toHaveBeenCalledTimes(1);
    const [tx, title, info] = e.submit.mock.calls[0];
    expect(calls(tx)).toEqual(["profile::create", "game::mint_attested", "game::mint_attested", "profile::share"]);
    expect(title).toBe("Claim 2 trophies");
    expect(info).toEqual({ game: GAME, trophyIds: [0, 1] });
    expect(r).toMatchObject({ digest: "D1", skipped: [] });
    expect(r.claimed.map((d) => d.key)).toEqual(["ACH_WIN_ONE_GAME", "ACH_TRAVEL_FAR"]);
  });

  it("skips what is no longer unlocked on disk, the platinum, and unknown keys", async () => {
    const e = env();
    const r = await claimSteam(e, 480, ["ACH_WIN_100", "PLATINUM", "ACH_NOPE", "ACH_WIN_ONE_GAME"], chainGame, PROFILE);
    expect(e.attest).toHaveBeenCalledTimes(1);
    expect(r.skipped.map((s) => s.apiName)).toEqual(["ACH_WIN_100", "PLATINUM", "ACH_NOPE"]);
    expect(calls(e.submit.mock.calls[0][0])).toEqual(["game::mint_attested"]);
  });

  it("sends nothing when the attestation is refused", async () => {
    const e = env({ attest: vi.fn(async () => ({ status: 403, body: { error: "Not unlocked on Steam" } })) });
    await expect(claimSteam(e, 480, ["ACH_WIN_ONE_GAME"], chainGame, null)).rejects.toThrow("Not unlocked on Steam");
    expect(e.submit).not.toHaveBeenCalled();
  });

  it("stops before any request for a paused or studio-only game", async () => {
    const e = env();
    await expect(claimSteam(e, 480, ["ACH_WIN_ONE_GAME"], { ...chainGame, paused: true }, null)).rejects.toThrow(/paused/);
    await expect(claimSteam(e, 480, ["ACH_WIN_ONE_GAME"], { ...chainGame, attested: false }, null)).rejects.toThrow(/self-reported/);
    expect(e.recheck).not.toHaveBeenCalled();
  });

  it("fails cleanly when nothing is left to claim", async () => {
    const e = env({ recheck: vi.fn(async () => ({ ...steam, achievements: steam.achievements.map((a) => ({ ...a, unlocked: false })) })) });
    await expect(claimSteam(e, 480, ["ACH_WIN_ONE_GAME"], chainGame, null)).rejects.toThrow(/No longer unlocked/);
    expect(e.attest).not.toHaveBeenCalled();
  });
});
