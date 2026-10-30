import { bcs } from '@mysten/sui/bcs'
import type { Transaction } from '@mysten/sui/transactions'
import { fromBase64, normalizeSuiAddress, toHex } from '@mysten/sui/utils'
import { describe, expect, it, vi } from 'vitest'
import { fits, measure, SUI_LIMITS } from './limits'
import { analyzeNfts, policyIdsFromTrade, policyLoader, type NftChainReader } from './context'
import { DRIP_LISTING_REGISTRY, DRIP_LISTINGS_BAG, KIOSK_EXT_PACKAGE, KIOSK_OWNER_CAP_TYPE, OB_OWNER_TOKEN_TYPE, PERSONAL_KIOSK_CAP_TYPE, TRADEPORT_ORDERBOOK_STORE, TRADEPORT_STORE } from './ids'
import { buildSendNfts, planNftSends } from './transfer'
import type { KioskRef, SendItem, TransferPolicyInfo } from './types'

const SENDER = '0x' + 'a1'.repeat(32)
const TO = '0x' + 'b2'.repeat(32)
const id = (n: number) => '0x' + n.toString(16).padStart(64, '0')
const NFT_T = '0x' + 'c3'.repeat(32) + '::punks::Punk'

const PLAIN_KIOSK: KioskRef = { kioskId: id(0xa000), ownerCapId: id(0xa001), isPersonal: false }
const PERSONAL_KIOSK: KioskRef = { kioskId: id(0xb000), ownerCapId: id(0xb001), personalCapId: id(0xb002), isPersonal: true }

const rule = (module: string) => ({ type: `${module}`, ruleType: `${KIOSK_EXT_PACKAGE}::${module}::Rule` })
const ROYALTY_LOCK: TransferPolicyInfo = { id: id(0xf001), isShared: true, rules: [rule('royalty_rule'), rule('kiosk_lock_rule')] }
const PERSONAL_POLICY: TransferPolicyInfo = { id: id(0xf002), isShared: true, rules: [rule('kiosk_lock_rule'), rule('personal_kiosk_rule')] }
const FLOOR_POLICY: TransferPolicyInfo = { id: id(0xf003), isShared: true, rules: [{ ...rule('floor_price_rule'), config: { floor_price: '1000' } }] }

const wallet = (n: number): SendItem => ({ from: null, item: { nftId: id(n), nftType: NFT_T, isLocked: false } })
const unlocked = (n: number, from = PLAIN_KIOSK, extra = {}): SendItem => ({ from, item: { nftId: id(n), nftType: NFT_T, isLocked: false, ...extra } })
const locked = (n: number, policy = ROYALTY_LOCK, from = PLAIN_KIOSK): SendItem => ({ from, item: { nftId: id(n), nftType: NFT_T, isLocked: true }, policies: [policy] })

const commands = (tx: Transaction) => tx.getData().commands
const calls = (tx: Transaction) =>
  commands(tx).map((c) => (c.$kind === 'MoveCall' ? `${c.MoveCall.module}::${c.MoveCall.function}` : c.$kind))

/** every object id the transaction names: object inputs and 32-byte pure ids */
function namedIds(tx: Transaction): Set<string> {
  const out = new Set<string>()
  for (const i of tx.getData().inputs) {
    if (i.$kind === 'UnresolvedObject') out.add(normalizeSuiAddress(i.UnresolvedObject.objectId))
    if (i.$kind === 'Pure') {
      const b = fromBase64(i.Pure.bytes)
      if (b.length === 32) out.add(normalizeSuiAddress(toHex(b)))
    }
  }
  return out
}

describe('buildSendNfts', () => {
  it('sends wallet NFTs with one TransferObjects and every item as an input', () => {
    const items = [1, 2, 3].map(wallet)
    const tx = buildSendNfts(SENDER, TO, items)
    expect(calls(tx)).toEqual(['TransferObjects'])
    const named = namedIds(tx)
    for (const s of items) expect(named.has(s.item.nftId)).toBe(true)
  })

  it('splits more than 500 wallet NFTs across TransferObjects commands', () => {
    const tx = buildSendNfts(SENDER, TO, Array.from({ length: 600 }, (_, i) => wallet(i + 1)))
    const t = commands(tx).filter((c) => c.$kind === 'TransferObjects')
    expect(t.map((c) => c.TransferObjects!.objects.length)).toEqual([500, 100])
  })

  it('takes unlocked kiosk items and sends them with the wallet ones', () => {
    const tx = buildSendNfts(SENDER, TO, [unlocked(1), unlocked(2), wallet(3)])
    expect(calls(tx)).toEqual(['kiosk::take', 'kiosk::take', 'TransferObjects'])
    expect(commands(tx)[2].TransferObjects!.objects).toHaveLength(3)
  })

  it('cancels listings first: plain kiosk, Drip, Tradeport and Tradeport orderbook', () => {
    const tx = buildSendNfts(SENDER, TO, [
      unlocked(1, PLAIN_KIOSK, { isListed: true }),
      unlocked(2, PLAIN_KIOSK, { isListed: true, market: { kind: 'drip' } }),
      unlocked(3, PLAIN_KIOSK, { isListed: true, isExclusiveListing: true, market: { kind: 'tradeport', orderbook: false } }),
      unlocked(4, PLAIN_KIOSK, { isListed: true, isExclusiveListing: true, market: { kind: 'tradeport', orderbook: true } }),
    ])
    expect(calls(tx)).toEqual([
      'kiosk::delist',
      'kiosk::take',
      'marketplace::delist',
      'kiosk::take',
      'tradeport_listings::cancel_listing_with_transfer_policy',
      'kiosk::take',
      'tradeport_listings::cancel_ob_listing_with_transfer_policy',
      'kiosk::take',
      'TransferObjects',
    ])
    const named = namedIds(tx)
    for (const o of [DRIP_LISTING_REGISTRY, TRADEPORT_STORE, TRADEPORT_ORDERBOOK_STORE]) expect(named.has(normalizeSuiAddress(o))).toBe(true)
  })

  it('refuses an item listed elsewhere with a purchase cap', () => {
    expect(() => buildSendNfts(SENDER, TO, [unlocked(1, PLAIN_KIOSK, { isListed: true, isExclusiveListing: true })])).toThrow(/listed elsewhere/)
  })

  it('moves a locked royalty item into a new kiosk made for the recipient', () => {
    const tx = buildSendNfts(SENDER, TO, [locked(1)])
    expect(calls(tx)).toEqual([
      'kiosk::new',
      'kiosk::list_with_purchase_cap',
      'coin::zero',
      'kiosk::purchase_with_cap',
      'kiosk::lock',
      'transfer_policy::paid',
      'royalty_rule::fee_amount',
      'SplitCoins',
      'royalty_rule::pay',
      'kiosk_lock_rule::prove',
      'transfer_policy::confirm_request',
      'kiosk::set_owner_custom',
      'transfer::public_share_object',
      'TransferObjects',
    ])
    // the new kiosk's cap goes to the recipient
    const all = commands(tx)
    const last = all[all.length - 1].TransferObjects!
    const addr = tx.getData().inputs[(last.address as { Input: number }).Input]
    expect(normalizeSuiAddress(toHex(fromBase64((addr as { Pure: { bytes: string } }).Pure.bytes)))).toBe(TO)
  })

  it('shares one recipient kiosk across locked items and borrows a personal source kiosk once', () => {
    const tx = buildSendNfts(SENDER, TO, [locked(1, ROYALTY_LOCK, PERSONAL_KIOSK), locked(2, ROYALTY_LOCK, PERSONAL_KIOSK)])
    const c = calls(tx)
    expect(c.filter((x) => x === 'kiosk::new')).toHaveLength(1)
    expect(c.filter((x) => x === 'personal_kiosk::borrow_val')).toHaveLength(1)
    expect(c.filter((x) => x === 'personal_kiosk::return_val')).toHaveLength(1)
    expect(c.filter((x) => x === 'transfer_policy::confirm_request')).toHaveLength(2)
    // 2 + 4 for the kiosks, 10 per item
    expect(c).toHaveLength(2 + 4 + 2 * 10)
  })

  it('uses a personal kiosk made with create_for when the policy needs one, proving after it is personal', () => {
    const tx = buildSendNfts(SENDER, TO, [locked(1, PERSONAL_POLICY)])
    const c = calls(tx)
    expect(c[0]).toBe('kiosk::new')
    const createFor = c.indexOf('personal_kiosk::create_for')
    expect(createFor).toBeGreaterThan(0)
    expect(c.indexOf('personal_kiosk_rule::prove')).toBeGreaterThan(createFor)
    expect(c.indexOf('transfer_policy::confirm_request')).toBeGreaterThan(createFor)
    expect(c[c.length - 1]).toBe('transfer::public_share_object')
  })

  it('buys at the floor price and withdraws the payment back to the sender', () => {
    const c = calls(buildSendNfts(SENDER, TO, [locked(1, FLOOR_POLICY)]))
    expect(c).toContain('floor_price_rule::prove')
    expect(c).toContain('kiosk::withdraw')
    expect(c.filter((x) => x === 'SplitCoins')).toHaveLength(1)
  })

  it('mixed set: every item included, one command list, all inside the limits', () => {
    const items = [wallet(1), unlocked(2), locked(3), locked(4, PERSONAL_POLICY, PERSONAL_KIOSK), wallet(5)]
    const tx = buildSendNfts(SENDER, TO, items)
    const named = namedIds(tx)
    for (const s of items) expect(named.has(s.item.nftId)).toBe(true)
    // 1 take + 2 kiosks (4 + 3) + 1 personal borrow/return (2) + locked items (10 + 7) + 1 transfer
    expect(commands(tx)).toHaveLength(1 + 4 + 3 + 2 + 10 + 7 + 1)
    expect(fits(measure(tx))).toBe(true)
  })

  it('will not send to the same account', () => {
    expect(() => buildSendNfts(SENDER, SENDER, [wallet(1)])).toThrow()
  })
})

describe('planNftSends', () => {
  const check = (items: SendItem[]) => {
    const batches = planNftSends(SENDER, TO, items)
    const seen = batches.flat().map((s) => s.item.nftId)
    expect(seen.sort()).toEqual(items.map((s) => s.item.nftId).sort())
    for (const b of batches) expect(fits(measure(buildSendNfts(SENDER, TO, b)))).toBe(true)
    // minimal: no batch could take the first item of the next one
    for (let i = 0; i + 1 < batches.length; i++) expect(fits(measure(buildSendNfts(SENDER, TO, [...batches[i], batches[i + 1][0]])))).toBe(false)
    return batches
  }

  it('keeps a normal selection in one transaction', () => {
    expect(check([wallet(1), unlocked(2), locked(3)])).toHaveLength(1)
  })

  it('splits thousands of wallet NFTs by size', () => {
    const b = check(Array.from({ length: 3000 }, (_, i) => wallet(i + 1)))
    expect(b.length).toBeGreaterThan(1)
  })

  it('splits many locked items by command count and size', () => {
    const b = check(Array.from({ length: 150 }, (_, i) => locked(i + 1)))
    expect(b.length).toBeGreaterThan(1)
    for (const batch of b) expect(measure(buildSendNfts(SENDER, TO, batch)).commands).toBeLessThanOrEqual(SUI_LIMITS.commands)
  })
})

describe('analyzeNfts over gRPC', () => {
  const cap = (capId: string, kiosk: string) => ({ objectId: capId, type: KIOSK_OWNER_CAP_TYPE, content: new Uint8Array([...bcs.Address.serialize(capId).toBytes(), ...bcs.Address.serialize(kiosk).toBytes()]) })
  const personalCap = (pcId: string, capId: string, kiosk: string) => ({
    objectId: pcId,
    type: PERSONAL_KIOSK_CAP_TYPE,
    content: new Uint8Array([...bcs.Address.serialize(pcId).toBytes(), 1, ...bcs.Address.serialize(capId).toBytes(), ...bcs.Address.serialize(kiosk).toBytes()]),
  })
  const obToken = (kiosk: string) => ({ objectId: id(0xe0), type: OB_OWNER_TOKEN_TYPE, content: new Uint8Array([...bcs.Address.serialize(id(0xe0)).toBytes(), ...bcs.Address.serialize(kiosk).toBytes(), ...bcs.Address.serialize(SENDER).toBytes()]) })
  const field = (kind: string, nft: string, extra: number[] = []) => ({ name: { type: `0x0000000000000000000000000000000000000000000000000000000000000002::kiosk::${kind}`, bcs: new Uint8Array([...bcs.Address.serialize(nft).toBytes(), ...extra]) }, valueType: NFT_T })

  const OB_KIOSK = id(0xe1)
  const fields: Record<string, ReturnType<typeof field>[]> = {
    [PLAIN_KIOSK.kioskId]: [
      field('Item', id(11)), field('Item', id(12)), field('Lock', id(12)), field('Item', id(13)), field('Listing', id(13), [1]), field('Item', id(14)), field('Lock', id(14)),
      field('Item', id(19)), field('Listing', id(19), [1]), field('Item', id(21)), field('Listing', id(21), [0]), field('Item', id(22)), field('Listing', id(22), [0]),
    ],
    [PERSONAL_KIOSK.kioskId]: [field('Item', id(15)), field('Listing', id(15), [1])],
    [OB_KIOSK]: [field('Item', id(16))],
  }
  const BLOCKED_T = '0x' + 'd4'.repeat(32) + '::sbt::Badge'
  const client: NftChainReader = {
    listOwnedObjects: async ({ type }) => ({
      objects: type === KIOSK_OWNER_CAP_TYPE ? [cap(PLAIN_KIOSK.ownerCapId, PLAIN_KIOSK.kioskId)] : type === PERSONAL_KIOSK_CAP_TYPE ? [personalCap(PERSONAL_KIOSK.personalCapId!, PERSONAL_KIOSK.ownerCapId, PERSONAL_KIOSK.kioskId)] : [obToken(OB_KIOSK)],
      hasNextPage: false,
      cursor: null,
    }),
    listDynamicFields: async ({ parentId }) => ({ dynamicFields: fields[normalizeSuiAddress(parentId)] ?? [], hasNextPage: false, cursor: null }),
    // Tradeport holds 15 (orderbook listing) and 19 (plain listing); Drip's registry has 21
    getDynamicObjectField: async ({ parentId, name }) => {
      const key = normalizeSuiAddress(toHex(name.bcs))
      if (normalizeSuiAddress(parentId) === normalizeSuiAddress(TRADEPORT_STORE) && (key === id(15) || key === id(19))) return { object: { objectId: id(0x5000 + Number(BigInt(key))) } }
      throw new Error('not found')
    },
    getDynamicField: async ({ parentId, name }) => {
      const key = normalizeSuiAddress(toHex(name.bcs))
      if (normalizeSuiAddress(parentId) === normalizeSuiAddress(TRADEPORT_ORDERBOOK_STORE) && key === id(0x5000 + 15)) return { dynamicField: { value: { bcs: new Uint8Array() } } }
      if (normalizeSuiAddress(parentId) === normalizeSuiAddress(DRIP_LISTINGS_BAG) && key === id(21)) return { dynamicField: { value: { bcs: new Uint8Array() } } }
      throw new Error('not found')
    },
    getObjects: async () => ({ objects: [] }),
    getObject: async () => ({ object: { previousTransaction: null } }),
    getTransaction: async () => ({}),
  }
  const policies = async (t: string): Promise<TransferPolicyInfo[]> =>
    t === BLOCKED_T ? [{ id: id(0xf9), isShared: true, rules: [{ type: 'disable_transfer_rule', ruleType: '0xc88b1c7df0336eb4310b7d420ab93ce63a4190b1c9d5024a8df8f4ff4b8f1760::disable_transfer_rule::Rule' }] }] : [ROYALTY_LOCK]

  it('marks what can be sent and says why the rest can not', async () => {
    const nfts = [
      { objectId: id(10), type: NFT_T },
      { objectId: id(11), type: NFT_T, kiosk: true },
      { objectId: id(12), type: NFT_T, kiosk: true },
      { objectId: id(13), type: NFT_T, kiosk: true },
      { objectId: id(14), type: BLOCKED_T, kiosk: true },
      { objectId: id(15), type: NFT_T, kiosk: true },
      { objectId: id(16), type: NFT_T, kiosk: true },
      { objectId: id(17), type: NFT_T, kiosk: true },
      { objectId: id(19), type: NFT_T, kiosk: true },
      { objectId: id(21), type: NFT_T, kiosk: true },
      { objectId: id(22), type: NFT_T, kiosk: true },
    ]
    const SBT = '0x' + 'e5'.repeat(32) + '::badge::Badge'
    nfts.push({ objectId: id(18), type: SBT })
    const hasStore = async (t: string) => t !== SBT
    const r = await analyzeNfts({ client, policies, hasStore }, SENDER, nfts)
    const ok = (n: number) => r.get(id(n))!
    expect(ok(10)).toMatchObject({ ok: true, send: { from: null } })
    expect(ok(11)).toMatchObject({ ok: true, send: { from: PLAIN_KIOSK, item: { isLocked: false } } })
    expect(ok(12)).toMatchObject({ ok: true, send: { item: { isLocked: true }, policies: [ROYALTY_LOCK] } })
    expect(ok(13)).toMatchObject({ ok: false, reason: expect.stringMatching(/can not delist from/) })
    expect(ok(14)).toMatchObject({ ok: false, reason: expect.stringMatching(/transfer rules block it/) })
    expect(ok(15)).toMatchObject({ ok: true, send: { from: PERSONAL_KIOSK, item: { market: { kind: 'tradeport', orderbook: true } } } })
    expect(ok(19)).toMatchObject({ ok: true, send: { item: { market: { kind: 'tradeport', orderbook: false } } } })
    expect(ok(21)).toMatchObject({ ok: true, send: { item: { isListed: true, market: { kind: 'drip' } } } })
    expect(ok(22)).toMatchObject({ ok: true, send: { item: { isListed: true, market: undefined } } })
    expect(ok(16)).toMatchObject({ ok: false, reason: expect.stringMatching(/OriginByte/) })
    expect(ok(17)).toMatchObject({ ok: false, reason: expect.stringMatching(/Could not find/) })
    expect(ok(18)).toMatchObject({ ok: false, reason: expect.stringMatching(/can not be transferred/) })

    // and the ones that can go build into one transaction
    const sendable = [...r.values()].flatMap((s) => (s.ok ? [s.send] : []))
    expect(sendable).toHaveLength(7)
    const tx = buildSendNfts(SENDER, TO, sendable)
    const named = namedIds(tx)
    for (const s of sendable) expect(named.has(s.item.nftId)).toBe(true)
  })
})

describe('transfer policies from trade', () => {
  const POLICY_T = `0x2::transfer_policy::TransferPolicy<${NFT_T}>`
  const policyBytes = (pid: string, rules: string[]) =>
    bcs.struct('P', { id: bcs.Address, balance: bcs.u64(), rules: bcs.vector(bcs.string()) }).serialize({ id: pid, balance: 0n, rules }).toBytes()
  const tradeRule = `${KIOSK_EXT_PACKAGE.slice(2)}::royalty_rule::Rule`
  const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })

  /** a gRPC reader that knows policy 0xf001 and, through item 12's last transaction, policy 0xf009 */
  const reader = () => {
    const getObjects = vi.fn(async ({ objectIds }: { objectIds: string[] }) => ({
      objects: objectIds.map((o) => ({ objectId: o, owner: { $kind: 'Shared' }, content: policyBytes(o, [tradeRule]) })),
    }))
    const r = {
      getObjects,
      getObject: vi.fn(async () => ({ object: { previousTransaction: 'DIGEST' } })),
      getTransaction: vi.fn(async () => ({ Transaction: { objectTypes: { [id(0xf009)]: POLICY_T, [id(0xa000)]: '0x2::kiosk::Kiosk' } } })),
    }
    return r as typeof r & NftChainReader
  }

  it('asks trade for the normalized type and returns the policy ids', async () => {
    const f = vi.fn(async () => json({ policies: [{ id: '0xf001', rules: [`0x${tradeRule}`] }] }))
    expect(await policyIdsFromTrade('https://rare.market/v1/trade/', '0x' + 'c3'.repeat(32) + '::punks::Punk', f as unknown as typeof fetch)).toEqual([id(0xf001)])
    expect((f.mock.calls[0] as unknown[])[0]).toBe(`https://rare.market/v1/trade/ext/nft/policies?type=${encodeURIComponent(NFT_T)}`)
    await expect(policyIdsFromTrade('https://rare.market/v1/trade', NFT_T, (async () => json({ error: 'x' }, 503)) as unknown as typeof fetch)).rejects.toThrow(/503/)
  })

  it('reads the policies trade names over gRPC, without touching the item history', async () => {
    const client = reader()
    const load = policyLoader(client, 'https://rare.market/v1/trade', (async () => json({ policies: [{ id: id(0xf001), rules: [] }] })) as unknown as typeof fetch)
    const out = await load(NFT_T, id(12))
    expect(out).toEqual([{ id: id(0xf001), isShared: true, rules: [{ type: 'royalty_rule', ruleType: `0x${tradeRule}` }] }])
    expect(client.getObjects).toHaveBeenCalledWith({ objectIds: [id(0xf001)], include: { content: true } })
    expect(client.getObject).not.toHaveBeenCalled()
  })

  it("falls back to the item's last transaction when trade fails or knows none", async () => {
    for (const f of [async () => json({}, 503), async () => json({ policies: [] }), async () => Promise.reject(new Error('offline'))]) {
      const client = reader()
      const out = await policyLoader(client, 'https://rare.market/v1/trade', f as unknown as typeof fetch)(NFT_T, id(12))
      expect(out.map((p) => p.id)).toEqual([id(0xf009)])
      expect(client.getObject).toHaveBeenCalledWith({ objectId: id(12), include: { previousTransaction: true } })
    }
  })
})
