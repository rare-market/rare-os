import { describe, expect, it } from 'vitest'
import { analyze, normType, SUI_TYPE } from './analyze'
import { describeReview } from './describe'
import { reviewTransaction } from './index'
import type { SimObjectChange, Simulation } from './types'

const ME = '0x' + 'a1'.repeat(32)
const BOB = '0x' + 'b2'.repeat(32)
const SPONSOR = '0x' + 'c3'.repeat(32)
const MARKET = '0x' + 'd4'.repeat(32)
const GAS = '0x' + '11'.repeat(32)
const NFT = '0x' + '22'.repeat(32)
const USDC = normType('0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC')
const NFT_T = '0x' + 'e5'.repeat(32) + '::popkins::Popkins'
const own = (address: string) => ({ kind: 'address' as const, address })
const obj = (o: Partial<SimObjectChange> & { objectId: string }): SimObjectChange => ({
  type: NFT_T,
  inputOwner: own(ME),
  outputOwner: own(ME),
  idOperation: 'None',
  inputExists: true,
  outputExists: true,
  outputKind: 'object',
  ...o,
})
// fee: 1_000_000 + 2_000_000 - 1_500_000 = 1_500_000 MIST
const FEE = 1_500_000n
const sim = (o: Partial<Simulation>): Simulation => ({
  source: 'node',
  status: 'success',
  balanceChanges: [],
  objects: [],
  gas: { computation: '1000000', storage: '2000000', rebate: '1500000' },
  gasOwner: ME,
  gasObjectId: GAS,
  ...o,
})
const META = { [SUI_TYPE]: { symbol: 'SUI', decimals: 9 }, [USDC]: { symbol: 'USDC', decimals: 6 } }
const rowText = (rows: ReturnType<typeof describeReview>) => rows.map((r) => `${r.label}: ${r.text}${r.sub ? ` (${r.sub})` : ''}`)

describe('review: coin send', () => {
  const s = sim({
    balanceChanges: [
      { coinType: '0x2::sui::SUI', address: ME, amount: String(-2_500_000_000n - FEE) },
      { coinType: '0x2::sui::SUI', address: BOB, amount: '2500000000' },
    ],
  })
  it('one SUI row out, fee on its own line, Bob as the recipient', () => {
    const ins = analyze(s, ME)
    expect(ins.send).toEqual([{ coinType: SUI_TYPE, amount: 2_500_000_000n }])
    expect(ins.receive).toEqual([])
    expect(ins.recipients).toEqual([BOB])
    expect(ins.fee.net).toBe(FEE)
    expect(rowText(describeReview(ins, META))).toEqual(['You send: 2.5 SUI', 'Network fee: 0.0015 SUI'])
  })
  it('formats tokens by their decimals', () => {
    const ins = analyze(
      sim({ balanceChanges: [{ coinType: USDC, address: ME, amount: '-12345678' }, { coinType: '0x2::sui::SUI', address: ME, amount: String(-FEE) }] }),
      ME,
    )
    expect(rowText(describeReview(ins, META))).toEqual(['You send: 12.3456 USDC', 'Network fee: 0.0015 SUI'])
  })
})

describe('review: NFTs', () => {
  it('an NFT out to another address', () => {
    const ins = analyze(sim({ balanceChanges: [{ coinType: '0x2::sui::SUI', address: ME, amount: String(-FEE) }], objects: [obj({ objectId: NFT, outputOwner: own(BOB) })] }), ME)
    expect(ins.send).toEqual([])
    expect(ins.objectsOut).toEqual([{ objectId: NFT, type: NFT_T, kind: 'sent', to: BOB }])
    expect(ins.recipients).toEqual([BOB])
    const rows = describeReview(ins, META, { display: { [NFT]: { name: 'Popkins #8475', image: 'https://x/1.png' } } })
    expect(rowText(rows)).toEqual(['You send: Popkins #8475 (To 0xb2b2…b2b2)', 'Network fee: 0.0015 SUI'])
    expect(rows[0]).toMatchObject({ objectId: NFT, image: 'https://x/1.png' })
  })

  it('a purchase: SUI out to the seller and the market, NFT in from a kiosk', () => {
    const ins = analyze(
      sim({
        balanceChanges: [
          { coinType: '0x2::sui::SUI', address: ME, amount: String(-87_000_000_000n - FEE) },
          { coinType: '0x2::sui::SUI', address: BOB, amount: '85000000000' },
          { coinType: '0x2::sui::SUI', address: MARKET, amount: '2000000000' },
        ],
        objects: [
          obj({ objectId: NFT, inputOwner: { kind: 'object', id: '0x' + '99'.repeat(32) }, outputOwner: own(ME) }),
          obj({ objectId: '0xk1', type: '0x2::kiosk::Kiosk', inputOwner: { kind: 'shared' }, outputOwner: { kind: 'shared' } }),
        ],
      }),
      ME,
    )
    expect(ins.send).toEqual([{ coinType: SUI_TYPE, amount: 87_000_000_000n }])
    expect(ins.objectsIn).toEqual([{ objectId: NFT, type: NFT_T, kind: 'received', created: false }])
    expect(ins.othersReceive.map((o) => o.address).sort()).toEqual([BOB, MARKET].sort())
    expect(ins.shared.map((s) => s.label)).toEqual(['Sui Kiosk'])
    expect(rowText(describeReview(ins, META))).toEqual(['You send: 87 SUI', 'You receive: Popkins', 'Network fee: 0.0015 SUI'])
  })
})

describe('review: kiosk items', () => {
  const MY_KIOSK = '0x' + '0a'.repeat(32)
  const NEW_KIOSK = '0x' + '0b'.repeat(32)
  const F_IN = '0x' + 'f1'.repeat(32)
  const F_OUT = '0x' + 'f2'.repeat(32)
  const FIELD_T = '0x2::dynamic_field::Field<0x2::dynamic_object_field::Wrapper<0x2::kiosk::Item>, 0x2::object::ID>'
  const moved = (outputOwner: SimObjectChange['outputOwner']) =>
    sim({
      balanceChanges: [{ coinType: '0x2::sui::SUI', address: ME, amount: String(-FEE) }],
      objects: [
        obj({ objectId: NFT, inputOwner: { kind: 'object', id: F_IN }, outputOwner }),
        obj({ objectId: F_IN, type: FIELD_T, inputOwner: { kind: 'object', id: MY_KIOSK }, outputOwner: null, outputExists: false, idOperation: 'Deleted' }),
        obj({ objectId: F_OUT, type: FIELD_T, inputOwner: null, inputExists: false, idOperation: 'Created', outputOwner: { kind: 'object', id: NEW_KIOSK } }),
        obj({ objectId: MY_KIOSK, type: '0x2::kiosk::Kiosk', inputOwner: { kind: 'shared' }, outputOwner: { kind: 'shared' } }),
      ],
    })

  it('an item leaving my kiosk for a new kiosk counts as sent out once my kiosks are known', () => {
    const s = moved({ kind: 'object', id: F_OUT })
    expect(analyze(s, ME).objectsOut).toEqual([])
    const ins = analyze(s, ME, [], { kiosks: [MY_KIOSK] })
    expect(ins.objectsOut).toEqual([{ objectId: NFT, type: NFT_T, kind: 'into-object', to: NEW_KIOSK }])
    expect(rowText(describeReview(ins, META))).toEqual(['You send: Popkins (Into another object)', 'Network fee: 0.0015 SUI'])
  })

  it("an item moved into a kiosk made for Bob (its cap goes to Bob) is a send to Bob", () => {
    const s = moved({ kind: 'object', id: F_OUT })
    s.objects.push(
      obj({ objectId: NEW_KIOSK, type: '0x2::kiosk::Kiosk', inputOwner: null, inputExists: false, idOperation: 'Created', outputOwner: { kind: 'shared' } }),
      obj({ objectId: '0x' + '0c'.repeat(32), type: '0x2::kiosk::KioskOwnerCap', inputOwner: null, inputExists: false, idOperation: 'Created', outputOwner: own(BOB) }),
    )
    const ins = analyze(s, ME, [], { kiosks: [MY_KIOSK] })
    expect(ins.objectsOut).toEqual([{ objectId: NFT, type: NFT_T, kind: 'sent', to: BOB }])
    expect(ins.recipients).toEqual([BOB])
  })

  it("a first purchase: the kiosk made here with its cap coming to me is mine, so the NFT bought into it is received", () => {
    const SELLER_KIOSK = '0x' + '5e'.repeat(32)
    const CAP = '0x' + '0c'.repeat(32)
    const s = sim({
      balanceChanges: [{ coinType: '0x2::sui::SUI', address: ME, amount: String(-21_000_000_000n - FEE) }, { coinType: '0x2::sui::SUI', address: BOB, amount: '21000000000' }],
      objects: [
        obj({ objectId: NFT, inputOwner: { kind: 'object', id: F_IN }, outputOwner: { kind: 'object', id: F_OUT } }),
        obj({ objectId: F_IN, type: FIELD_T, inputOwner: { kind: 'object', id: SELLER_KIOSK }, outputOwner: null, outputExists: false, idOperation: 'Deleted' }),
        obj({ objectId: F_OUT, type: FIELD_T, inputOwner: null, inputExists: false, idOperation: 'Created', outputOwner: { kind: 'object', id: NEW_KIOSK } }),
        obj({ objectId: SELLER_KIOSK, type: '0x2::kiosk::Kiosk', inputOwner: { kind: 'shared' }, outputOwner: { kind: 'shared' } }),
        obj({ objectId: NEW_KIOSK, type: '0x2::kiosk::Kiosk', inputOwner: null, inputExists: false, idOperation: 'Created', outputOwner: { kind: 'shared' } }),
        obj({ objectId: CAP, type: '0x2::kiosk::KioskOwnerCap', inputOwner: null, inputExists: false, idOperation: 'Created', outputOwner: own(ME) }),
      ],
    })
    const ins = analyze(s, ME)
    expect(ins.objectsIn.map((o) => o.objectId)).toContain(NFT)
    expect(ins.objectsOut).toEqual([])
  })

  it('an item taken from my kiosk and sent to Bob is a send to Bob', () => {
    const ins = analyze(moved(own(BOB)), ME, [], { kiosks: [MY_KIOSK] })
    expect(ins.objectsOut).toEqual([{ objectId: NFT, type: NFT_T, kind: 'sent', to: BOB }])
    expect(ins.recipients).toEqual([BOB])
  })

  it('moving between my own kiosk and wallet is not a send', () => {
    expect(analyze(moved(own(ME)), ME, [], { kiosks: [MY_KIOSK] }).objectsOut).toEqual([])
    expect(analyze(moved(own(ME)), ME, [], { kiosks: [MY_KIOSK] }).objectsIn).toEqual([])
  })
})

describe('review: failure and sponsored gas', () => {
  it('a failed simulation says why and keeps the fee', () => {
    const ins = analyze(
      sim({
        status: 'failure',
        failure: { message: 'MoveAbort', abort: { code: '2', package: '0x2', module: 'balance', function: 'split' } },
        balanceChanges: [{ coinType: '0x2::sui::SUI', address: ME, amount: String(-FEE) }],
      }),
      ME,
    )
    expect(ins.ok).toBe(false)
    expect(ins.reason).toBe('Not enough balance.')
    expect(rowText(describeReview(ins, META))).toEqual(['Network fee: 0.0015 SUI'])
  })

  it('sponsored gas: the fee stays out of my rows and the fee row says who pays', () => {
    const ins = analyze(
      sim({
        gasOwner: SPONSOR,
        balanceChanges: [
          { coinType: USDC, address: ME, amount: '-5000000' },
          { coinType: USDC, address: BOB, amount: '5000000' },
          { coinType: '0x2::sui::SUI', address: SPONSOR, amount: String(-FEE) },
        ],
      }),
      ME,
    )
    expect(ins.feePaidBySigner).toBe(false)
    expect(ins.send).toEqual([{ coinType: USDC, amount: 5_000_000n }])
    expect(rowText(describeReview(ins, META))).toEqual(['You send: 5 USDC', 'Network fee: 0 SUI (Paid by someone else)'])
  })

  it('a storage refund larger than the cost reads as SUI back', () => {
    const ins = analyze(sim({ gas: { computation: '1000000', storage: '1000000', rebate: '9000000' }, balanceChanges: [{ coinType: '0x2::sui::SUI', address: ME, amount: '7000000' }] }), ME)
    expect(rowText(describeReview(ins, META))).toEqual(['Network fee: 0.007 SUI back (Storage refund is larger than the fee)'])
  })

  it('unknown coins fall back to the type name and 9 decimals', () => {
    const X = normType('0xabc::x::XCOIN')
    const ins = analyze(sim({ balanceChanges: [{ coinType: X, address: ME, amount: '1500000000' }, { coinType: '0x2::sui::SUI', address: ME, amount: String(-FEE) }] }), ME)
    expect(rowText(describeReview(ins))[0]).toBe('You receive: 1.5 XCOIN')
  })
})

describe('reviewTransaction', () => {
  const sdk = {
    $kind: 'Transaction' as const,
    Transaction: {
      status: { success: true, error: null },
      balanceChanges: [
        { coinType: '0x2::sui::SUI', address: ME, amount: String(-1_000_000_000n - FEE) },
        { coinType: '0x2::sui::SUI', address: BOB, amount: '1000000000' },
      ],
      objectTypes: { [GAS]: '0x2::coin::Coin<0x2::sui::SUI>' },
      effects: {
        gasUsed: { computationCost: '1000000', storageCost: '2000000', storageRebate: '1500000', nonRefundableStorageFee: '0' },
        gasObject: { objectId: GAS, inputOwner: { $kind: 'AddressOwner', AddressOwner: ME } },
        changedObjects: [],
      },
    },
  }

  it('simulates the given bytes and returns rows, risks and the same bytes', async () => {
    const seen: Uint8Array[] = []
    const client = {
      simulateTransaction: async ({ transaction }: { transaction: Uint8Array }) => (seen.push(transaction), sdk),
      getCoinMetadata: async () => ({ coinMetadata: { decimals: 9, symbol: 'SUI', name: 'Sui' } }),
    }
    const bytes = new Uint8Array([9, 9, 9])
    const r = await reviewTransaction(bytes, ME, { client: client as never, trade: null, addresses: { sentTo: [], receivedFrom: [], contacts: [] } })
    expect(seen).toEqual([bytes])
    expect(r.bytes).toBe(bytes)
    expect(r.ok).toBe(true)
    expect(rowText(r.rows)).toEqual(['You send: 1 SUI', 'Network fee: 0.0015 SUI'])
    expect(r.risks.map((x) => x.id)).toEqual(["pays-others", `new-recipient-${BOB}`])
    expect(r.coins[SUI_TYPE]).toMatchObject({ symbol: 'SUI', decimals: 9, priceUsd: null })
  })

  it('uses trade prices for the portfolio-share check when it answers', async () => {
    const client = { simulateTransaction: async () => sdk }
    const trade = { tokenDetail: async () => ({ symbol: 'SUI', decimals: 9, priceUsd: 2, logo: null, safety: null }) }
    const r = await reviewTransaction(new Uint8Array([1]), ME, { client: client as never, trade: trade as never, portfolioUsd: 3 })
    expect(r.coins[SUI_TYPE].priceUsd).toBe(2)
    expect(r.risks.find((x) => x.id === 'large-share')?.text).toBe('Sends about 67% of your portfolio.')
  })

  it('a transaction that fails during build is a failed review with no bytes, not a crash', async () => {
    const err = Object.assign(new Error('Transaction resolution failed: InsufficientCoinBalance in command 0'), {
      name: 'SimulationError',
      executionError: { $kind: 'InsufficientCoinBalance', message: 'InsufficientCoinBalance in command 0', command: 0 },
    })
    const tx = { setSenderIfNotSet: () => {}, build: async () => Promise.reject(err) }
    const r = await reviewTransaction(tx as never, ME, { client: { simulateTransaction: async () => sdk } as never, trade: null })
    expect(r.ok).toBe(false)
    expect(r.bytes).toBeNull()
    expect(r.rows).toEqual([])
    expect(r.insights.reason).toBe('Not enough balance.')
    expect(r.risks.map((x) => x.id)).toEqual(['failed'])
    const down = { setSenderIfNotSet: () => {}, build: async () => Promise.reject(new Error('fetch failed')) }
    await expect(reviewTransaction(down as never, ME, { client: {} as never, trade: null })).rejects.toThrow('fetch failed')
  })

  it('still reviews when trade is down', async () => {
    const client = { simulateTransaction: async () => sdk }
    const trade = { tokenDetail: async () => Promise.reject(new Error('down')) }
    const r = await reviewTransaction(new Uint8Array([1]), ME, { client: client as never, trade: trade as never })
    expect(r.ok).toBe(true)
    expect(r.coins[SUI_TYPE].priceUsd).toBeNull()
  })
})
