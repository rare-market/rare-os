// Signing review: build the transaction, simulate the exact bytes over gRPC (never executes),
// and say in plain words what leaves the wallet, what arrives, the fee and anything risky.

import { SimulationError, type SuiClientTypes } from '@mysten/sui/client'
import type { Transaction } from '@mysten/sui/transactions'
import { normalizeSuiAddress } from '@mysten/sui/utils'
import { grpcClient } from '../grpc'
import { trade as defaultTrade, type TokenDetail, type PriceClient } from '../data/prices'
import { analyze, normType, SUI_TYPE, type Insights } from './analyze'
import { describeReview, type CoinMeta, type ReviewRow } from './describe'
import { callsOf, createGrpcSimulator, mapFailure, type SimClient } from './grpc'
import { riskChecks, type AddressHistory, type Risk, type TokenFlags } from './risk'
import type { Simulation } from './types'
import { ownedKiosks, type NftChainReader } from '../tx/nft/context'

export * from './types'
export * from './packages'
export * from './analyze'
export * from './risk'
export * from './describe'
export * from './fees'
export { createGrpcSimulator, fromSdk, callsOf, mapFailure, type SimClient } from './grpc'

export interface ReviewOptions {
  /** gRPC client (tests pass a stub; a Transaction input needs a real client to build) */
  client?: SimClient
  /** price/safety source; null skips it */
  trade?: PriceClient | null
  /** wallet history for new-recipient and lookalike checks; leave out to skip them */
  addresses?: AddressHistory
  /** signer's portfolio value, for the "sends most of your portfolio" check */
  portfolioUsd?: number | null
  /** the signer's kiosk ids; read over gRPC when left out and the transaction touches object-owned items */
  kiosks?: string[]
  /** Rare's fee sponsor wallets: a gas owner among these shows "Covered by Rare" */
  sponsors?: string[]
  /** who pays when a known sponsor is the gas owner: "Rare" (default) or a game's name */
  paidBy?: string
  /** ms to wait for best-effort extras (display, metadata, prices) */
  extrasMs?: number
}

export interface ReviewCoin extends CoinMeta {
  name?: string
  priceUsd: number | null
}

export interface Review {
  ok: boolean
  insights: Insights
  risks: Risk[]
  /** "You send" / "You receive" / "Network fee" rows, ready to render */
  rows: ReviewRow[]
  /** the exact bytes that were simulated; sign these. Null when the transaction would fail
   * before it could be built (insights.reason says why): never offer Approve then. */
  bytes: Uint8Array | null
  simulation: Simulation
  /** metadata for every coin in the rows, keyed by normalized coin type */
  coins: Record<string, ReviewCoin>
}

async function settle<T>(p: Promise<T>, ms: number): Promise<T | null> {
  return Promise.race([p.catch(() => null), new Promise<null>((r) => setTimeout(() => r(null), ms))])
}

/** The SDK resolves gas with a simulation and throws SimulationError when the transaction would
 * fail. That is a review result (with the reason), not a crash. */
function buildFailure(e: unknown): Simulation | null {
  const err = e as { name?: string; message?: string; executionError?: SuiClientTypes.ExecutionError }
  const isSim = e instanceof SimulationError || err?.name === 'SimulationError'
  if (!isSim || !/Transaction resolution failed/.test(err.message ?? '')) return null
  const failure = err.executionError ? mapFailure(err.executionError) : { message: (err.message ?? '').replace(/^Transaction resolution failed: /, '') }
  return { source: 'node', status: 'failure', failure, balanceChanges: [], objects: [], gas: { computation: '0', storage: '0', rebate: '0' }, gasOwner: null }
}

/** Build (when given a Transaction), simulate and explain. Throws only when the node can't be reached or the transaction can't be built. */
export async function reviewTransaction(input: Transaction | Uint8Array, sender: string, opts: ReviewOptions = {}): Promise<Review> {
  const client = opts.client ?? (grpcClient as unknown as SimClient)
  const extrasMs = opts.extrasMs ?? 2500
  let bytes: Uint8Array | null = null
  let simulation: Simulation
  if (input instanceof Uint8Array) bytes = input
  else {
    input.setSenderIfNotSet(sender)
    try {
      bytes = await input.build({ client: client as never })
    } catch (e) {
      const failed = buildFailure(e)
      if (!failed) throw e
      simulation = failed
    }
  }
  if (bytes) simulation = await createGrpcSimulator({ client, extrasMs }).simulate(bytes)
  // items in the signer's kiosks are owned by the kiosk, so read which kiosks are theirs (best effort)
  let kiosks = opts.kiosks
  const core = (client as { core?: NftChainReader }).core
  if (!kiosks && core && simulation!.objects.some((o) => o.inputOwner?.kind === 'object' || o.outputOwner?.kind === 'object')) {
    const owned = await settle(ownedKiosks(core, sender), extrasMs * 2)
    kiosks = owned?.kiosks.map((k) => k.kioskId) ?? []
  }
  const insights = analyze(simulation!, sender, bytes ? callsOf(bytes) : [], { kiosks })

  // prices and spam flags from the Trade API, best effort
  const types = [...new Set([SUI_TYPE, ...insights.send.map((f) => f.coinType), ...insights.receive.map((f) => f.coinType)])]
  const y = opts.trade === undefined ? defaultTrade : opts.trade
  const details: Record<string, TokenDetail | null> = {}
  if (y) await Promise.all(types.map(async (t) => void (details[t] = await settle(y.tokenDetail(t), extrasMs))))

  const coins: Record<string, ReviewCoin> = {}
  for (const t of types) {
    const chain = simulation!.coins?.[t] ?? Object.entries(simulation!.coins ?? {}).find(([k]) => normType(k) === t)?.[1]
    const d = details[t]
    coins[t] = {
      symbol: chain?.symbol || d?.symbol || (t === SUI_TYPE ? 'SUI' : t.split('::').pop() ?? '?'),
      decimals: chain?.decimals ?? d?.decimals ?? 9,
      name: chain?.name ?? d?.name,
      iconUrl: chain?.iconUrl ?? d?.logo ?? null,
      priceUsd: d?.priceUsd ?? null,
    }
  }
  const safety: Record<string, TokenFlags | null> = {}
  for (const t of types) safety[t] = details[t]?.safety ?? null

  const risks = riskChecks(insights, {
    prices: Object.fromEntries(types.map((t) => [t, coins[t].priceUsd])),
    decimals: (t) => coins[normType(t)]?.decimals ?? 9,
    symbol: (t) => coins[normType(t)]?.symbol ?? t.split('::').pop() ?? '?',
    portfolioUsd: opts.portfolioUsd ?? null,
    safety,
    originals: simulation!.packages,
    addresses: opts.addresses,
    self: sender,
  })
  // no fee row for a transaction that could not even be built: there is nothing to pay for
  const gasOwner = simulation!.gasOwner ? normalizeSuiAddress(simulation!.gasOwner) : null
  const coveredByRare = !!gasOwner && gasOwner !== normalizeSuiAddress(sender) && (opts.sponsors ?? []).some((a) => normalizeSuiAddress(a) === gasOwner)
  const rows = bytes ? describeReview(insights, coins, { display: simulation!.display, originals: simulation!.packages, coveredByRare, paidBy: opts.paidBy }) : []
  return { ok: insights.ok && !!bytes, insights, risks, rows, bytes, simulation: simulation!, coins }
}
