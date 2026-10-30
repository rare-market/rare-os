import { describe, expect, it } from 'vitest'
import type { Insights } from './analyze'
import { knownFee, splitFees } from './fees'
import { riskChecks, type AddressHistory, type RiskContext } from './risk'

const ROUTER_FEE = '0x1dd4889f1ba614d0b44bd0fda31dedbb734d4edf8f25254f7d6b69671903ff06'
const STRANGER = '0x' + '77'.repeat(32)
const SUI = '0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI'

/** A 100 SUI swap: 0.2 SUI router fee to the fee wallet, plus optionally coins to a stranger. */
const swap = (extra: Insights['othersReceive'] = []): Insights => {
  const othersReceive = [{ address: ROUTER_FEE, coinType: SUI, amount: 200_000_000n }, ...extra]
  return {
    ok: true, send: [{ coinType: SUI, amount: 100_000_000_000n }], receive: [], othersReceive, objectsOut: [], objectsIn: [], shared: [], publishes: false,
    fee: { computation: 0n, storage: 0n, rebate: 0n, net: 0n }, feePaidBySigner: true, calls: [], recipients: othersReceive.map((o) => o.address),
  }
}
const hist: AddressHistory = { sentTo: [], receivedFrom: [], contacts: [] }
const ctx: RiskContext = { prices: {}, decimals: () => 9, symbol: () => 'SUI', addresses: hist }

describe("Rare's own fee recipients", () => {
  it('knows the router fee wallet, in any address spelling', () => {
    expect(knownFee(ROUTER_FEE)?.label).toBe('Rare fee')
    expect(knownFee(ROUTER_FEE.toUpperCase().replace('0X', '0x'))?.label).toBe('Rare fee')
    expect(knownFee(STRANGER)).toBeNull()
  })

  it('shows the swap fee as "Rare fee 0.2%", not as a recipient', () => {
    const { fees, others } = splitFees(swap())
    expect(fees).toEqual([{ address: ROUTER_FEE, label: 'Rare fee', coinType: SUI, amount: 200_000_000n, pct: '0.2%' }])
    expect(others).toEqual([])
  })

  it('no "sends coins to" or first-time warning for the fee wallet alone', () => {
    const ids = riskChecks(swap(), ctx).map((r) => r.id)
    expect(ids).not.toContain('pays-others')
    expect(ids.some((i) => i.startsWith('new-recipient'))).toBe(false)
  })

  it('unknown recipients keep the full warning next to the fee', () => {
    const ins = swap([{ address: STRANGER, coinType: SUI, amount: 5_000_000_000n }])
    const { fees, others } = splitFees(ins)
    expect(fees).toHaveLength(1)
    expect(others.map((o) => o.address)).toEqual([STRANGER])
    const r = riskChecks(ins, ctx)
    expect(r.find((x) => x.id === 'pays-others')?.text).toMatch(/^Sends coins to 0x7777.+7777, not you\.$/)
    expect(r.map((x) => x.id)).toContain(`new-recipient-${STRANGER}`)
  })

  it('formats other fee shares', () => {
    const at = (fee: bigint) => splitFees({ ...swap(), othersReceive: [{ address: ROUTER_FEE, coinType: SUI, amount: fee }] }).fees[0].pct
    expect(at(1_000_000_000n)).toBe('1%')
    expect(at(250_000_000n)).toBe('0.25%')
    expect(splitFees({ send: [], othersReceive: [{ address: ROUTER_FEE, coinType: SUI, amount: 1n }] }).fees[0].pct).toBeNull()
  })
})
