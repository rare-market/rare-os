// NFT send entry points for the screens: which NFTs can go (wallet objects, kiosk items incl.
// personal kiosks and locked items under transfer policies, Drip/Tradeport listings cancelled
// first) and the transaction that sends them. Reads are gRPC; the Trade API only names policy ids.

import type { Transaction } from "@mysten/sui/transactions";
import { parseStructTag } from "@mysten/sui/utils";
import { grpcClient } from "../grpc";
import { TRADE_URL } from "../data/prices";
import { checkAddress, SendError } from "./coin";
import { analyzeNfts, buildSendNfts, planNftSends, policyLoader, type NftChainReader, type NftRef, type Sendability, type SendItem } from "./nft";

/** The slice of MovePackageService used for the soulbound check. */
export interface DatatypeClient {
  movePackageService: {
    getDatatype(o: { packageId: string; moduleName: string; name: string }): PromiseLike<{ response: { datatype?: { abilities?: number[] } } }>;
  };
}

/** Ability.STORE = 3 in sui.rpc.v2. Undefined when the node does not say. */
export async function hasStore(type: string, client: DatatypeClient = grpcClient as unknown as DatatypeClient): Promise<boolean | undefined> {
  const tag = parseStructTag(type);
  const { response } = await client.movePackageService.getDatatype({ packageId: tag.address, moduleName: tag.module, name: tag.name });
  const abilities = response.datatype?.abilities;
  return abilities?.length ? abilities.includes(3) : undefined;
}

/** Whether each NFT can be sent, with the builder input for the ones that can (keyed by objectId). */
export function nftSendability(owner: string, nfts: NftRef[]): Promise<Map<string, Sendability>> {
  const reader = grpcClient.core as unknown as NftChainReader;
  return analyzeNfts({ client: reader, policies: policyLoader(reader, TRADE_URL), hasStore: (t) => hasStore(t) }, owner, nfts);
}

export interface NftSendParams {
  sender: string;
  to: string;
  /** from nftSendability(); items that can't be sent never get here */
  items: SendItem[];
}

/** One transaction that sends every item. Use planNftSends for selections too big for one. */
export function buildNftSend(p: NftSendParams): Transaction {
  const sender = checkAddress(p.sender);
  const to = checkAddress(p.to);
  if (!p.items.length) throw new SendError("bad-amount", "Pick at least one item");
  if (sender === to) throw new SendError("same-address", "Pick an address other than this account");
  const tx = buildSendNfts(sender, to, p.items);
  tx.setSender(sender);
  return tx;
}

/** Split a large selection into the fewest transactions that fit Sui's limits. */
export function planNftSend(p: NftSendParams): SendItem[][] {
  return planNftSends(checkAddress(p.sender), checkAddress(p.to), p.items);
}
