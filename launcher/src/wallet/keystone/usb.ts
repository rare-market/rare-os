/**
 * Keystone 3 Pro over USB: requests as URs through the Rust EAPDU transport (native.ts
 * `keystoneNative`). Every answer is checked before use: keys must be 32 bytes on the asked
 * paths, signatures must carry our request id and verify against the account's key.
 */
import '../ledger/shims/buffer'
import { Ed25519PublicKey } from '@mysten/sui/keypairs/ed25519'
import { normalizeSuiAddress, toBase64 } from '@mysten/sui/utils'
import { keystoneNative } from '../native'
import { suiLedgerPath } from '../core/ledger/path'
import { KeystoneError, toKeystoneError } from './errors'
import { decodeUR, keyDerivationUR, parseMultiAccounts, parseSuiSignature, responsePayload, signHashUR } from './ur'

/** EAPDU commands (firmware src/webusb_protocol/general/eapdu_protocol_parser.h). */
export const KEYSTONE_CMD = { ECHO: 1, RESOLVE_UR: 2, CHECK_LOCK_STATUS: 3, EXPORT_ADDRESS: 4, GET_DEVICE_INFO: 5 } as const

/** The person approves on the device (and types its password for key export): give them time. */
const PROMPT_MS = 5 * 60_000

/** One command to the Keystone; returns the answer text. Errors become KeystoneErrors. */
export async function keystoneSend(command: number, data: string, timeoutMs = PROMPT_MS): Promise<string> {
  try {
    return await keystoneNative.send(command, data, timeoutMs)
  } catch (e) {
    throw toKeystoneError(e)
  }
}

export interface KeystoneAccountInfo {
  index: number
  path: string
  /** base64 32-byte Ed25519 key */
  publicKey: string
  address: string
  /** master fingerprint, 8 hex chars */
  mfp: string
}

/** Keys for accounts from..from+count-1 (m/44'/784'/i'/0'/0') in one request: one approval on the device. */
export async function getKeystoneAccounts(from: number, count: number): Promise<{ mfp: string; device?: string; accounts: KeystoneAccountInfo[] }> {
  if (count < 1 || count > 24) throw new Error('Ask for 1 to 24 accounts at a time')
  const paths = Array.from({ length: count }, (_, i) => suiLedgerPath(from + i))
  const answer = await keystoneSend(KEYSTONE_CMD.RESOLVE_UR, keyDerivationUR(paths))
  const got = parseMultiAccounts(decodeUR(responsePayload(answer)))
  if (!/^[0-9a-f]{8}$/.test(got.mfp)) throw new KeystoneError('bad-response')
  const accounts = paths.map((path, i) => {
    const k = got.keys.find((x) => x.path === path)
    if (!k) throw new KeystoneError('bad-response')
    return { index: from + i, path, publicKey: toBase64(k.publicKey), address: new Ed25519PublicKey(k.publicKey).toSuiAddress(), mfp: got.mfp }
  })
  return { mfp: got.mfp, device: got.device, accounts }
}

export async function getKeystoneAccount(index: number): Promise<KeystoneAccountInfo> {
  return (await getKeystoneAccounts(index, 1)).accounts[0]
}

/**
 * Has the Keystone sign a 32-byte digest at `path` (sui-sign-hash-request; the device shows the
 * hash, address and path). Returns the 64-byte Ed25519 signature after checking it is for our
 * request, comes from `address` (and `publicKey` when given), and verifies over `digest`.
 */
export async function keystoneSignHash(path: string, mfp: string, address: string, digest: Uint8Array, publicKey?: Uint8Array): Promise<Uint8Array> {
  const { ur, requestId } = signHashUR({ digest, path, mfp, address })
  const answer = await keystoneSend(KEYSTONE_CMD.RESOLVE_UR, ur)
  const sig = parseSuiSignature(decodeUR(responsePayload(answer)))
  if (sig.requestId && sig.requestId !== requestId) throw new KeystoneError('bad-response')
  const key = publicKey ?? sig.publicKey
  if (!key || key.length !== 32) throw new KeystoneError('bad-response')
  const pk = new Ed25519PublicKey(key)
  if (normalizeSuiAddress(pk.toSuiAddress()) !== normalizeSuiAddress(address)) throw new KeystoneError('mismatch')
  if (sig.publicKey && toBase64(sig.publicKey) !== toBase64(key)) throw new KeystoneError('mismatch')
  if (!(await pk.verify(digest, sig.signature))) throw new KeystoneError('mismatch')
  return sig.signature
}
