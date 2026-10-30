import { afterEach, describe, expect, it, vi } from 'vitest'
import { createActivityFeed, toActivityItem, type TradeActivityItem } from './activity'

const ME = '0x' + 'a'.repeat(64)
const BOB = '0x' + 'b'.repeat(64)
const SUI = '0x' + '0'.repeat(63) + '2::sui::SUI'
const TOK = '0x' + 'd'.repeat(64) + '::magma::MAGMA'

const item = (o: Partial<TradeActivityItem> = {}): TradeActivityItem => ({
  digest: 'D1',
  timestampMs: 1_700_000_000_000,
  kind: 'receive',
  changes: [{ coinType: SUI, amount: '1500000000', symbol: 'SUI', decimals: 9 }],
  counterparty: BOB,
  status: 'success',
  ...o,
})

const ok = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { 'content-type': 'application/json' } })

describe('toActivityItem', () => {
  it('maps a receive with its sender', () => {
    expect(toActivityItem(item())).toEqual({
      digest: 'D1',
      kind: 'receive',
      changes: [{ coinType: SUI, amount: '1500000000', symbol: 'SUI', decimals: 9 }],
      summary: '1.5 SUI',
      timestamp: 1_700_000_000_000,
      status: 'success',
      counterparty: BOB,
    })
  })

  it('shows both legs of a swap, paid then received, without a counterparty', () => {
    const a = toActivityItem(
      item({
        kind: 'swap',
        counterparty: undefined,
        changes: [
          { coinType: SUI, amount: '-215194996385', symbol: 'SUI', decimals: 9 },
          { coinType: TOK, amount: '502615361461', symbol: 'MAGMA', decimals: 9 },
        ],
      }),
    )
    expect(a.kind).toBe('swap')
    expect(a.summary).toBe('215.195 SUI → 502.6154 MAGMA')
    expect(a.counterparty).toBeUndefined()
  })

  it('shows the one leg a fresh swap has so far', () => {
    const a = toActivityItem(item({ kind: 'swap', changes: [{ coinType: TOK, amount: '2000000000', symbol: 'MAGMA', decimals: 9 }] }))
    expect(a.summary).toBe('2 MAGMA')
  })

  it('maps a send and falls back to the coin name and 9 decimals', () => {
    const a = toActivityItem(item({ kind: 'send', changes: [{ coinType: TOK, amount: '-3000000000' }] }))
    expect(a).toMatchObject({ kind: 'send', summary: '3 MAGMA', counterparty: BOB })
  })

  it('reads other activity by which way coins moved', () => {
    expect(toActivityItem(item({ kind: 'other', changes: [{ coinType: SUI, amount: '-1', decimals: 9 }] })).kind).toBe('send')
    expect(toActivityItem(item({ kind: 'other', changes: [{ coinType: SUI, amount: '1', decimals: 9 }] })).kind).toBe('receive')
    expect(
      toActivityItem(
        item({
          kind: 'other',
          changes: [
            { coinType: SUI, amount: '-1', decimals: 9 },
            { coinType: TOK, amount: '1', decimals: 9 },
          ],
        }),
      ).kind,
    ).toBe('swap')
  })

  it('says how many transfers a rolled up row holds, and marks failures', () => {
    const a = toActivityItem(item({ count: 3, status: 'failure' }))
    expect(a.summary).toBe('1.5 SUI in 3 transfers')
    expect(a.count).toBe(3)
    expect(a.status).toBe('failed')
  })

  it('keeps precision for big amounts', () => {
    const a = toActivityItem(item({ changes: [{ coinType: TOK, amount: '123456789012345678901234', symbol: 'X', decimals: 9 }] }))
    expect(a.summary).toBe(`${(123456789012345.678901234).toLocaleString(undefined, { maximumFractionDigits: 4 })} X`)
  })
})

describe('createActivityFeed', () => {
  afterEach(() => vi.unstubAllGlobals())

  it("reads trade's activity endpoint and maps every item", async () => {
    const fetch = vi.fn(async () => ok({ items: [item(), item({ digest: 'D2', kind: 'send' })], cursor: 'next' }))
    const feed = createActivityFeed({ baseUrl: 'https://y.test/', fetch })
    const page = await feed.list(ME)
    expect(fetch).toHaveBeenCalledWith(`https://y.test/ext/activity/${ME}?limit=30`, expect.objectContaining({ headers: { accept: 'application/json' } }))
    expect(page.cursor).toBe('next')
    const list = page.items
    expect(list.map((a) => [a.digest, a.kind])).toEqual([
      ['D1', 'receive'],
      ['D2', 'send'],
    ])
  })

  it('calls the global fetch unbound by default', async () => {
    const g = vi.fn(async () => ok({ items: [], cursor: null }))
    vi.stubGlobal('fetch', g)
    const page = await createActivityFeed({ baseUrl: 'https://y.test' }).list(ME)
    expect(page).toEqual({ items: [], cursor: null })
    expect(g).toHaveBeenCalledTimes(1)
  })

  it("throws trade's error on a failed answer", async () => {
    const fetch = vi.fn(async () => new Response(JSON.stringify({ error: 'Activity temporarily unavailable' }), { status: 502 }))
    await expect(createActivityFeed({ fetch }).list(ME)).rejects.toThrow('Activity temporarily unavailable')
  })

  it('throws a plain error when the answer is not JSON', async () => {
    const fetch = vi.fn(async () => new Response('<html>', { status: 504 }))
    await expect(createActivityFeed({ fetch }).list(ME)).rejects.toThrow('Activity unavailable (504)')
  })

  it('throws on a malformed body', async () => {
    const fetch = vi.fn(async () => ok({ nope: true }))
    await expect(createActivityFeed({ fetch }).list(ME)).rejects.toThrow(/unavailable/)
  })

  it('passes the cursor back for the next page', async () => {
    const fetch = vi.fn(async () => ok({ items: [item()], cursor: null }))
    const page = await createActivityFeed({ baseUrl: 'https://y.test', fetch }).list(ME, 'eyJiIjoxfQ')
    expect(fetch).toHaveBeenCalledWith(`https://y.test/ext/activity/${ME}?limit=30&cursor=eyJiIjoxfQ`, expect.anything())
    expect(page.cursor).toBeNull()
  })

  it('skips the network on testnet, where trade has no history', async () => {
    const fetch = vi.fn()
    expect(await createActivityFeed({ network: 'testnet', fetch }).list(ME)).toEqual({ items: [], cursor: null })
    expect(fetch).not.toHaveBeenCalled()
  })
})
