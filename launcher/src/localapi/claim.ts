// Submitting a studio voucher after the player pressed Claim on the claim sheet. Builds the one
// transaction for it and hands it to Rare Wallet, whose review sheet shows it and asks for the PIN
// or the device. Trophy claims go through Rare's gas sponsor (category "trophy") when the wallet is
// short of SUI; item claims pay their own gas until game budgets exist (build item 7).

import { normalizeSuiAddress } from "@mysten/sui/utils";
import type { ItemCard } from "../marketplace/api";
import { native } from "../lib/bridge";
import { buildClaimTx } from "../trophies/attest";
import { grpcReader, lookupProfile, readRegistry } from "../trophies/chain";
import { trophyConfig } from "../trophies/config";
import { grpcClient } from "../wallet/grpc";
import { signAndExecuteForGame, signAndExecuteWithCover } from "../wallet/sign";
import { ownedKiosks, type NftChainReader } from "../wallet/tx/nft/context";
import { ApiFail, type ClaimSheetData } from "./methods";
import { buildItemClaim, buildStateClaim, itemsPackage } from "./voucher";

export interface Claimed {
  status: "claimed";
  digest: string;
}

async function kiosksOf(owner: string) {
  const core = (grpcClient as unknown as { core: NftChainReader }).core;
  return (await ownedKiosks(core, owner)).kiosks.map((k) => ({ kioskId: normalizeSuiAddress(k.kioskId), ownerCapId: k.ownerCapId, isPersonal: k.isPersonal }));
}

export async function runClaim(c: ClaimSheetData, player: string, from: { name: string; url?: string }): Promise<Claimed> {
  const v = c.voucher;
  const origin = { name: from.name, url: from.url, kind: "game" as const };
  // A game's claim: its gas budget pays first (game:<slug>), then Rare's trophy cover, then the player.
  const payer = { slug: c.game?.slug ?? null, name: c.game?.title ?? from.name };
  const done = (r: { status?: string; digest?: string }): Claimed => {
    if (r.status === "failure") throw new Error("The network rejected the claim");
    return { status: "claimed", digest: r.digest ?? "" };
  };
  if (v.type === "trophy") {
    const cfg = trophyConfig();
    if (!cfg) throw new ApiFail("unavailable", "Rare Trophies isn't live yet");
    const read = grpcReader(cfg.network);
    const profileId = await lookupProfile(read, await readRegistry(read, cfg.registry), player);
    const tx = buildClaimTx({ pkg: cfg.package, registry: cfg.registry, game: v.game, player, profileId, attestations: [{ registry: v.registry, game: v.game, trophyId: v.trophyId, nonce: v.nonce, expiresMs: v.expiresMs, signature: v.sig }] });
    return done(await signAndExecuteForGame(tx, payer, { account: player, origin, title: c.title }, (o) => signAndExecuteWithCover(tx, "trophy", o)));
  }
  const pkg = itemsPackage("mainnet");
  if (!pkg || !c.chain) throw new ApiFail("unavailable", "Rare items aren't live yet");
  const kiosks = await kiosksOf(player);
  if (v.type === "item") {
    const tx = buildItemClaim(pkg, v, { marker: c.chain.marker, policyId: c.chain.policyId! }, player, kiosks.find((k) => !k.isPersonal) ?? null);
    return done(await signAndExecuteForGame(tx, payer, { account: player, origin, title: c.title }));
  }
  // Where the item is now: in one of the player's kiosks, or in the wallet.
  const card = await native.rareGet<ItemCard>(`/v1/items/${encodeURIComponent(v.itemId)}`).catch(() => null);
  const kid = card?.kioskId ? normalizeSuiAddress(card.kioskId) : null;
  const k = kid ? kiosks.find((x) => x.kioskId === kid) : null;
  if (kid && !k) throw new ApiFail("unavailable", "That item isn't in one of your kiosks");
  if (k?.isPersonal) throw new ApiFail("unavailable", "Items in a personal kiosk can't be updated yet");
  const tx = buildStateClaim(pkg, v, { marker: c.chain.marker, classId: c.chain.classId! }, player, k ?? null);
  return done(await signAndExecuteForGame(tx, payer, { account: player, origin, title: c.title }));
}
