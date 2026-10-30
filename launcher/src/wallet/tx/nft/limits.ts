/* Sui's per-transaction limits and a planner that splits a long list into the fewest
 * transactions that each stay inside them. Pure: no network, never signs. */
import { fromBase64, parseStructTag } from '@mysten/sui/utils'
import type { Transaction } from '@mysten/sui/transactions'

/** Protocol limits (mainnet protocol config), with room left for gas data and signatures. */
export const SUI_LIMITS = {
  /** max_programmable_tx_commands */
  commands: 1024,
  /** max_arguments: a TransferObjects / SplitCoins / MoveCall takes at most this many */
  args: 512,
  /** max_input_objects */
  objects: 2048,
  /** max_tx_size_bytes is 128 KiB; the rest is kept for gas coins, expiration and signatures */
  bytes: 100 * 1024,
}
export type Limits = typeof SUI_LIMITS

/** Items per TransferObjects / SplitCoins command, under SUI_LIMITS.args. */
export const ARGS_PER_COMMAND = 500

export interface TxSize {
  commands: number
  /** distinct object inputs */
  objects: number
  /** estimated BCS size of inputs + commands */
  bytes: number
}

const uleb = (n: number) => (n < 0x80 ? 1 : n < 0x4000 ? 2 : 3)
const str = (s: string) => uleb(s.length) + s.length

/** BCS size of a type tag written as a string (`0x2::sui::SUI`, `u64`, `vector<u8>`). */
export function typeTagBytes(t: string): number {
  const s = t.trim()
  if (s.startsWith('vector<')) return 1 + typeTagBytes(s.slice(7, -1))
  if (!s.includes('::')) return 1
  try {
    const tag = parseStructTag(s)
    return 1 + 32 + str(tag.module) + str(tag.name) + uleb(tag.typeParams.length) + tag.typeParams.reduce((n: number, p) => n + typeTagBytes(typeof p === 'string' ? p : `${p.address}::${p.module}::${p.name}`), 0)
  } catch {
    return 1 + 32 + s.length
  }
}

const ARG = 3
// a coinWithBalance intent turns into a few commands at build time (merge + split, or a withdrawal)
const INTENT_COMMANDS = 4

/** Commands, object inputs and an estimated size for a built-but-unresolved Transaction. */
export function measure(tx: Transaction): TxSize {
  const d = tx.getData()
  let bytes = 0
  let objects = 0
  for (const i of d.inputs) {
    if (i.$kind === 'Pure') bytes += 1 + 3 + fromBase64(i.Pure.bytes).length
    else if (i.$kind === 'UnresolvedPure') bytes += 1 + 3 + 32
    else {
      objects++
      // owned object ref (id, version, digest); shared ones are smaller
      bytes += 1 + 1 + 32 + 8 + 33
    }
  }
  let commands = 0
  for (const c of d.commands) {
    if (c.$kind === 'MoveCall') {
      commands++
      const m = c.MoveCall
      bytes += 1 + 32 + str(m.module) + str(m.function) + uleb(m.typeArguments.length) + m.typeArguments.reduce((n, t) => n + typeTagBytes(t), 0) + uleb(m.arguments.length) + m.arguments.length * ARG
    } else if (c.$kind === 'TransferObjects') {
      commands++
      bytes += 1 + uleb(c.TransferObjects.objects.length) + c.TransferObjects.objects.length * ARG + ARG
    } else if (c.$kind === 'SplitCoins') {
      commands++
      bytes += 1 + ARG + uleb(c.SplitCoins.amounts.length) + c.SplitCoins.amounts.length * ARG
    } else if (c.$kind === 'MergeCoins') {
      commands++
      bytes += 1 + ARG + uleb(c.MergeCoins.sources.length) + c.MergeCoins.sources.length * ARG
    } else if (c.$kind === '$Intent') {
      commands += INTENT_COMMANDS
      bytes += INTENT_COMMANDS * 120
    } else {
      commands++
      bytes += 64
    }
  }
  return { commands, objects, bytes }
}

export function fits(s: TxSize, limits: Limits = SUI_LIMITS): boolean {
  return s.commands <= limits.commands && s.objects <= limits.objects && s.bytes <= limits.bytes
}

/**
 * Split `items` into the fewest consecutive batches whose transactions fit. `build` must make
 * the transaction for a slice; its size has to grow with the slice (true for every builder here).
 * Each batch is the longest prefix that fits, found by binary search, which gives the minimum
 * count for this order. Throws when a single item does not fit on its own.
 */
export function planBatches<T>(items: T[], build: (slice: T[]) => Transaction, limits: Limits = SUI_LIMITS): T[][] {
  const out: T[][] = []
  const ok = (from: number, n: number) => fits(measure(build(items.slice(from, from + n))), limits)
  let at = 0
  while (at < items.length) {
    const left = items.length - at
    if (ok(at, left)) {
      out.push(items.slice(at))
      break
    }
    if (!ok(at, 1)) throw new Error('One item is too big for a single transaction')
    // largest n in [1, left) that fits
    let lo = 1
    let hi = left - 1
    while (lo < hi) {
      const mid = Math.ceil((lo + hi) / 2)
      if (ok(at, mid)) lo = mid
      else hi = mid - 1
    }
    out.push(items.slice(at, at + lo))
    at += lo
  }
  return out
}

/** Split an array into chunks of at most `n`. */
export function chunk<T>(a: T[], n: number): T[][] {
  const out: T[][] = []
  for (let i = 0; i < a.length; i += n) out.push(a.slice(i, i + n))
  return out
}
