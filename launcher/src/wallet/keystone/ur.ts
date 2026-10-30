/**
 * The URs Rare Wallet exchanges with a Keystone 3 Pro over USB (EAPDU CMD_RESOLVE_UR):
 * - accounts: a `qr-hardware-call` KeyDerivation (version 1, ed25519, SLIP-10, chain "SUI") ->
 *   `crypto-multi-accounts` with the master fingerprint and one 32-byte key per path, as
 *   @keystonehq/hw-app-sol's getAddress does
 * - signing: `sui-sign-hash-request` (the 32-byte intent digest as hex, the path with the master
 *   fingerprint, the address) -> `sui-signature` (64-byte signature and the public key)
 * The firmware only accepts the Sui hash request over USB (see docs/KEYSTONE.md).
 */
import { Buffer } from '../ledger/shims/buffer'
import {
  CryptoKeypath,
  CryptoMultiAccounts,
  Curve,
  DerivationAlgorithm,
  KeyDerivation,
  KeyDerivationSchema,
  PathComponent,
  QRHardwareCall,
  QRHardwareCallType,
  QRHardwareCallVersion,
} from '@keystonehq/bc-ur-registry'
import { SuiSignHashRequest, SuiSignature } from '@keystonehq/bc-ur-registry-sui'
import { UR, URDecoder, UREncoder } from '@ngraveio/bc-ur'
import { normalizeSuiAddress } from '@mysten/sui/utils'
import { KeystoneError } from './errors'

/** Shown on the Keystone as the app asking. */
export const KEYSTONE_ORIGIN = 'Rare Wallet'

const hex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('')

/** "m/44'/784'/0'/0'/0'" -> CryptoKeypath, with the master fingerprint when given. */
export function toKeypath(path: string, mfp?: string): CryptoKeypath {
  const parts = path.replace(/^m\//i, '').split('/')
  const components = parts.map((p) => new PathComponent({ index: parseInt(p.replace("'", ''), 10), hardened: p.endsWith("'") }))
  return new CryptoKeypath(components, mfp ? Buffer.from(mfp, 'hex') : undefined)
}

/** CryptoKeypath.getPath() has no "m/" prefix. */
export const withM = (p: string) => (p.startsWith('m/') ? p : `m/${p}`)

/** UR -> the single-part string the device takes ("UR:TYPE/..."). */
export const encodeUR = (ur: UR) => UREncoder.encodeSinglePart(ur).toUpperCase()

export function decodeUR(text: string): UR {
  const s = text.trim()
  if (!/^ur:/i.test(s)) throw new KeystoneError('bad-response')
  try {
    // multi-part (ur:type/1-3/...) when a firmware ever splits it; single part otherwise
    if (/^ur:[^/]+\/\d+-\d+\//i.test(s)) {
      const d = new URDecoder()
      d.receivePart(s)
      if (!d.isComplete()) throw new Error('incomplete')
      return d.resultUR()
    }
    return URDecoder.decode(s)
  } catch {
    throw new KeystoneError('bad-response')
  }
}

// ---------------------------------------------------------------- accounts

export function keyDerivationCall(paths: string[]): QRHardwareCall {
  const schemas = paths.map((p) => new KeyDerivationSchema(toKeypath(p), Curve.ed25519, DerivationAlgorithm.slip10, 'SUI'))
  return new QRHardwareCall(QRHardwareCallType.KeyDerivation, new KeyDerivation(schemas), KEYSTONE_ORIGIN, QRHardwareCallVersion.V1)
}

export const keyDerivationUR = (paths: string[]) => encodeUR(keyDerivationCall(paths).toUR())

export interface KeystoneKeys {
  /** master fingerprint, 8 hex chars */
  mfp: string
  /** device name the Keystone reports ("Keystone 3 Pro"), when it does */
  device?: string
  keys: { path: string; publicKey: Uint8Array }[]
}

export function parseMultiAccounts(ur: UR): KeystoneKeys {
  if (ur.type !== 'crypto-multi-accounts') throw new KeystoneError('bad-response')
  const acc = CryptoMultiAccounts.fromCBOR(ur.cbor)
  const keys = acc.getKeys().map((k) => {
    const pk = new Uint8Array(k.getKey())
    const origin = k.getOrigin()?.getPath()
    // ed25519 keys are 32 bytes; some encoders prefix 0x00
    const publicKey = pk.length === 33 && pk[0] === 0 ? pk.subarray(1) : pk
    if (publicKey.length !== 32 || !origin) throw new KeystoneError('bad-response')
    return { path: withM(origin), publicKey }
  })
  return { mfp: hex(acc.getMasterFingerprint()), device: acc.getDevice() || undefined, keys }
}

// ---------------------------------------------------------------- signing

/** RFC 4122 v4 uuid without a dependency (the request id). */
export function newRequestId(): string {
  const b = crypto.getRandomValues(new Uint8Array(16))
  b[6] = (b[6] & 0x0f) | 0x40
  b[8] = (b[8] & 0x3f) | 0x80
  const h = hex(b)
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`
}

export function signHashRequest(o: { digest: Uint8Array; path: string; mfp: string; address: string; requestId?: string }): { request: SuiSignHashRequest; requestId: string } {
  if (o.digest.length !== 32) throw new Error('Expected a 32-byte digest')
  const requestId = o.requestId ?? newRequestId()
  const addr = Buffer.from(normalizeSuiAddress(o.address).slice(2), 'hex')
  const request = SuiSignHashRequest.constructSuiRequest(hex(o.digest), [o.path], [o.mfp], requestId, [addr], KEYSTONE_ORIGIN)
  return { request, requestId }
}

export const signHashUR = (o: Parameters<typeof signHashRequest>[0]) => {
  const { request, requestId } = signHashRequest(o)
  return { ur: encodeUR(request.toUR()), requestId }
}

export interface KeystoneSignature {
  signature: Uint8Array
  publicKey?: Uint8Array
  /** request id as a uuid string, when present */
  requestId?: string
}

const uuidString = (b: Uint8Array) => {
  const h = hex(b)
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`
}

export function parseSuiSignature(ur: UR): KeystoneSignature {
  if (ur.type !== 'sui-signature') throw new KeystoneError('bad-response')
  let s: SuiSignature
  try {
    s = SuiSignature.fromCBOR(ur.cbor)
  } catch {
    throw new KeystoneError('bad-response')
  }
  const signature = new Uint8Array(s.getSignature())
  if (signature.length !== 64) throw new KeystoneError('bad-response')
  const pk = s.getPublicKey()
  const id = s.getRequestId()
  return { signature, publicKey: pk ? new Uint8Array(pk) : undefined, requestId: id ? uuidString(new Uint8Array(id)) : undefined }
}

/**
 * The answer text from keystoneNative.send: the device's JSON ({"payload": "UR:..."}), a bare
 * JSON string, or the UR itself.
 */
export function responsePayload(text: string): string {
  const t = text.trim()
  if (/^ur:/i.test(t)) return t
  try {
    const v = JSON.parse(t) as unknown
    if (typeof v === 'string') return v
    if (v && typeof v === 'object' && typeof (v as { payload?: unknown }).payload === 'string') return (v as { payload: string }).payload
  } catch {
    /* not JSON */
  }
  throw new KeystoneError('bad-response')
}
