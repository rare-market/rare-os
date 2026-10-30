// Mainnet dry run, simulate only: LIVE=1 npx vitest run src/wallet/review/live.test.ts
// Builds sends FROM a real funded watch address TO a throwaway key made here, then reviews them.
// Nothing is signed for that sender and nothing is executed.

import { Ed25519Keypair } from '@mysten/sui/keypairs/ed25519'
import { describe, expect, it } from 'vitest'
import { activity } from '../data/activity'
import { portfolio } from '../data/balances'
import { nameOf } from '../data/names'
import { nftDetail } from '../data/nfts'
import { activityDetail } from '../data/txdetail'
import { grpcClient, SUI_TYPE } from '../grpc'
import { buildCoinSend } from '../tx/coin'
import { buildNftSend, hasStore, nftSendability } from '../tx/nftSend'
import { reviewTransaction, type Review } from './index'

const WATCH = '0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e'
const USDC = '0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC'
const rareGet = async <T,>(p: string): Promise<T> => {
  const r = await fetch(`https://rare.market${p}`)
  if (!r.ok) throw new Error(`rare.market ${r.status}`)
  return (await r.json()) as T
}
const show = (title: string, r: Review) => {
  console.log(`\n== ${title}: ok=${r.ok}${r.insights.reason ? ` reason="${r.insights.reason}"` : ''} bytes=${r.bytes?.length ?? 'none'}`)
  for (const row of r.rows) console.log(`  ${row.label}: ${row.text}${row.sub ? ` (${row.sub})` : ''}`)
  for (const k of r.risks) console.log(`  risk${k.danger ? ' [danger]' : ''}: ${k.text}`)
}

describe.skipIf(!process.env.LIVE)('mainnet dry run (simulate only)', () => {
  const to = new Ed25519Keypair().toSuiAddress()

  it('SUI send, SUI max, USDC send, too-large send', async () => {
    console.log(`throwaway recipient ${to}`)
    const a = await reviewTransaction(await buildCoinSend({ sender: WATCH, to, coinType: SUI_TYPE, amount: 100_000_000n }), WATCH, { addresses: { sentTo: [], receivedFrom: [], contacts: [] } })
    show('0.1 SUI', a)
    expect(a.ok).toBe(true)
    expect(a.rows[0]).toMatchObject({ label: 'You send', text: '0.1 SUI' })

    const m = await reviewTransaction(await buildCoinSend({ sender: WATCH, to, coinType: SUI_TYPE, amount: 0n, max: true }), WATCH)
    show('max SUI', m)
    expect(m.ok).toBe(true)

    const u = await buildCoinSend({ sender: WATCH, to, coinType: USDC, amount: 10_000n })
      .then((tx) => reviewTransaction(tx, WATCH))
      .catch((e) => (console.log('\nUSDC send:', String(e)), null))
    if (u) show('0.1 USDC', u)

    // passes our balance check (balance given) but not the chain: shows the failure path
    const big = await reviewTransaction(await buildCoinSend({ sender: WATCH, to, coinType: SUI_TYPE, amount: 10_000_000_000_000n, balance: 10n ** 18n }), WATCH)
    show('too much SUI', big)
    expect(big.ok).toBe(false)
    expect(big.bytes).toBeNull()
  }, 60_000)

  it('NFT send of one kiosk item', async () => {
    const page = await rareGet<{ data: Array<{ id: string; type: string; kioskId?: string | null }> }>(`/v1/accounts/${WATCH}/items?limit=5`)
    const pick = page.data[0]
    if (!pick) return console.log('no NFTs on the watch address')
    const s = await nftSendability(WATCH, [{ objectId: pick.id, type: pick.type, kiosk: !!pick.kioskId }])
    const one = s.get(pick.id)!
    console.log(`\nnft ${pick.id} kiosk=${!!pick.kioskId} sendable=${one.ok}${one.ok ? '' : ` reason="${one.reason}"`}`)
    if (one.ok) {
      const r = await reviewTransaction(buildNftSend({ sender: WATCH, to, items: [one.send] }), WATCH)
      show('NFT send', r)
      expect(r.ok).toBe(true)
      expect(r.rows[0]).toMatchObject({ label: 'You send', objectId: pick.id })
    }
    const d = await nftDetail(pick.id, WATCH, { rareGet, getObject: (o) => grpcClient.core.getObject(o) as never, hasStore: (t) => hasStore(t) })
    console.log(`nftDetail: ${d.name} owner=${d.owner.kind} inKiosk=${d.inKiosk} hasStore=${d.hasStore} canSend=${d.canSend}`)
  }, 60_000)

  it('portfolio, activity, detail, name', async () => {
    const p = await portfolio(WATCH)
    console.log(`\nportfolio total=$${p.totalUsd?.toFixed(2)} 24h=$${p.change24hUsd?.toFixed(2)} tokens=${p.tokens.length} first=${p.tokens.slice(0, 3).map((t) => `${t.symbol}:${t.amount}@${t.priceUsd}`).join(', ')}`)
    expect(p.tokens[0].symbol).toBe('SUI')
    const page = await activity(WATCH).catch((e) => (console.log(`activity failed: ${String(e)}`), { items: [], cursor: null }))
    console.log(`activity ${page.items.length} items, cursor=${!!page.cursor}; first: ${page.items[0]?.kind} ${page.items[0]?.summary}`)
    const next = page.cursor ? await activity(WATCH, page.cursor).catch((e) => (console.log(`activity page 2 failed: ${String(e)}`), null)) : null
    if (next) console.log(`activity page 2: ${next.items.length} items`)
    const d = page.items[0] ? await activityDetail(page.items[0].digest, WATCH) : null
    console.log(`detail: ${d ? `${d.status} flows=${JSON.stringify(d.flows)} fee=${d.feeMist}` : 'pruned'}`)
    console.log(`nameOf(watch)=${await nameOf(WATCH)}`)
  }, 60_000)
})
