import { normalizeStructTag } from '@mysten/sui/utils'
import { isCoinObject, normId, parseType, knownPackage, objectLabel, humanize, shortId } from './packages'
import type { Owner, SimFailure, Simulation } from './types'

export const SUI_TYPE = normalizeStructTag('0x2::sui::SUI')

export const normType = (t: string) => {
  try {
    return normalizeStructTag(t)
  } catch {
    return t
  }
}

export interface CoinFlow {
  coinType: string
  /** absolute base units */
  amount: bigint
}

export type ObjectFlowKind =
  /** owner changes to another address */
  | 'sent'
  /** becomes a child of another object (kiosk listing, vault deposit) */
  | 'into-object'
  /** wrapped inside another object */
  | 'wrapped'
  | 'deleted'
  /** made shared: anyone can use it */
  | 'shared'
  /** frozen, no one can change it again */
  | 'frozen'
  | 'received'

export interface ObjectFlow {
  objectId: string
  type: string | null
  kind: ObjectFlowKind
  /** recipient address or parent object id */
  to?: string
  created?: boolean
}

export interface SharedTouch {
  objectId: string
  type: string | null
  label: string
  sub: string
  created: boolean
}

export interface MoveCallRef {
  package: string
  module: string
  function: string
}

export interface Insights {
  ok: boolean
  /** where the simulation ran */
  source?: 'node'
  failure?: SimFailure
  /** plain words for the failure banner */
  reason?: string
  send: CoinFlow[]
  receive: CoinFlow[]
  /** positive balance changes for other addresses (who else gets coins) */
  othersReceive: { address: string; coinType: string; amount: bigint }[]
  objectsOut: ObjectFlow[]
  objectsIn: ObjectFlow[]
  shared: SharedTouch[]
  publishes: boolean
  fee: { computation: bigint; storage: bigint; rebate: bigint; net: bigint }
  feePaidBySigner: boolean
  calls: MoveCallRef[]
  /** other addresses that receive coins or objects from this tx */
  recipients: string[]
}

const big = (s: string | null | undefined) => {
  try {
    return BigInt(s ?? '0')
  } catch {
    return 0n
  }
}

const ownerAddr = (o: Owner | null): string | null => (o?.kind === 'address' ? normId(o.address) : null)

/** `0x2::dynamic_field::Field<..>`: the wrapper a kiosk keeps each item under. */
const isDynamicField = (t: string | null) => !!t && /^0x0*2::dynamic_field::Field</.test(t)

/** Classify a simulation from the signer's point of view. Coins are balance changes; Coin<T>
 * objects, the gas coin, address-balance accumulators and packages never show as objects. */
export function analyze(sim: Simulation, signer: string, calls: MoveCallRef[] = [], opts: { kiosks?: Iterable<string> } = {}): Insights {
  const me = normId(signer)
  // Kiosk items are owned by a dynamic field whose owner is the kiosk, not by the address. With
  // the signer's kiosk ids (opts.kiosks), items in them count as the signer's on either side.
  const kiosks = new Set([...(opts.kiosks ?? [])].map(normId))
  const fieldIn = new Map<string, string>()
  const fieldOut = new Map<string, string>()
  for (const o of sim.objects) {
    if (!isDynamicField(o.type)) continue
    if (o.inputOwner?.kind === 'object') fieldIn.set(normId(o.objectId), normId(o.inputOwner.id))
    if (o.outputOwner?.kind === 'object') fieldOut.set(normId(o.objectId), normId(o.outputOwner.id))
  }
  /** the kiosk (or other parent) an object-owned object sits in */
  const parentOf = (o: Owner | null, side: Map<string, string>): string | null => {
    if (o?.kind !== 'object') return null
    return side.get(normId(o.id)) ?? o.id
  }
  // a kiosk made in this transaction whose cap goes to one other address belongs to that
  // address: items moved into it are a send to them (how locked NFTs are sent)
  const newKiosks = sim.objects.filter((o) => !o.inputExists && o.outputOwner?.kind === 'shared' && /^0x0*2::kiosk::Kiosk$/.test(o.type ?? ''))
  const capTo = new Set(
    sim.objects
      .filter((o) => !o.inputExists && /^0x[0-9a-f]+::(kiosk::KioskOwnerCap|personal_kiosk::PersonalKioskCap)$/.test(o.type ?? ''))
      .map((o) => ownerAddr(o.outputOwner))
      .filter((a): a is string => !!a && a !== me),
  )
  const kioskOwner = newKiosks.length === 1 && capTo.size === 1 ? { id: normId(newKiosks[0].objectId), owner: [...capTo][0] } : null
  // a kiosk made here whose cap comes to the signer is the signer's: a first purchase creates the
  // buyer's kiosk, and the item bought into it is what they receive
  const capToMe = sim.objects.some(
    (o) => !o.inputExists && /^0x[0-9a-f]+::(kiosk::KioskOwnerCap|personal_kiosk::PersonalKioskCap)$/.test(o.type ?? '') && ownerAddr(o.outputOwner) === me,
  )
  if (capToMe && capTo.size === 0) for (const k of newKiosks) kiosks.add(normId(k.objectId))
  const mine = (o: Owner | null, side: Map<string, string>) => {
    if (ownerAddr(o) === me) return true
    const p = kiosks.size ? parentOf(o, side) : null
    return !!p && kiosks.has(normId(p))
  }
  const gas = sim.gas
  const fee = { computation: big(gas.computation), storage: big(gas.storage), rebate: big(gas.rebate), net: 0n }
  fee.net = fee.computation + fee.storage - fee.rebate
  const feePaidBySigner = !sim.gasOwner || normId(sim.gasOwner) === me

  // balance changes, summed per (address, coin)
  const sums = new Map<string, { address: string; coinType: string; amount: bigint }>()
  for (const c of sim.balanceChanges) {
    const address = normId(c.address)
    const coinType = normType(c.coinType)
    const k = `${address}|${coinType}`
    const cur = sums.get(k) ?? { address, coinType, amount: 0n }
    cur.amount += big(c.amount)
    sums.set(k, cur)
  }
  // the fee is its own line: take it out of the signer's SUI change
  if (feePaidBySigner) {
    const k = `${me}|${SUI_TYPE}`
    const cur = sums.get(k) ?? { address: me, coinType: SUI_TYPE, amount: 0n }
    cur.amount += fee.net
    sums.set(k, cur)
  }
  const send: CoinFlow[] = []
  const receive: CoinFlow[] = []
  const othersReceive: Insights['othersReceive'] = []
  for (const s of sums.values()) {
    if (s.amount === 0n) continue
    if (s.address === me) (s.amount < 0n ? send : receive).push({ coinType: s.coinType, amount: s.amount < 0n ? -s.amount : s.amount })
    else if (s.amount > 0n) othersReceive.push(s)
  }
  const bySize = (a: CoinFlow, b: CoinFlow) => (b.amount > a.amount ? 1 : b.amount < a.amount ? -1 : 0)
  send.sort(bySize)
  receive.sort(bySize)

  const objectsOut: ObjectFlow[] = []
  const objectsIn: ObjectFlow[] = []
  const shared: SharedTouch[] = []
  let publishes = false
  const gasId = sim.gasObjectId ? normId(sim.gasObjectId) : null
  for (const o of sim.objects) {
    if (o.outputKind === 'package') {
      publishes = true
      continue
    }
    if (o.outputKind === 'accumulator') continue
    if (gasId && normId(o.objectId) === gasId) continue
    const sharedIn = o.inputOwner?.kind === 'shared'
    const sharedOut = o.outputOwner?.kind === 'shared'
    if (sharedIn || (sharedOut && !o.inputExists)) {
      const l = objectLabel(o.type, sim.packages)
      shared.push({ objectId: o.objectId, type: o.type, label: l.title, sub: l.sub, created: !o.inputExists })
      continue
    }
    if (isCoinObject(o.type) || isDynamicField(o.type)) continue
    const to = ownerAddr(o.outputOwner)
    const mineOut = mine(o.outputOwner, fieldOut)
    if (o.inputExists && mine(o.inputOwner, fieldIn)) {
      if (!o.outputExists) objectsOut.push({ objectId: o.objectId, type: o.type, kind: o.idOperation === 'Deleted' ? 'deleted' : 'wrapped' })
      else if (mineOut) continue
      else if (to) objectsOut.push({ objectId: o.objectId, type: o.type, kind: 'sent', to })
      else if (o.outputOwner?.kind === 'object') {
        const parent = parentOf(o.outputOwner, fieldOut)!
        if (kioskOwner && normId(parent) === kioskOwner.id) objectsOut.push({ objectId: o.objectId, type: o.type, kind: 'sent', to: kioskOwner.owner })
        else objectsOut.push({ objectId: o.objectId, type: o.type, kind: 'into-object', to: parent })
      }
      else if (sharedOut) objectsOut.push({ objectId: o.objectId, type: o.type, kind: 'shared' })
      else if (o.outputOwner?.kind === 'immutable') objectsOut.push({ objectId: o.objectId, type: o.type, kind: 'frozen' })
    } else if (o.outputExists && mineOut) {
      objectsIn.push({ objectId: o.objectId, type: o.type, kind: 'received', created: !o.inputExists || o.idOperation === 'Created' })
    }
  }

  const recipients = [...new Set([...othersReceive.map((r) => r.address), ...objectsOut.filter((o) => o.kind === 'sent').map((o) => o.to!)])]
  const ok = sim.status === 'success'
  return {
    ok,
    source: sim.source,
    failure: sim.failure,
    reason: ok ? undefined : friendlyFailure(sim.failure),
    send,
    receive,
    othersReceive,
    objectsOut,
    objectsIn,
    shared,
    publishes,
    fee,
    feePaidBySigner,
    calls,
    recipients,
  }
}

/** Short plain-words reason for a failed simulation. */
export function friendlyFailure(f: SimFailure | undefined): string {
  if (!f) return 'Unknown error.'
  const m = f.message ?? ''
  if (/InsufficientCoinBalance/i.test(m)) return 'Not enough balance.'
  if (/InsufficientGas|lower than the needed amount|gas balance|GasBalanceTooLow|budget/i.test(m)) return 'Not enough SUI to pay the network fee.'
  if (f.abort) {
    const a = f.abort
    const where = a.module && a.function ? `${a.module}::${a.function}` : a.module ?? ''
    // the framework's own "not enough" aborts
    if (a.package && normId(a.package) === normId('0x2') && a.module === 'balance' && a.code === '2') return 'Not enough balance.'
    if (a.constant) {
      const words = humanize(a.constant.replace(/^E(?=[A-Z])/, ''))
      return `${words.charAt(0).toUpperCase()}${words.slice(1).toLowerCase()}${where ? ` (${where})` : ''}.`
    }
    const proto = a.package ? knownPackage(a.package) : null
    return `${proto ?? "The app's contract"} stopped it${where ? ` in ${where}` : ''}, code ${a.code}.`
  }
  if (/input objects|ObjectVersionUnavailable|not available for consumption|ObjectNotFound|deleted/i.test(m))
    return 'Something it uses changed or no longer exists. Ask the site to try again.'
  const clean = m.replace(/0x[0-9a-f]{20,}/gi, (id) => shortId(id))
  return clean.length > 140 ? `${clean.slice(0, 137)}…` : clean || 'Unknown error.'
}

/** Type-level package of a Move call target, used by the calls list. */
export function callLabel(c: MoveCallRef, originals?: Record<string, string>): { title: string; sub: string } {
  const proto = knownPackage(c.package, originals)
  return { title: `${c.module}::${c.function}`, sub: proto ?? shortId(c.package) }
}

export { parseType }
