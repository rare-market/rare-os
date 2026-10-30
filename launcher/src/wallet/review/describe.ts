import { formatAmount } from '../tx/amount'
import { normType, SUI_TYPE, type Insights, type ObjectFlow } from './analyze'
import { objectLabel, shortId } from './packages'

export interface CoinMeta {
  symbol: string
  decimals: number
  iconUrl?: string | null
}

/** Coin metadata by coin type (any spelling of the address), or a lookup function. */
export type CoinMetaLookup = Record<string, CoinMeta | undefined> | ((coinType: string) => CoinMeta | undefined)

export interface ReviewRow {
  kind: 'send' | 'receive' | 'fee'
  label: 'You send' | 'You receive' | 'Network fee'
  /** "1.5 SUI", "Popkins #8475", "0.0019 SUI" */
  text: string
  /** small second line: where an object goes, "Paid by someone else" */
  sub?: string
  coinType?: string
  /** absolute base units for coin rows and the fee */
  amount?: bigint
  iconUrl?: string | null
  objectId?: string
  objectType?: string | null
  image?: string
}

const FALLBACK_SUI: CoinMeta = { symbol: 'SUI', decimals: 9 }

function lookup(meta: CoinMetaLookup, coinType: string): CoinMeta {
  const t = normType(coinType)
  const hit = typeof meta === 'function' ? meta(t) ?? meta(coinType) : meta[t] ?? meta[coinType] ?? meta[t.replace(/^0x0*/, '0x')]
  if (hit) return hit
  if (t === SUI_TYPE) return FALLBACK_SUI
  return { symbol: t.split('::').pop() ?? '?', decimals: 9 }
}

function objectRow(kind: 'send' | 'receive', o: ObjectFlow, display?: Record<string, { name?: string; image?: string }>, originals?: Record<string, string>): ReviewRow {
  const d = display?.[o.objectId]
  const title = d?.name ?? objectLabel(o.type, originals).title
  const sub =
    o.kind === 'sent' && o.to
      ? `To ${shortId(o.to)}`
      : o.kind === 'into-object'
        ? 'Into another object'
        : o.kind === 'wrapped'
          ? 'Held by the app'
          : o.kind === 'deleted'
            ? 'Destroyed'
            : o.kind === 'shared'
              ? 'Made public'
              : o.kind === 'frozen'
                ? 'Frozen'
                : o.created
                  ? 'New'
                  : undefined
  return { kind, label: kind === 'send' ? 'You send' : 'You receive', text: title, sub, objectId: o.objectId, objectType: o.type, image: d?.image }
}

/** Display rows for the review sheet: what leaves, what arrives, and the network fee. */
export function describeReview(
  ins: Insights,
  coinMeta: CoinMetaLookup = {},
  extras: {
    display?: Record<string, { name?: string; image?: string }>
    originals?: Record<string, string>
    /** the gas owner is Rare's fee sponsor (rare.market) */
    coveredByRare?: boolean
    /** who that sponsorship is paid by: Rare, or a game's gas budget ("Network fee: Paid by <game>") */
    paidBy?: string
  } = {},
): ReviewRow[] {
  const coin = (kind: 'send' | 'receive', coinType: string, amount: bigint): ReviewRow => {
    const m = lookup(coinMeta, coinType)
    return { kind, label: kind === 'send' ? 'You send' : 'You receive', text: `${formatAmount(amount, m.decimals)} ${m.symbol}`, coinType, amount, iconUrl: m.iconUrl ?? null }
  }
  const rows: ReviewRow[] = [
    ...ins.send.map((f) => coin('send', f.coinType, f.amount)),
    ...ins.objectsOut.map((o) => objectRow('send', o, extras.display, extras.originals)),
    ...ins.receive.map((f) => coin('receive', f.coinType, f.amount)),
    ...ins.objectsIn.map((o) => objectRow('receive', o, extras.display, extras.originals)),
  ]
  const sui = lookup(coinMeta, SUI_TYPE)
  const net = ins.fee.net
  if (!ins.feePaidBySigner && extras.coveredByRare) rows.push({ kind: 'fee', label: 'Network fee', text: `Paid by ${extras.paidBy || 'Rare'}`, coinType: SUI_TYPE, amount: 0n })
  else if (!ins.feePaidBySigner) rows.push({ kind: 'fee', label: 'Network fee', text: `0 ${sui.symbol}`, sub: 'Paid by someone else', coinType: SUI_TYPE, amount: 0n })
  else if (net < 0n) rows.push({ kind: 'fee', label: 'Network fee', text: `${formatAmount(-net, sui.decimals)} ${sui.symbol} back`, sub: 'Storage refund is larger than the fee', coinType: SUI_TYPE, amount: net })
  else rows.push({ kind: 'fee', label: 'Network fee', text: `${formatAmount(net, sui.decimals)} ${sui.symbol}`, coinType: SUI_TYPE, amount: net })
  return rows
}
