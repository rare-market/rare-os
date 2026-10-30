/* What the send builder needs to know about the sender's NFTs, read over gRPC only:
 * the kiosks the wallet owns (plain, personal, OriginByte), what sits in each one (locked,
 * listed), which marketplace holds a listing (Drip, Tradeport), and the collection's transfer
 * policies, all from the fullnode (the Trade API only names which policy objects exist for a type).
 * Read only, never signs. */

import { bcs } from '@mysten/sui/bcs'
import { normalizeStructTag, normalizeSuiAddress, toHex } from '@mysten/sui/utils'
import { DRIP_LISTINGS_BAG, KIOSK_OWNER_CAP_TYPE, OB_OWNER_TOKEN_TYPE, PERSONAL_KIOSK_CAP_TYPE, TRADEPORT_ORDERBOOK_STORE, TRADEPORT_STORE } from './ids'
import { matchRule, policyProblem } from './policy'
import type { KioskRef, Market, PolicyRule, SendItem, TransferPolicyInfo } from './types'

/** The slice of SuiGrpcClient.core this module reads with (easy to stub in tests). */
export interface NftChainReader {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  listOwnedObjects(o: { owner: string; type?: string; cursor?: string | null; limit?: number; include?: { content?: boolean } }): Promise<{ objects: Array<{ objectId: string; type: string; content?: Uint8Array; owner?: any }>; hasNextPage: boolean; cursor: string | null }>
  listDynamicFields(o: { parentId: string; cursor?: string | null; limit?: number }): Promise<{
    dynamicFields: Array<{ name: { type: string; bcs: Uint8Array }; valueType: string; childId?: string; $kind?: string }>
    hasNextPage: boolean
    cursor: string | null
  }>
  getDynamicObjectField(o: { parentId: string; name: { type: string; bcs: Uint8Array } }): Promise<{ object: { objectId: string } }>
  getDynamicField(o: { parentId: string; name: { type: string; bcs: Uint8Array } }): Promise<{ dynamicField: { value: { bcs: Uint8Array } } }>
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  getObjects(o: { objectIds: string[]; include?: { content?: boolean } }): Promise<{ objects: Array<any> }>
  getObject(o: { objectId: string; include?: { previousTransaction?: boolean } }): Promise<{ object: { previousTransaction?: string | null } }>
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  getTransaction(o: { digest: string; include?: { objectTypes?: boolean } }): Promise<any>
}

export interface KioskSlot {
  kiosk: KioskRef
  isLocked: boolean
  isListed: boolean
  isExclusiveListing: boolean
}

export interface KioskContext {
  /** every item in a kiosk this wallet can open, by object id */
  items: Map<string, KioskSlot>
  /** items in OriginByte kiosks */
  obItems: Set<string>
  /** some kiosk could not be listed */
  partial: boolean
}

const norm = (id: string) => normalizeSuiAddress(id)
const hexAt = (b: Uint8Array, at: number) => norm(toHex(b.slice(at, at + 32)))

async function ownedAll(client: NftChainReader, owner: string, type: string) {
  const out: Array<{ objectId: string; content?: Uint8Array }> = []
  let cursor: string | null = null
  for (let page = 0; page < 20; page++) {
    const r = await client.listOwnedObjects({ owner, type, cursor, limit: 50, include: { content: true } })
    out.push(...r.objects)
    if (!r.hasNextPage || !r.cursor) break
    cursor = r.cursor
  }
  return out
}

async function fieldsAll(client: NftChainReader, parentId: string) {
  const out: Awaited<ReturnType<NftChainReader['listDynamicFields']>>['dynamicFields'] = []
  let cursor: string | null = null
  for (let page = 0; page < 40; page++) {
    const r = await client.listDynamicFields({ parentId, cursor, limit: 50 })
    out.push(...r.dynamicFields)
    if (!r.hasNextPage || !r.cursor) break
    cursor = r.cursor
  }
  return out
}

/** `0x2::kiosk::Item` / `Listing` / `Lock`, or null for any other field name type. */
function kioskKey(type: string): 'Item' | 'Listing' | 'Lock' | null {
  const m = /^(0x[0-9a-fA-F]+)::kiosk::(Item|Listing|Lock)$/.exec(type.trim())
  if (!m || norm(m[1]) !== norm('0x2')) return null
  return m[2] as 'Item' | 'Listing' | 'Lock'
}

/** The kiosks a wallet owns: plain ones (KioskOwnerCap) and personal ones (PersonalKioskCap). */
export async function ownedKiosks(client: NftChainReader, owner: string): Promise<{ kiosks: KioskRef[]; ob: string[] }> {
  const [caps, personal, tokens] = await Promise.all([
    ownedAll(client, owner, KIOSK_OWNER_CAP_TYPE),
    ownedAll(client, owner, PERSONAL_KIOSK_CAP_TYPE).catch(() => []),
    ownedAll(client, owner, OB_OWNER_TOKEN_TYPE).catch(() => []),
  ])
  const kiosks: KioskRef[] = []
  for (const c of caps) {
    // KioskOwnerCap { id: UID, for: ID }
    if (c.content && c.content.length >= 64) kiosks.push({ kioskId: hexAt(c.content, 32), ownerCapId: norm(c.objectId), isPersonal: false })
  }
  for (const p of personal) {
    // PersonalKioskCap { id: UID, cap: Option<KioskOwnerCap> }
    const b = p.content
    if (b && b.length >= 32 + 1 + 64 && b[32] === 1) kiosks.push({ kioskId: hexAt(b, 33 + 32), ownerCapId: hexAt(b, 33), personalCapId: norm(p.objectId), isPersonal: true })
  }
  // ob_kiosk::OwnerToken { id: UID, kiosk: ID, owner: address }
  const ob = tokens.filter((t) => t.content && t.content.length >= 64).map((t) => hexAt(t.content!, 32))
  return { kiosks, ob }
}

/** Every item in the wallet's kiosks with its lock and listing state. */
export async function loadKioskContext(client: NftChainReader, owner: string): Promise<KioskContext> {
  const { kiosks, ob } = await ownedKiosks(client, owner)
  const items = new Map<string, KioskSlot>()
  const obItems = new Set<string>()
  let partial = false
  await Promise.all([
    ...kiosks.map(async (kiosk) => {
      const fields = await fieldsAll(client, kiosk.kioskId).catch(() => null)
      if (!fields) return void (partial = true)
      const locked = new Set<string>()
      const listed = new Map<string, boolean>()
      const ids: string[] = []
      for (const f of fields) {
        const key = kioskKey(f.name.type)
        if (!key || f.name.bcs.length < 32) continue
        const id = hexAt(f.name.bcs, 0)
        if (key === 'Item') ids.push(id)
        else if (key === 'Lock') locked.add(id)
        else listed.set(id, f.name.bcs[32] === 1)
      }
      for (const id of ids) items.set(id, { kiosk, isLocked: locked.has(id), isListed: listed.has(id), isExclusiveListing: listed.get(id) === true })
    }),
    ...ob.map(async (k) => {
      const fields = await fieldsAll(client, k).catch(() => null)
      if (!fields) return void (partial = true)
      for (const f of fields) if (kioskKey(f.name.type) === 'Item' && f.name.bcs.length >= 32) obItems.add(hexAt(f.name.bcs, 0))
    }),
  ])
  return { items, obItems, partial }
}

const idKey = (id: string) => ({ type: '0x2::object::ID', bcs: bcs.Address.serialize(id).toBytes() })

/**
 * Which marketplace holds a listed kiosk item. Exclusive listings: Tradeport when its Store has a
 * SimpleListing for the NFT (orderbook when the orderbook Store also has that listing), otherwise
 * unknown (null). Plain listings: Drip when its registry has a record, otherwise a bare kiosk listing.
 */
export async function listingMarket(client: NftChainReader, nftId: string, exclusive: boolean): Promise<Market | null | undefined> {
  if (exclusive) {
    const listing = await client.getDynamicObjectField({ parentId: TRADEPORT_STORE, name: idKey(nftId) }).catch(() => null)
    if (!listing) return null
    const ob = await client.getDynamicField({ parentId: TRADEPORT_ORDERBOOK_STORE, name: idKey(listing.object.objectId) }).catch(() => null)
    return { kind: 'tradeport', orderbook: !!ob }
  }
  const drip = await client.getDynamicField({ parentId: DRIP_LISTINGS_BAG, name: idKey(nftId) }).catch(() => null)
  return drip ? { kind: 'drip' } : undefined
}

const TransferPolicyBcs = bcs.struct('TransferPolicy', {
  id: bcs.Address,
  balance: bcs.u64(),
  rules: bcs.vector(bcs.string()),
})

/**
 * Read transfer policies by id over gRPC: the objects give the rule types, and rule configs come
 * from the policy's dynamic fields when a rule needs them (floor price, staking registry).
 */
export async function readPolicies(client: NftChainReader, ids: string[]): Promise<TransferPolicyInfo[]> {
  if (!ids.length) return []
  const { objects } = await client.getObjects({ objectIds: [...new Set(ids)], include: { content: true } })
  const out: TransferPolicyInfo[] = []
  for (const o of objects) {
    if (!o || o instanceof Error || !o.content) continue
    let parsed: { rules: string[] }
    try {
      parsed = TransferPolicyBcs.parse(o.content)
    } catch {
      continue
    }
    const shared = o.owner?.$kind ? o.owner.$kind === 'Shared' || o.owner.$kind === 'ConsensusAddressOwner' : undefined
    const rules: PolicyRule[] = parsed.rules.map((r) => {
      const ruleType = r.startsWith('0x') ? r : `0x${r}`
      return { type: ruleType.split('::')[1] ?? ruleType, ruleType }
    })
    // configs only matter for rules whose proof takes a value from them
    const needConfig = rules.filter((r) => ['floor_price_rule', 'staking_lock_rule'].includes(matchRule(r)?.spec.id ?? ''))
    if (needConfig.length) {
      const fields = await fieldsAll(client, o.objectId).catch(() => [])
      for (const r of needConfig) {
        const f = fields.find((x) => x.name.type.includes(`::${r.ruleType!.split('::').slice(1).join('::')}`))
        if (!f) continue
        const v = await client.getDynamicField({ parentId: o.objectId, name: f.name }).catch(() => null)
        const b = v?.dynamicField.value.bcs
        if (!b) continue
        const id = matchRule(r)?.spec.id
        if (id === 'floor_price_rule' && b.length >= 8) r.config = { floor_price: bcs.u64().parse(b.slice(0, 8)) }
        if (id === 'staking_lock_rule' && b.length >= 32) r.config = { registry_id: hexAt(b, 0) }
      }
    }
    out.push({ id: norm(o.objectId), isShared: shared, rules })
  }
  return out
}

/**
 * Policy ids from the item's own history: the transaction that last moved a locked item into a
 * kiosk had to pass its TransferPolicy<T> (kiosk::lock, confirm_request), so it is among that
 * transaction's objects. Pruned fullnodes only keep recent transactions, so this is a fallback.
 */
export async function policyIdsFromHistory(client: NftChainReader, nftId: string, nftType: string): Promise<string[]> {
  const want = `0x2::transfer_policy::TransferPolicy<${normalizeStructTag(nftType)}>`
  const { object } = await client.getObject({ objectId: nftId, include: { previousTransaction: true } })
  if (!object.previousTransaction) return []
  const tx = await client.getTransaction({ digest: object.previousTransaction, include: { objectTypes: true } })
  const types: Record<string, string> = (tx.Transaction ?? tx.FailedTransaction)?.objectTypes ?? {}
  return Object.entries(types)
    .filter(([, t]) => {
      try {
        return normalizeStructTag(t) === normalizeStructTag(want)
      } catch {
        return false
      }
    })
    .map(([id]) => norm(id))
}

/**
 * Ids of every TransferPolicy<T> for a type, from the Trade API (GET /ext/nft/policies, found in its
 * indexed chain history: gRPC has no way to find shared objects by type and public fullnodes prune
 * the creation events). Throws when the Trade API can't answer.
 */
export async function policyIdsFromTrade(base: string, nftType: string, f: typeof fetch = fetch): Promise<string[]> {
  const url = `${base.replace(/\/+$/, '')}/ext/nft/policies?type=${encodeURIComponent(normalizeStructTag(nftType))}`
  const res = await f(url, { signal: AbortSignal.timeout(8_000) })
  if (!res.ok) throw new Error(`Policy lookup failed (${res.status})`)
  const body = (await res.json()) as { policies?: Array<{ id?: unknown }> }
  if (!Array.isArray(body.policies)) throw new Error('Policy lookup failed')
  return body.policies.flatMap((p) => (typeof p.id === 'string' ? [norm(p.id)] : []))
}

/**
 * A collection's transfer policies: ids from the Trade API, each policy's rules and configs read over gRPC.
 * When the Trade API can't answer or knows none, the item's last transaction (gRPC) names the policy it was
 * locked under.
 */
export function policyLoader(client: NftChainReader, tradeBase: string, f: typeof fetch = fetch) {
  return async (nftType: string, nftId: string): Promise<TransferPolicyInfo[]> => {
    const ids = await policyIdsFromTrade(tradeBase, nftType, f).catch(() => [] as string[])
    const found = ids.length ? await readPolicies(client, ids) : []
    if (found.length) return found
    return readPolicies(client, await policyIdsFromHistory(client, nftId, nftType))
  }
}

/** One NFT as the send screen knows it. */
export interface NftRef {
  objectId: string
  type: string
  /** the Trade API says it sits in a kiosk */
  kiosk?: boolean
}

export type Sendability = { ok: true; send: SendItem } | { ok: false; reason: string }

export interface AnalyzeDeps {
  client: NftChainReader
  /** policies for a type (the item id helps find them from its history) */
  policies?: (nftType: string, nftId: string) => Promise<TransferPolicyInfo[]>
  /** whether a struct type has `store` (needed to transfer it from the wallet); undefined when unknown */
  hasStore?: (nftType: string) => Promise<boolean | undefined>
}

/** Whether each NFT can be sent, and the builder input for the ones that can. */
export async function analyzeNfts(deps: AnalyzeDeps, owner: string, nfts: NftRef[]): Promise<Map<string, Sendability>> {
  const out = new Map<string, Sendability>()
  const inKiosk = nfts.filter((n) => n.kiosk)
  const inWallet = nfts.filter((n) => !n.kiosk)
  // objects without `store` (soulbound badges and the like) can't leave the wallet
  const storeOf = new Map<string, boolean | undefined>()
  if (deps.hasStore && inWallet.length) {
    const types = [...new Set(inWallet.map((n) => n.type.split('<')[0]))]
    await Promise.all(types.map(async (t) => storeOf.set(t, await deps.hasStore!(t).catch(() => undefined))))
  }
  for (const n of inWallet)
    out.set(
      n.objectId,
      storeOf.get(n.type.split('<')[0]) === false
        ? { ok: false, reason: 'It is bound to this account. Its creator made it so it can not be transferred' }
        : { ok: true, send: { from: null, item: { nftId: norm(n.objectId), nftType: n.type, isLocked: false } } },
    )
  if (!inKiosk.length) return out

  const ctx = await loadKioskContext(deps.client, owner)
  const policyCache = new Map<string, Promise<TransferPolicyInfo[] | null>>()
  const fallback = async (t: string, nftId: string) => readPolicies(deps.client, await policyIdsFromHistory(deps.client, nftId, t))
  const policiesOf = (t: string, nftId: string) => {
    let p = policyCache.get(t)
    if (!p) {
      p = (deps.policies ?? fallback)(t, nftId).catch(() => null)
      policyCache.set(t, p)
    }
    return p
  }

  await Promise.all(
    inKiosk.map(async (n) => {
      const id = norm(n.objectId)
      const slot = ctx.items.get(id)
      if (!slot) {
        out.set(n.objectId, {
          ok: false,
          reason: ctx.obItems.has(id)
            ? 'It sits in an OriginByte kiosk, which only OriginByte trading can move'
            : ctx.partial
              ? 'Could not read its kiosk. Try again in a moment'
              : 'Could not find the kiosk that holds it',
        })
        return
      }
      let market: Market | undefined
      if (slot.isListed) {
        const m = await listingMarket(deps.client, id, slot.isExclusiveListing)
        if (m === null) return void out.set(n.objectId, { ok: false, reason: 'It is listed on a marketplace this wallet can not delist from. Cancel that listing first' })
        market = m
      }
      let policies: TransferPolicyInfo[] | null = null
      if (slot.isLocked) {
        policies = await policiesOf(n.type, id)
        if (policies === null) return void out.set(n.objectId, { ok: false, reason: 'Could not load the collection rules. Try again in a moment' })
        const problem = policyProblem(policies)
        if (problem) return void out.set(n.objectId, { ok: false, reason: problem })
      }
      out.set(n.objectId, {
        ok: true,
        send: {
          from: slot.kiosk,
          item: { nftId: id, nftType: n.type, isLocked: slot.isLocked, isListed: slot.isListed, isExclusiveListing: slot.isExclusiveListing, market },
          policies,
        },
      })
    }),
  )
  return out
}
