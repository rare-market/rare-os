// Rare Store transactions (rare-store INTERFACE.md, "Players"). Each builder returns an unsigned
// Transaction with the sender set; the caller hands it to Rare Wallet's review sheet
// (signAndExecute). Tests build and dry-run them; nothing here signs or executes.
//
//   buy_usdc<USDC>(store, game, &mut Coin<USDC>, clock)             exact price in USDC
//   buy_sui(store, game, &mut Coin<SUI>, PriceInfoObject, clock)    after a fresh Pyth update
//   claim_free(store, game, clock)                                  price 0
//   refund<USDC>(store, game, Licence, clock)                       inside the refund window
//   resale::make_tradable<USDC>(store, game, Licence, kiosk, cap, policy, clock)

import { coinWithBalance, Transaction, type TransactionObjectArgument } from "@mysten/sui/transactions";
import { native } from "../lib/bridge";
import { KioskScope, loadTradeContext, type OwnedKioskRef, type RareGet } from "../marketplace/venues";
import { grpcClient } from "../wallet/grpc";
import { ownedKiosks, type NftChainReader } from "../wallet/tx/nft/context";
import { addSuiUsdUpdateFrom, PYTH, updateBytes } from "./pyth";
import type { StoreConfig, SuiPrice } from "./types";

/** A deployed store: what every builder needs from /v1/store/config. */
export interface StoreIds {
  package: string;
  store: string;
  storeVersion: string | null;
  usdcType: string;
  policy?: string | null;
  policyVersion?: string | null;
}

export function idsOf(c: StoreConfig): StoreIds {
  if (!c.package || !c.store || !c.usdcType) throw new Error("The store isn't open yet.");
  return { package: c.package, store: c.store, storeVersion: c.storeVersion, usdcType: c.usdcType, policy: c.policy, policyVersion: c.policyVersion };
}

/** `&Store`: the shared Store, by its initial version when the config has it (no lookup). */
function storeArg(tx: Transaction, ids: StoreIds): TransactionObjectArgument {
  return ids.storeVersion ? tx.sharedObjectRef({ objectId: ids.store, initialSharedVersion: ids.storeVersion, mutable: false }) : tx.object(ids.store);
}

function policyArg(tx: Transaction, ids: StoreIds): TransactionObjectArgument {
  if (!ids.policy) throw new Error("Resale isn't open yet.");
  return ids.policyVersion ? tx.sharedObjectRef({ objectId: ids.policy, initialSharedVersion: ids.policyVersion, mutable: false }) : tx.object(ids.policy);
}

const fn = (ids: StoreIds, m: string) => `${ids.package}::${m}`;

function start(sender: string): Transaction {
  const tx = new Transaction();
  tx.setSender(sender);
  return tx;
}

/** Buy with USDC: a coin of exactly the price; buy_usdc splits it off and the empty coin is destroyed. */
export function buildBuyUsdc(ids: StoreIds, o: { sender: string; gameId: string; priceUsd: bigint }): Transaction {
  if (o.priceUsd <= 0n) throw new Error("This game is free. Use claim_free.");
  const tx = start(o.sender);
  const pay = tx.add(coinWithBalance({ type: ids.usdcType, balance: o.priceUsd }));
  tx.moveCall({ target: fn(ids, "game::buy_usdc"), typeArguments: [ids.usdcType], arguments: [storeArg(tx, ids), tx.object(o.gameId), pay, tx.object.clock()] });
  tx.moveCall({ target: "0x2::coin::destroy_zero", typeArguments: [ids.usdcType], arguments: [pay] });
  return tx;
}

/** MIST owed for `usd` micro-USD at `priceE8` (store::usd_to_mist, rounded up). */
export function usdToMist(usd: bigint, priceE8: bigint): bigint {
  if (priceE8 <= 0n) throw new Error("No SUI price right now.");
  const num = usd * 100_000_000_000n;
  return (num + priceE8 - 1n) / priceE8;
}

/** What a SUI purchase costs at the quoted price, and the most it may take (quote plus 1%). */
export function suiQuote(priceUsd: bigint, price: Pick<SuiPrice, "price_e8">): { mist: bigint; maxMist: bigint } {
  const mist = usdToMist(priceUsd, BigInt(price.price_e8));
  return { mist, maxMist: mist + (mist + 99n) / 100n };
}

/**
 * Buy with SUI: push the fresh Pyth price first (unless the server says the on-chain price is fresh), split quote + 1% off gas as `pay`, buy_sui takes
 * exactly what the price says and the rest goes back into gas.
 */
export function buildBuySui(ids: StoreIds, o: { sender: string; gameId: string; priceUsd: bigint; price: SuiPrice }): { tx: Transaction; mist: bigint; maxMist: bigint } {
  if (o.priceUsd <= 0n) throw new Error("This game is free. Use claim_free.");
  if (!o.price.fresh && !o.price.update_b64) throw new Error("No SUI price right now. Try again in a moment.");
  const { mist, maxMist } = suiQuote(o.priceUsd, o.price);
  const tx = start(o.sender);
  // The server says the on-chain object is fresh enough for the contract: skip the Wormhole and Pyth update calls.
  if (!o.price.fresh) addSuiUsdUpdateFrom(tx, updateBytes(o.price.update_b64));
  const [pay] = tx.splitCoins(tx.gas, [maxMist]);
  tx.moveCall({ target: fn(ids, "game::buy_sui"), arguments: [storeArg(tx, ids), tx.object(o.gameId), pay, tx.object(PYTH.suiUsdPriceInfoObject), tx.object.clock()] });
  tx.mergeCoins(tx.gas, [pay]);
  return { tx, mist, maxMist };
}

export function buildClaimFree(ids: StoreIds, o: { sender: string; gameId: string }): Transaction {
  const tx = start(o.sender);
  tx.moveCall({ target: fn(ids, "game::claim_free"), arguments: [storeArg(tx, ids), tx.object(o.gameId), tx.object.clock()] });
  return tx;
}

export function buildRefund(ids: StoreIds, o: { sender: string; gameId: string; licenceId: string }): Transaction {
  const tx = start(o.sender);
  tx.moveCall({ target: fn(ids, "game::refund"), typeArguments: [ids.usdcType], arguments: [storeArg(tx, ids), tx.object(o.gameId), tx.object(o.licenceId), tx.object.clock()] });
  return tx;
}

/**
 * Make a licence tradable: it becomes a ResaleLicence locked in the sender's kiosk. Uses the
 * kiosk the Marketplace would (a plain one first, else a personal one), or creates one in the same
 * transaction, the way the native listing flow does (KioskScope).
 */
export function buildMakeTradable(ids: StoreIds, o: { sender: string; gameId: string; licenceId: string; kiosks: OwnedKioskRef[] }): Transaction {
  const tx = start(o.sender);
  const scope = new KioskScope(tx, o.sender);
  const ref = o.kiosks.find((k) => !k.isPersonal && k.ownerCapId) ?? o.kiosks.find((k) => k.isPersonal && k.personalCapId) ?? null;
  const k = ref ? scope.open(ref) : scope.create();
  tx.moveCall({
    target: fn(ids, "resale::make_tradable"),
    typeArguments: [ids.usdcType],
    arguments: [storeArg(tx, ids), tx.object(o.gameId), tx.object(o.licenceId), k.kiosk, k.cap, policyArg(tx, ids), tx.object.clock()],
  });
  scope.close();
  return tx;
}

/** The sender's kiosks: rare.market's list, the chain (gRPC) when rare.market can't answer. */
export async function senderKiosks(sender: string, get: RareGet = native.rareGet): Promise<OwnedKioskRef[]> {
  const core = (grpcClient as unknown as { core: NftChainReader }).core;
  const ctx = await loadTradeContext(get, sender, [], (s) => ownedKiosks(core, s).then((r) => r.kiosks));
  return ctx.buyerKiosks;
}
