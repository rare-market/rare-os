/**
 * Stand-in for `bs58check` (aliased in vite.config.ts). bc-ur-registry only uses it to print
 * xpubs, but the real package loads create-hash -> readable-stream, which reads Node's `process`
 * at load and breaks the Keystone chunk in a webview. Same API, built on the hashes already bundled.
 */
import { sha256 } from '@noble/hashes/sha2.js'
import { fromBase58, toBase58 } from '@mysten/sui/utils'
import { Buffer } from '../../ledger/shims/buffer'

const checksum = (payload: Uint8Array) => sha256(sha256(payload)).subarray(0, 4)

export function encode(payload: Uint8Array): string {
  const out = new Uint8Array(payload.length + 4)
  out.set(payload)
  out.set(checksum(payload), payload.length)
  return toBase58(out)
}

export function decode(text: string): Buffer {
  const raw = fromBase58(text)
  const payload = raw.subarray(0, raw.length - 4)
  const want = checksum(payload)
  if (raw.length < 4 || raw.subarray(raw.length - 4).some((b, i) => b !== want[i])) throw new Error('Invalid checksum')
  return Buffer.from(payload)
}

export function decodeUnsafe(text: string): Buffer | undefined {
  try {
    return decode(text)
  } catch {
    return undefined
  }
}

export default { encode, decode, decodeUnsafe }
