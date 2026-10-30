import { describe, expect, it } from 'vitest'
import { analyze, friendlyFailure, normType, SUI_TYPE } from './analyze'
import { riskChecks, type RiskContext } from './risk'
import { createGrpcSimulator, fromSdk } from './grpc'
import { knownPackage, objectLabel, shortId } from './packages'
import type { SimObjectChange, Simulation } from './types'

const ME = '0x' + 'a1'.repeat(32)
const BOB = '0x' + 'b2'.repeat(32)
const GAS = '0x' + '11'.repeat(32)
const USDC = normType('0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC')
const CETUS = '0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb'
const own = (address: string) => ({ kind: 'address' as const, address })
const obj = (o: Partial<SimObjectChange> & { objectId: string }): SimObjectChange => ({
  type: '0xabc::nft::Nft',
  inputOwner: own(ME),
  outputOwner: own(ME),
  idOperation: 'None',
  inputExists: true,
  outputExists: true,
  outputKind: 'object',
  ...o,
})
// fee: 1000 + 5000 - 2000 = 4000 MIST
const sim = (o: Partial<Simulation>): Simulation => ({
  status: 'success',
  balanceChanges: [],
  objects: [],
  gas: { computation: '1000', storage: '5000', rebate: '2000' },
  gasOwner: ME,
  gasObjectId: GAS,
  ...o,
})
const ctx = (o: Partial<RiskContext> = {}): RiskContext => ({
  prices: { [SUI_TYPE]: 4, [USDC]: 1 },
  decimals: (t) => (t === USDC ? 6 : 9),
  symbol: (t) => t.split('::').pop()!,
  ...o,
})

describe('analyze: balance changes', () => {
  it('splits send/receive for the signer and keeps the fee out of the SUI row', () => {
    const ins = analyze(
      sim({
        balanceChanges: [
          { coinType: '0x2::sui::SUI', address: ME, amount: String(-10_000_000_000n - 4000n) },
          { coinType: USDC, address: ME, amount: '39820000' },
        ],
      }),
      ME,
    )
    expect(ins.ok).toBe(true)
    expect(ins.send).toEqual([{ coinType: SUI_TYPE, amount: 10_000_000_000n }])
    expect(ins.receive).toEqual([{ coinType: USDC, amount: 39_820_000n }])
    expect(ins.fee.net).toBe(4000n)
    expect(ins.othersReceive).toEqual([])
  })

  it('a pure fee leaves no coin rows; a storage refund larger than the cost is not a receive', () => {
    expect(analyze(sim({ balanceChanges: [{ coinType: '0x2::sui::SUI', address: ME, amount: '-4000' }] }), ME).send).toEqual([])
    const refund = analyze(sim({ gas: { computation: '1000', storage: '1000', rebate: '9000' }, balanceChanges: [{ coinType: '0x2::sui::SUI', address: ME, amount: '7000' }] }), ME)
    expect(refund.receive).toEqual([])
    expect(refund.fee.net).toBe(-7000n)
  })

  it('positive changes for other addresses are listed as recipients; sponsored gas stays in the signer rows', () => {
    const ins = analyze(
      sim({
        gasOwner: BOB,
        balanceChanges: [
          { coinType: USDC, address: ME, amount: '-5000000' },
          { coinType: USDC, address: BOB, amount: '5000000' },
          { coinType: '0x2::sui::SUI', address: BOB, amount: '-4000' },
        ],
      }),
      ME,
    )
    expect(ins.feePaidBySigner).toBe(false)
    expect(ins.send).toEqual([{ coinType: USDC, amount: 5_000_000n }])
    expect(ins.othersReceive).toEqual([{ address: BOB, coinType: USDC, amount: 5_000_000n }])
    expect(ins.recipients).toEqual([BOB])
  })
})

describe('analyze: objects', () => {
  it('classifies objects leaving, arriving, deleted, wrapped and moved into another object', () => {
    const ins = analyze(
      sim({
        objects: [
          obj({ objectId: GAS, type: '0x2::coin::Coin<0x2::sui::SUI>' }),
          obj({ objectId: '0x01', outputOwner: own(BOB) }),
          obj({ objectId: '0x02', inputOwner: null, inputExists: false, idOperation: 'Created' }),
          obj({ objectId: '0x03', outputOwner: null, outputExists: false, idOperation: 'Deleted' }),
          obj({ objectId: '0x04', outputOwner: null, outputExists: false }),
          obj({ objectId: '0x05', outputOwner: { kind: 'object', id: '0xk1' } }),
          obj({ objectId: '0x06' }), // mutated in place, stays mine
          obj({ objectId: '0x07', type: `0x2::coin::Coin<${USDC}>`, outputOwner: own(BOB) }), // coins are balance changes
          obj({ objectId: '0x08', inputOwner: own(BOB), outputOwner: own(ME) }),
          obj({ objectId: '0x09', outputKind: 'accumulator', type: '0x2::accumulator::X' }),
        ],
      }),
      ME,
    )
    expect(ins.objectsOut.map((o) => [o.objectId, o.kind, o.to])).toEqual([
      ['0x01', 'sent', BOB],
      ['0x03', 'deleted', undefined],
      ['0x04', 'wrapped', undefined],
      ['0x05', 'into-object', '0xk1'],
    ])
    expect(ins.objectsIn.map((o) => [o.objectId, o.created])).toEqual([
      ['0x02', true],
      ['0x08', false],
    ])
    expect(ins.recipients).toEqual([BOB])
  })

  it('lists shared objects touched with the protocol name, and flags publishes', () => {
    const ins = analyze(
      sim({
        objects: [
          obj({ objectId: '0xp1', type: `${CETUS}::pool::Pool<0x2::sui::SUI, ${USDC}>`, inputOwner: { kind: 'shared' }, outputOwner: { kind: 'shared' } }),
          obj({ objectId: '0xp2', type: '0x9999::vault::Vault', inputOwner: { kind: 'shared' }, outputOwner: { kind: 'shared' } }),
          obj({ objectId: '0xpk', type: null, inputOwner: null, inputExists: false, outputKind: 'package', outputOwner: { kind: 'immutable' } }),
        ],
      }),
      ME,
    )
    expect(ins.shared.map((s) => [s.label, s.sub])).toEqual([
      ['Cetus Pool', 'pool::Pool'],
      ['Vault', '0x9999::vault'],
    ])
    expect(ins.publishes).toBe(true)
  })
})

describe('friendlyFailure', () => {
  it('turns node errors into plain words', () => {
    expect(friendlyFailure({ message: 'InsufficientCoinBalance in command 0' })).toBe('Not enough balance.')
    expect(friendlyFailure({ message: 'MoveAbort ...', abort: { code: '2', package: '0x2', module: 'balance', function: 'split' } })).toBe('Not enough balance.')
    expect(friendlyFailure({ message: 'x', abort: { code: '6', package: CETUS, module: 'router', function: 'swap', constant: 'EAmountOutBelowMinLimit' } })).toBe('Amount out below min limit (router::swap).')
    expect(friendlyFailure({ message: 'x', abort: { code: '7', package: CETUS, module: 'router', function: 'swap' } })).toBe('Cetus stopped it in router::swap, code 7.')
    expect(friendlyFailure({ message: 'Error checking transaction input objects: ObjectNotFound' })).toMatch(/changed or no longer exists/)
    expect(friendlyFailure({ message: 'Balance of gas object 10 is lower than the needed amount: 100' })).toBe('Not enough SUI to pay the network fee.')
  })
})

describe('riskChecks', () => {
  const ids = (r: { id: string }[]) => r.map((x) => x.id)
  it('flags failure, given-away objects and big portfolio share as danger', () => {
    const ins = analyze(
      sim({
        status: 'failure',
        failure: { message: 'x' },
        balanceChanges: [{ coinType: '0x2::sui::SUI', address: ME, amount: String(-20_000_000_000n - 4000n) }],
        objects: [obj({ objectId: '0x01', outputOwner: own(BOB) })],
      }),
      ME,
    )
    const r = riskChecks(ins, ctx({ portfolioUsd: 100 }))
    expect(ids(r)).toEqual(['failed', 'gives-objects', 'large-share'])
    expect(r.every((x) => x.danger)).toBe(true)
    expect(r[2].text).toBe('Sends about 80% of your portfolio.')
    expect(ids(riskChecks(ins, ctx({ portfolioUsd: 1000 })))).not.toContain('large-share')
  })

  it('honeypot is danger, spam and payments to others are muted', () => {
    const SCAM = normType('0xdead::scam::SCAM')
    const ins = analyze(
      sim({
        balanceChanges: [
          { coinType: SCAM, address: ME, amount: '100' },
          { coinType: USDC, address: ME, amount: '-1000000' },
          { coinType: USDC, address: BOB, amount: '1000000' },
        ],
      }),
      ME,
    )
    const r = riskChecks(ins, ctx({ safety: { [SCAM]: { spam: false, honeypot: true } } }))
    expect(r.find((x) => x.id.startsWith('honeypot'))?.danger).toBe(true)
    expect(r.find((x) => x.id === 'pays-others')?.danger).toBeFalsy()
    const spam = riskChecks(ins, ctx({ safety: { [SCAM]: { spam: true, honeypot: false } } }))
    expect(spam.find((x) => x.id.startsWith('spam'))?.danger).toBeFalsy()
  })

  it('unknown packages are muted; upgraded known packages resolve through their original id', () => {
    const upgraded = '0x' + '77'.repeat(32)
    const ins = analyze(sim({}), ME, [
      { package: upgraded, module: 'pool_script', function: 'swap' },
      { package: '0x2', module: 'coin', function: 'split' },
    ])
    const unknown = riskChecks(ins, ctx()).find((x) => x.id === 'unknown-package')
    expect(unknown?.text).toMatch(/doesn't recognize: 0x7777/)
    expect(unknown?.danger).toBeFalsy()
    expect(ids(riskChecks(ins, ctx({ originals: { [upgraded]: CETUS } })))).not.toContain('unknown-package')
    expect(riskChecks(analyze(sim({}), ME), ctx())).toEqual([])
  })
})

describe('packages', () => {
  it('names known protocols and shortens ids', () => {
    expect(knownPackage(CETUS)).toBe('Cetus')
    expect(knownPackage('0x0000000000000000000000000000000000000000000000000000000000000002')).toBe('Sui')
    expect(shortId('0x2')).toBe('0x2')
    expect(shortId(CETUS)).toBe('0x1eab…b2fb')
    expect(objectLabel('0x3::sui_system::SuiSystemState').title).toBe('Sui System State')
  })
})

describe('grpc simulator', () => {
  const sdkResult = {
    $kind: 'FailedTransaction' as const,
    FailedTransaction: {
      status: { success: false, error: { $kind: 'MoveAbort', message: 'MoveAbort', command: 0, MoveAbort: { abortCode: '2', location: { package: '0x2', module: 'balance', functionName: 'split' } } } },
      balanceChanges: [{ coinType: '0x2::sui::SUI', address: ME, amount: '-100099' }],
      objectTypes: { [GAS]: '0x2::coin::Coin<0x2::sui::SUI>', '0xn1': '0xabc::nft::Nft' },
      effects: {
        gasUsed: { computationCost: '100000', storageCost: '988000', storageRebate: '987901', nonRefundableStorageFee: '99' },
        gasObject: { objectId: GAS, inputOwner: { $kind: 'AddressOwner', AddressOwner: ME } },
        changedObjects: [
          { objectId: GAS, inputState: 'Exists', outputState: 'ObjectWrite', idOperation: 'None', inputOwner: { $kind: 'AddressOwner', AddressOwner: ME }, outputOwner: { $kind: 'AddressOwner', AddressOwner: ME } },
          { objectId: '0xn1', inputState: 'Exists', outputState: 'ObjectWrite', idOperation: 'None', inputOwner: { $kind: 'ConsensusAddressOwner', ConsensusAddressOwner: { owner: ME, startVersion: '1' } }, outputOwner: { $kind: 'AddressOwner', AddressOwner: BOB } },
        ],
      },
    },
    gasPaymentMocked: false,
  }

  it('maps the SDK result: status, abort location, owners, gas', () => {
    const s = fromSdk(sdkResult as never)
    expect(s.status).toBe('failure')
    expect(s.failure?.abort).toEqual({ code: '2', package: '0x2', module: 'balance', function: 'split', constant: undefined })
    expect(s.gasOwner).toBe(ME)
    expect(s.objects[1]).toMatchObject({ type: '0xabc::nft::Nft', inputOwner: own(ME), outputOwner: own(BOB), inputExists: true, outputExists: true })
    expect(analyze(s, ME).reason).toBe('Not enough balance.')
  })

  it('simulates the exact bytes, adds display and coin metadata, maps rejections, rethrows network errors', async () => {
    const seen: Uint8Array[] = []
    const client = {
      simulateTransaction: async ({ transaction }: { transaction: Uint8Array }) => (seen.push(transaction), sdkResult),
      getObjects: async () => ({ objects: [{ objectId: '0xn1', display: { output: { name: 'Nft #1', image_url: 'https://x/1.png' } } }] }),
      getCoinMetadata: async () => ({ coinMetadata: { decimals: 9, symbol: 'SUI' } }),
    }
    const bytes = new Uint8Array([1, 2, 3])
    const s = await createGrpcSimulator({ client: client as never }).simulate(bytes)
    expect(seen[0]).toBe(bytes)
    expect(s.display).toEqual({ '0xn1': { name: 'Nft #1', image: 'https://x/1.png' } })
    expect(s.coins?.['0x2::sui::SUI']).toMatchObject({ decimals: 9, symbol: 'SUI' })

    const rejected = { simulateTransaction: async () => Promise.reject(Object.assign(new Error('Error checking transaction input objects: ObjectNotFound'), { code: 'INVALID_ARGUMENT' })) }
    const r = await createGrpcSimulator({ client: rejected as never }).simulate(bytes)
    expect(r.status).toBe('failure')
    expect(analyze(r, ME).reason).toMatch(/no longer exists/)

    const down = { simulateTransaction: async () => Promise.reject(Object.assign(new Error('fetch failed'), { code: 'UNAVAILABLE' })) }
    await expect(createGrpcSimulator({ client: down as never }).simulate(bytes)).rejects.toThrow('fetch failed')
  })
})
