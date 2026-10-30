import type { Insights } from './analyze'
import { knownPackage, shortId as sid } from './packages'
import { knownFee } from './fees'

/** short id that never breaks across lines (word joiners around the ellipsis) */
const shortId = (id: string) => sid(id).replace('…', '\u2060…\u2060')

export interface Risk {
  id: string
  text: string
  /** red: real danger. Everything else renders muted. */
  danger?: boolean
  /** the user must tick an extra confirmation before signing */
  requiresConfirm?: boolean
}

/** Addresses this wallet already knows, for new-recipient and lookalike checks. */
export interface AddressHistory {
  /** addresses this wallet has sent to */
  sentTo: string[]
  /** addresses that have sent to this wallet */
  receivedFrom: string[]
  /** saved contacts */
  contacts: { address: string; name: string }[]
  /** false for flows where fees go to protocol addresses (swaps, lending): skip the first-time line */
  warnNew?: boolean
}

export interface TokenFlags {
  spam: boolean
  honeypot: boolean
}

export interface RiskContext {
  /** USD per whole token */
  prices: Record<string, number | null | undefined>
  decimals: (coinType: string) => number
  symbol: (coinType: string) => string
  /** the signer's portfolio value in USD; null when unknown */
  portfolioUsd?: number | null
  /** spam/honeypot flags per coin type (received coins) */
  safety?: Record<string, TokenFlags | null | undefined>
  /** called package id -> original id */
  originals?: Record<string, string>
  /** history for recipient checks; leave out to skip them */
  addresses?: AddressHistory
  /** the signer's own address; never flagged as a recipient */
  self?: string
}

const toFloat = (amount: bigint, decimals: number) => Number(amount) / 10 ** decimals

/** USD value of a list of coin flows; null when any price is missing. */
export function flowUsd(flows: { coinType: string; amount: bigint }[], ctx: Pick<RiskContext, 'prices' | 'decimals'>): number | null {
  let sum = 0
  for (const f of flows) {
    const p = ctx.prices[f.coinType]
    if (p == null) return null
    sum += toFloat(f.amount, ctx.decimals(f.coinType)) * p
  }
  return sum
}

/** USD of the flows that have a price; unpriced coins count as 0. */
export function pricedUsd(flows: { coinType: string; amount: bigint }[], ctx: Pick<RiskContext, 'prices' | 'decimals'>): number {
  return flows.reduce((s, f) => s + (ctx.prices[f.coinType] != null ? toFloat(f.amount, ctx.decimals(f.coinType)) * ctx.prices[f.coinType]! : 0), 0)
}

const norm = (a: string) => a.trim().toLowerCase()
const ends = (a: string) => ({ head: norm(a).replace(/^0x/, '').slice(0, 4), tail: norm(a).slice(-4) })
/** `0xabcd…1234`: the first and last 4 hex chars, as the user would compare them */
export const lookalikeShort = (a: string) => `0x${ends(a).head}\u2026${ends(a).tail}`

/** Checks on addresses that would receive coins or objects: lookalikes of known addresses, first-time recipients, saved names. */
export function addressChecks(recipients: string[], h: AddressHistory | undefined, self?: string): Risk[] {
  if (!h) return []
  const out: Risk[] = []
  const sent = new Set(h.sentTo.map(norm))
  const contacts = new Map(h.contacts.map((c) => [norm(c.address), c.name]))
  const known = new Set([...sent, ...h.receivedFrom.map(norm), ...contacts.keys()])
  for (const raw of new Set(recipients.map(norm))) {
    if (self && raw === norm(self)) continue
    const e = ends(raw)
    const twin = [...known].find((k) => k !== raw && ends(k).head === e.head && ends(k).tail === e.tail)
    const name = contacts.get(raw)
    if (twin) {
      out.push({ id: `lookalike-${raw}`, danger: true, requiresConfirm: true, text: `Looks like ${lookalikeShort(twin)} you've used before, but it's a different address.` })
    } else if (name) {
      out.push({ id: `contact-${raw}`, text: `Sending to ${name}, a saved contact.` })
    } else if (h.warnNew !== false && !sent.has(raw)) {
      out.push({ id: `new-recipient-${raw}`, text: 'First time sending to this address.' })
    }
  }
  return out
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`

/** Plain risk lines for the review screen, most serious first. */
export function riskChecks(ins: Insights, ctx: RiskContext): Risk[] {
  const out: Risk[] = []
  if (!ins.ok) out.push({ id: 'failed', danger: true, text: 'Simulation failed. If sent, it fails and still costs a fee.' })

  // giving away objects to another address is the classic drainer pattern
  const given = ins.objectsOut.filter((o) => o.kind === 'sent')
  if (given.length) {
    const to = [...new Set(given.map((o) => o.to!))]
    out.push({ id: 'gives-objects', danger: true, text: `Gives ${plural(given.length, 'object')} to ${to.length === 1 ? shortId(to[0]) : `${to.length} other addresses`}.` })
  }

  // large share of the portfolio leaving
  const outUsd = pricedUsd(ins.send, ctx)
  if (ctx.portfolioUsd && ctx.portfolioUsd > 0 && outUsd / ctx.portfolioUsd > 0.5) {
    const pct = Math.min(100, Math.round((outUsd / ctx.portfolioUsd) * 100))
    out.push({ id: 'large-share', danger: true, text: `Sends about ${pct}% of your portfolio.` })
  }

  for (const r of ins.receive) {
    const s = ctx.safety?.[r.coinType]
    if (s?.honeypot) out.push({ id: `honeypot-${r.coinType}`, danger: true, text: `${ctx.symbol(r.coinType)} may be a honeypot. You may not be able to sell it.` })
    else if (s?.spam) out.push({ id: `spam-${r.coinType}`, text: `${ctx.symbol(r.coinType)} is flagged as spam.` })
  }
  for (const r of ins.send) {
    const s = ctx.safety?.[r.coinType]
    if (s?.spam || s?.honeypot) out.push({ id: `spam-out-${r.coinType}`, text: `${ctx.symbol(r.coinType)} is flagged as ${s.honeypot ? 'a honeypot' : 'spam'}.` })
  }

  // coins to other addresses (protocol fees, payments): worth seeing, not alarming
  // Rare's own fee wallets show as a fee line, not as "sends coins to someone"
  const coinTo = [...new Set(ins.othersReceive.filter((o) => !knownFee(o.address)).map((o) => o.address))]
  if (coinTo.length) out.push({ id: 'pays-others', text: `Sends coins to ${coinTo.length === 1 ? shortId(coinTo[0]) : `${coinTo.length} other addresses`}, not you.` })

  // lookalikes first: they are the address-poisoning tell
  const addr = addressChecks(ins.recipients.filter((r) => !knownFee(r)), ctx.addresses, ctx.self)
  const bad = addr.filter((r) => r.danger)
  out.unshift(...bad)
  const unknown = [...new Set(ins.calls.map((c) => c.package))].filter((p) => !knownPackage(p, ctx.originals))
  if (unknown.length)
    out.push({ id: 'unknown-package', text: unknown.length === 1 ? `Uses a contract Rare Wallet doesn't recognize: ${shortId(unknown[0])}.` : `Uses ${unknown.length} contracts Rare Wallet doesn't recognize.` })

  const moved = ins.objectsOut.filter((o) => o.kind !== 'sent')
  if (moved.length) out.push({ id: 'objects-leave', text: `${plural(moved.length, 'object')} leave${moved.length === 1 ? 's' : ''} your wallet.` })
  out.push(...addr.filter((r) => !r.danger))
  if (ins.publishes) out.push({ id: 'publish', text: 'Publishes or upgrades a Move package.' })
  if (!ins.feePaidBySigner) out.push({ id: 'sponsored', text: 'Someone else pays the network fee.' })
  return out
}
