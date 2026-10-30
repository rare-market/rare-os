import { Ed25519PublicKey } from '@mysten/sui/keypairs/ed25519'
import { fromBase64, toBase64 } from '@mysten/sui/utils'

/** Sui's coin type in SLIP-44. Every Sui wallet (Slush, Nightly, Ledger Live) uses it. */
export const SUI_COIN_TYPE = 784

/** Ledger Sui path for account `i`: m/44'/784'/i'/0'/0' (all hardened, as the Sui app requires). */
export function suiLedgerPath(index: number): string {
  if (!Number.isInteger(index) || index < 0 || index >= 0x80000000) throw new Error('Invalid account index')
  return `m/44'/${SUI_COIN_TYPE}'/${index}'/0'/0'`
}

const PATH_RE = /^m\/44'\/784'\/(\d+)'\/0'\/0'$/

/** Account index of a standard Sui path, or null for anything else. */
export function ledgerPathIndex(path: string): number | null {
  const m = PATH_RE.exec(path.trim())
  if (!m) return null
  const i = Number(m[1])
  return i < 0x80000000 ? i : null
}

export const isSuiLedgerPath = (path: string) => ledgerPathIndex(path) !== null

/** Raw 32-byte Ed25519 key from the device -> Sui address (blake2b(0x00 || pk)). */
export function addressFromPublicKey(publicKey: Uint8Array | string): string {
  const raw = typeof publicKey === 'string' ? fromBase64(publicKey) : publicKey
  if (raw.length !== 32) throw new Error('Expected a 32-byte Ed25519 public key')
  return new Ed25519PublicKey(raw).toSuiAddress()
}

export const publicKeyToB64 = (publicKey: Uint8Array) => toBase64(publicKey)
