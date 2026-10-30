// Claiming a Steam achievement as a Rare trophy, device side.
//
// THE HONEST LIMIT: the evidence here is the Steam client's local files (appcache/stats and the
// library cache, read by src-tauri/src/trophies/steam.rs). Anyone can edit those files, and the
// Rare attestation service has no way to tell an edited file from a real unlock. So every trophy
// claimed this way is minted with source 1 and Move's source_name "Self-reported", the UI labels
// it "Self-reported" everywhere, and the profile shows what share of the score is self-reported.
// Only trophies a game's studio mints with its GameCap (source 0) are labelled "Studio issued".
//
// Flow, per claim:
//   1. Re-read the game's achievements from disk (they may have changed since the screen loaded)
//      and keep only the ones still unlocked.
//   2. For each, POST https://rare.market/v1/trophies/attest
//        { player, game: "steam:<appid>", achievement: "<api name>", evidence: { unlockTime, steamId } }
//      -> { registry, game, trophyId, nonce, expiresMs, signature } (u64s as decimal strings,
//      signature base64 of a 64-byte ed25519 signature over game::attestation_message).
//      Each answer is checked against what the device expects before it goes into a transaction.
//   3. One PTB for all of them: if the player has no Profile yet,
//        p = profile::create(reg, player); game::mint_attested(reg, game, p, ...) x N; profile::share(p)
//      else the shared Profile goes straight into each mint_attested.
//   4. Rare Wallet reviews and signs it (nothing is signed without the player approving), and it
//      is executed over Sui gRPC on the deployment's network.

import { Transaction } from "@mysten/sui/transactions";
import { fromBase64, normalizeSuiAddress } from "@mysten/sui/utils";
import { grpc } from "../sui/chain";
import type { TrophyConfig } from "./config";
import type { HttpReply } from "./native";
import type { ChainDef, ChainGame, SteamAchievement, SteamGame } from "./types";

export interface AttestRequest {
  player: string;
  game: string;
  achievement: string;
  evidence: { unlockTime: number | null; steamId: string };
}

export interface Attestation {
  registry: string;
  game: string;
  trophyId: bigint;
  nonce: bigint;
  expiresMs: bigint;
  signature: Uint8Array;
}

/** Most attestations one transaction carries (each mint is a handful of dynamic-field writes). */
export const MAX_PER_TX = 25;

export function attestRequest(player: string, appid: number, steamId: string, a: Pick<SteamAchievement, "apiName" | "unlockTime">): AttestRequest {
  return { player: normalizeSuiAddress(player), game: `steam:${appid}`, achievement: a.apiName, evidence: { unlockTime: a.unlockTime, steamId } };
}

const big = (v: unknown, what: string): bigint => {
  if ((typeof v === "string" && /^\d+$/.test(v)) || (typeof v === "number" && Number.isSafeInteger(v) && v >= 0)) return BigInt(v);
  throw new Error(`The attestation has no valid ${what}`);
};

/** Turns the service's reply into an Attestation, or throws with a sentence the UI can show. */
export function parseAttestation(reply: HttpReply, expect: { registry: string; game: string; trophyId: number; nowMs: number }): Attestation {
  const body = (reply.body ?? {}) as Record<string, unknown>;
  if (reply.status < 200 || reply.status >= 300) {
    const msg = typeof body.error === "string" ? body.error : typeof body.message === "string" ? body.message : null;
    throw new Error(msg ?? (reply.status === 429 ? "Too many claims right now. Try again in a minute." : `The attestation service answered ${reply.status}`));
  }
  const registry = typeof body.registry === "string" ? normalizeSuiAddress(body.registry) : "";
  const game = typeof body.game === "string" ? normalizeSuiAddress(body.game) : "";
  if (registry !== normalizeSuiAddress(expect.registry)) throw new Error("The attestation is for a different registry");
  if (game !== normalizeSuiAddress(expect.game)) throw new Error("The attestation is for a different game");
  const trophyId = big(body.trophyId, "trophy id");
  if (trophyId !== BigInt(expect.trophyId)) throw new Error("The attestation is for a different trophy");
  const expiresMs = big(body.expiresMs, "expiry");
  if (expiresMs <= BigInt(expect.nowMs)) throw new Error("The attestation has expired. Try again.");
  let signature: Uint8Array;
  try {
    signature = fromBase64(String(body.signature ?? ""));
  } catch {
    throw new Error("The attestation signature is not readable");
  }
  if (signature.length !== 64) throw new Error("The attestation signature is not readable");
  return { registry, game, trophyId, nonce: big(body.nonce, "nonce"), expiresMs, signature };
}

/** The claim PTB. `profileId` null: the player's Profile is created and shared in the same transaction. */
export function buildClaimTx(o: { pkg: string; registry: string; game: string; player: string; profileId: string | null; attestations: Attestation[] }): Transaction {
  if (!o.attestations.length) throw new Error("Nothing to claim");
  const pkg = normalizeSuiAddress(o.pkg);
  const tx = new Transaction();
  tx.setSenderIfNotSet(normalizeSuiAddress(o.player));
  const reg = tx.object(o.registry);
  const game = tx.object(o.game);
  const profile = o.profileId ? tx.object(o.profileId) : tx.moveCall({ target: `${pkg}::profile::create`, arguments: [reg, tx.pure.address(normalizeSuiAddress(o.player))] });
  for (const a of o.attestations) {
    tx.moveCall({
      target: `${pkg}::game::mint_attested`,
      arguments: [reg, game, profile, tx.pure.u64(a.trophyId), tx.pure.u64(a.nonce), tx.pure.u64(a.expiresMs), tx.pure.vector("u8", Array.from(a.signature)), tx.object.clock()],
    });
  }
  if (!o.profileId) tx.moveCall({ target: `${pkg}::profile::share`, arguments: [profile] });
  return tx;
}

export interface ClaimEnv {
  config: TrophyConfig;
  player: string;
  /** fresh read of one game's Steam files */
  recheck: (appid: number) => Promise<SteamGame | null>;
  attest: (body: AttestRequest) => Promise<HttpReply>;
  /** review, sign and execute; resolves once the network ran it successfully */
  submit: (tx: Transaction, title: string, info: { game: string; trophyIds: number[] }) => Promise<{ digest: string }>;
  now?: () => number;
}

export interface ClaimResult {
  digest: string;
  claimed: ChainDef[];
  /** achievements that were not claimed and why */
  skipped: Array<{ apiName: string; reason: string }>;
}

/**
 * Claims `apiNames` of one Steam game. `game` is the chain's Game with its defs, `profileId`
 * the player's Profile (null before their first trophy).
 */
export async function claimSteam(env: ClaimEnv, appid: number, apiNames: string[], game: ChainGame, profileId: string | null): Promise<ClaimResult> {
  if (!game.attested) throw new Error("This game does not take self-reported trophies");
  if (game.paused) throw new Error("Claims for this game are paused");
  const fresh = await env.recheck(appid);
  if (!fresh) throw new Error("Steam's files for this game are gone from this device");
  const skipped: ClaimResult["skipped"] = [];
  const picks: Array<{ a: SteamAchievement; def: ChainDef }> = [];
  for (const name of apiNames) {
    const a = fresh.achievements.find((x) => x.apiName === name);
    const def = game.defs.find((d) => d.key === name);
    if (!a || !a.unlocked) skipped.push({ apiName: name, reason: "No longer unlocked on this device" });
    else if (!def) skipped.push({ apiName: name, reason: "Not a trophy on Rare yet" });
    else if (def.tier === 3) skipped.push({ apiName: name, reason: "The platinum unlocks by itself" });
    else picks.push({ a, def });
  }
  const batch = picks.slice(0, MAX_PER_TX);
  if (!batch.length) throw new Error(skipped[0]?.reason ?? "Nothing to claim");
  const now = env.now ?? Date.now;
  const attestations: Attestation[] = [];
  for (const { a, def } of batch) {
    const reply = await env.attest(attestRequest(env.player, appid, fresh.steamId, a));
    attestations.push(parseAttestation(reply, { registry: env.config.registry, game: game.id, trophyId: def.trophyId, nowMs: now() }));
  }
  const tx = buildClaimTx({ pkg: env.config.package, registry: env.config.registry, game: game.id, player: env.player, profileId, attestations });
  const title = batch.length === 1 ? `Claim ${batch[0].def.name}` : `Claim ${batch.length} trophies`;
  const { digest } = await env.submit(tx, title, { game: game.id, trophyIds: batch.map((b) => b.def.trophyId) });
  return { digest, claimed: batch.map((b) => b.def), skipped };
}

/**
 * Rare Wallet signs (after its review sheet), then the transaction runs over Sui gRPC on the
 * deployment's network. Built here against that network so gas and versions resolve there.
 */
export function walletSubmit(config: TrophyConfig, player: string): ClaimEnv["submit"] {
  return async (tx, title) => {
    const client = grpc(config.network);
    tx.setSenderIfNotSet(normalizeSuiAddress(player));
    const bytes = await tx.build({ client });
    const w = await import("../wallet/sign");
    const signed = await w.signTransaction(bytes, { account: player, origin: { name: "Rare Trophies", kind: "wallet" }, title });
    const res = await client.core.executeTransaction({ transaction: bytes, signatures: [signed.signature], include: { effects: true } });
    if (res.$kind !== "Transaction") throw new Error("The network rejected the claim");
    const fx = res.Transaction.effects;
    if (fx && fx.status && fx.status.success === false) throw new Error("The claim failed on chain");
    return { digest: res.Transaction.digest };
  };
}
