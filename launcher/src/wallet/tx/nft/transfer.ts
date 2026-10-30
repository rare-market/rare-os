/* Send NFTs (wallet or kiosk) to another address in one PTB.
 * Wallet NFTs and unlocked kiosk NFTs are transferred
 * directly. Locked kiosk NFTs go into one new kiosk created for the recipient (its cap is sent
 * to them); collections whose policy needs a personal kiosk go into a new personal kiosk made
 * for the recipient with personal_kiosk::create_for. Builders never fetch and never sign. */

import { Transaction, type TransactionObjectArgument } from '@mysten/sui/transactions'
import { normalizeSuiAddress } from '@mysten/sui/utils'
import { ARGS_PER_COMMAND, chunk, planBatches, SUI_LIMITS, type Limits } from './limits'
import { DRIP_LISTING_REGISTRY, SUI_TYPE, TRADEPORT_ORDERBOOK_STORE, TRADEPORT_STORE, target } from './ids'
import { KioskSession, type KioskHandle } from './kiosk'
import { pickPolicy, policyNeeds, resolveTransferRequest } from './policy'
import type { KioskItemRef, SendItem, TransferPolicyInfo } from './types'

/** Cancel the item's listing so it can leave the kiosk: Drip, Tradeport, or a plain kiosk listing. */
function clearListing(tx: Transaction, from: KioskHandle, item: KioskItemRef) {
  const id = tx.pure.id(item.nftId)
  if (item.market?.kind === 'drip') {
    tx.moveCall({ target: target.dripMarketplace('delist'), typeArguments: [item.nftType], arguments: [tx.object(DRIP_LISTING_REGISTRY), from.kiosk, from.cap, id] })
    return
  }
  if (item.market?.kind === 'tradeport') {
    if (item.market.orderbook)
      tx.moveCall({
        target: target.tradeport('cancel_ob_listing_with_transfer_policy'),
        typeArguments: [item.nftType],
        arguments: [tx.object(TRADEPORT_STORE), tx.object(TRADEPORT_ORDERBOOK_STORE), from.kiosk, from.cap, id],
      })
    else tx.moveCall({ target: target.tradeport('cancel_listing_with_transfer_policy'), typeArguments: [item.nftType], arguments: [tx.object(TRADEPORT_STORE), from.kiosk, from.cap, id] })
    return
  }
  if (item.isListed && item.isExclusiveListing) throw new Error('This NFT is listed elsewhere with a purchase cap; cancel that listing first')
  if (item.isListed) tx.moveCall({ target: '0x2::kiosk::delist', typeArguments: [item.nftType], arguments: [from.kiosk, from.cap, id] })
}

/**
 * Take an item out of `from`. Unlocked items are taken with kiosk::take and returned for the
 * caller to transfer. Locked items are bought from ourselves with a purchase cap (at the floor
 * price if the policy has one; the payment lands back in `from` and is withdrawn again) and the
 * TransferRequest is resolved against the policy into `dest.kiosk`.
 */
function extractItem(
  session: KioskSession,
  from: KioskHandle,
  item: KioskItemRef,
  policies: TransferPolicyInfo[] | null | undefined,
  dest: { kiosk: KioskHandle | null; recipient: string },
): TransactionObjectArgument | null {
  const tx = session.tx
  clearListing(tx, from, item)
  if (!item.isLocked) {
    return tx.moveCall({ target: '0x2::kiosk::take', typeArguments: [item.nftType], arguments: [from.kiosk, from.cap, tx.pure.id(item.nftId)] })
  }
  const policy = pickPolicy(policies)
  if (!policy) throw new Error(`No usable transfer policy for ${item.nftType}; the locked item can't be moved`)
  const price = policyNeeds(policy).floor ?? 0n
  const purchaseCap = tx.moveCall({
    target: '0x2::kiosk::list_with_purchase_cap',
    typeArguments: [item.nftType],
    arguments: [from.kiosk, from.cap, tx.pure.id(item.nftId), tx.pure.u64(price)],
  })
  const payment: TransactionObjectArgument =
    price > 0n ? tx.splitCoins(tx.gas, [tx.pure.u64(price)])[0] : tx.moveCall({ target: '0x2::coin::zero', typeArguments: [SUI_TYPE] })
  const [obj, request] = tx.moveCall({ target: '0x2::kiosk::purchase_with_cap', typeArguments: [item.nftType], arguments: [from.kiosk, purchaseCap, payment] })
  resolveTransferRequest(tx, { nftType: item.nftType, policy, request, item: obj, kiosk: dest.kiosk, recipient: dest.recipient, preferKiosk: !!dest.kiosk })
  if (price > 0n) {
    const refund = tx.moveCall({ target: '0x2::kiosk::withdraw', arguments: [from.kiosk, from.cap, tx.pure.option('u64', price)] })
    tx.transferObjects([refund], session.sender)
  }
  return null
}

/** Send NFTs to another address in one transaction. */
export function buildSendNfts(sender: string, recipient: string, items: SendItem[], tx = new Transaction()): Transaction {
  const from = normalizeSuiAddress(sender)
  const to = normalizeSuiAddress(recipient)
  if (from === to) throw new Error('Pick an address other than this account')
  tx.setSenderIfNotSet(from)
  const session = new KioskSession(tx, from)
  const direct: TransactionObjectArgument[] = []
  let recipientKiosk: KioskHandle | null = null
  let recipientPersonalKiosk: KioskHandle | null = null
  for (const send of items) {
    if (!send.from) {
      direct.push(tx.object(send.item.nftId))
      continue
    }
    const source = session.open(send.from)
    let kiosk: KioskHandle | null = null
    if (send.item.isLocked) {
      const needs = policyNeeds(pickPolicy(send.policies))
      if (needs.personal) kiosk = recipientPersonalKiosk ??= session.create({ owner: to, personal: true })
      else kiosk = recipientKiosk ??= session.create({ owner: to })
    }
    const taken = extractItem(session, source, send.item, send.policies, { kiosk, recipient: to })
    if (taken) direct.push(taken)
  }
  if (direct.length) {
    const toArg = tx.pure.address(to)
    for (const part of chunk(direct, ARGS_PER_COMMAND)) tx.transferObjects(part, toArg)
  }
  session.finish()
  return tx
}

/** Kiosk items next to each other, so a batch borrows each kiosk once. Wallet NFTs last. */
function byKiosk(items: SendItem[]): SendItem[] {
  return items
    .map((s, i) => ({ s, i }))
    .sort((a, b) => {
      const ka = a.s.from?.kioskId ?? '~'
      const kb = b.s.from?.kioskId ?? '~'
      return ka < kb ? -1 : ka > kb ? 1 : a.i - b.i
    })
    .map((x) => x.s)
}

/** The fewest transactions that send every item, each inside Sui's limits. */
export function planNftSends(sender: string, recipient: string, items: SendItem[], limits: Limits = SUI_LIMITS): SendItem[][] {
  if (!items.length) return []
  return planBatches(byKiosk(items), (slice) => buildSendNfts(sender, recipient, slice), limits)
}
