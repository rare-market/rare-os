import { describe, expect, it } from 'vitest'
import { addressChecks, riskChecks, type AddressHistory, type RiskContext } from './risk'
import type { Insights } from './analyze'

const mk = (head: string, tail: string, mid = '0') => '0x' + head + mid.repeat(64 - 8) + tail
const KNOWN = mk('abcd', '1234')
const FAKE = mk('abcd', '1234', '7')
const NEW = mk('9999', '8888')
const hist = (o: Partial<AddressHistory> = {}): AddressHistory => ({ sentTo: [], receivedFrom: [], contacts: [], ...o })

describe('addressChecks', () => {
  it('flags a first-time recipient plainly', () => {
    const r = addressChecks([NEW], hist({ sentTo: [KNOWN] }))
    expect(r).toEqual([{ id: `new-recipient-${NEW}`, text: 'First time sending to this address.' }])
  })
  it('is quiet for an address already sent to', () => {
    expect(addressChecks([KNOWN], hist({ sentTo: [KNOWN.toUpperCase().replace('0X', '0x')] }))).toEqual([])
  })
  it('marks a lookalike of a sent-to address as danger and asks for a confirm', () => {
    const [r] = addressChecks([FAKE], hist({ sentTo: [KNOWN] }))
    expect(r.danger).toBe(true)
    expect(r.requiresConfirm).toBe(true)
    expect(r.text).toBe("Looks like 0xabcd…1234 you've used before, but it's a different address.")
  })
  it('compares against received-from addresses and contacts too', () => {
    expect(addressChecks([FAKE], hist({ receivedFrom: [KNOWN] }))[0].danger).toBe(true)
    expect(addressChecks([FAKE], hist({ contacts: [{ address: KNOWN, name: 'Alice' }] }))[0].danger).toBe(true)
  })
  it('needs both the first 4 and last 4 to match', () => {
    expect(addressChecks([mk('abcd', '9999')], hist({ sentTo: [KNOWN] }))[0].danger).toBeUndefined()
    expect(addressChecks([mk('abce', '1234')], hist({ sentTo: [KNOWN] }))[0].danger).toBeUndefined()
  })
  it('shows the contact name instead of the first-time line', () => {
    const r = addressChecks([NEW], hist({ contacts: [{ address: NEW, name: 'Alice' }] }))
    expect(r).toEqual([{ id: `contact-${NEW}`, text: 'Sending to Alice, a saved contact.' }])
  })
  it('skips the first-time line when the flow pays protocol fees, but still catches lookalikes', () => {
    expect(addressChecks([NEW], hist({ warnNew: false }))).toEqual([])
    expect(addressChecks([FAKE], hist({ sentTo: [KNOWN], warnNew: false }))[0].danger).toBe(true)
  })
  it('never flags the signer and does nothing without history', () => {
    expect(addressChecks([NEW], hist(), NEW)).toEqual([])
    expect(addressChecks([FAKE], undefined)).toEqual([])
  })
})

describe('riskChecks with addresses', () => {
  const ins = (recipients: string[]): Insights => ({
    ok: true, send: [], receive: [], othersReceive: [], objectsOut: [], objectsIn: [], shared: [], publishes: false,
    fee: { computation: 0n, storage: 0n, rebate: 0n, net: 0n }, feePaidBySigner: true, calls: [], recipients,
  })
  const ctx = (addresses?: AddressHistory): RiskContext => ({ prices: {}, decimals: () => 9, symbol: () => 'X', addresses })
  it('puts the lookalike first so dApp approvals show it at the top', () => {
    const r = riskChecks(ins([FAKE]), ctx(hist({ sentTo: [KNOWN] })))
    expect(r[0].id).toBe(`lookalike-${FAKE}`)
    expect(r[0].requiresConfirm).toBe(true)
  })
  it('adds nothing when no history is given', () => {
    expect(riskChecks(ins([FAKE]), ctx())).toEqual([])
  })
  it('a new recipient of a dApp transfer is noted', () => {
    expect(riskChecks(ins([NEW]), ctx(hist())).map((r) => r.id)).toEqual([`new-recipient-${NEW}`])
  })
})
