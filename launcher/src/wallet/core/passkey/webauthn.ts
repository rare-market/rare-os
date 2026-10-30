import { PasskeyPublicKey } from '@mysten/sui/keypairs/passkey'
import { fromBase64, toBase64 } from '@mysten/sui/utils'
import { randomBytes } from '@noble/hashes/utils.js'

/**
 * WebAuthn (passkey) plumbing. EVERYTHING that touches `navigator.credentials` lives here and
 * must run in a page context (the popup): it needs a user gesture and a secure context, and a
 * service worker has no `navigator.credentials`. The background only ever sees public data.
 */

export interface PasskeyCreated {
  /** base64 of the 33-byte compressed secp256r1 public key */
  publicKey: string
  /** base64 of the credential id (public) */
  credentialId: string
  address: string
}

export interface AssertionRequest {
  /** base64 credential id to constrain the authenticator to */
  credentialId: string
  /** base64 of the 32-byte blake2b digest of the intent message; becomes the WebAuthn challenge */
  challenge: string
}

/** Raw WebAuthn assertion, base64 so it can cross process boundaries. */
export interface AssertionResult {
  authenticatorData: string
  clientDataJSON: string
  /** DER-encoded ECDSA signature as returned by the authenticator */
  signature: string
}

export interface CredentialsLike {
  create(o: any): Promise<any>
  get(o: any): Promise<any>
}

export interface CreateOptions {
  /** defaults to location.hostname (the launcher's origin) */
  rpId?: string
  rpName?: string
  label?: string
  creds?: CredentialsLike
}

const buf = (b: ArrayBuffer | ArrayBufferView) => (b instanceof ArrayBuffer ? new Uint8Array(b) : new Uint8Array(b.buffer, b.byteOffset, b.byteLength))

// SubjectPublicKeyInfo header for P-256 (26 bytes) followed by the 65-byte uncompressed point.
const SPKI_HEADER_LEN = 26

/** DER SPKI (from AuthenticatorAttestationResponse.getPublicKey()) -> 33-byte compressed key. */
export function compressedKeyFromSpki(spki: ArrayBuffer | ArrayBufferView): Uint8Array {
  const b = buf(spki)
  if (b.length !== SPKI_HEADER_LEN + 65 || b[SPKI_HEADER_LEN] !== 0x04) throw new Error('Unsupported passkey public key (expected P-256)')
  const out = new Uint8Array(33)
  out[0] = (b[SPKI_HEADER_LEN + 64] & 1) === 0 ? 0x02 : 0x03 // parity of y
  out.set(b.subarray(SPKI_HEADER_LEN + 1, SPKI_HEADER_LEN + 33), 1) // x
  return out
}

export function addressFromPublicKey(publicKey: string | Uint8Array): string {
  return new PasskeyPublicKey(publicKey).toSuiAddress()
}

/**
 * Register a new discoverable, user-verified platform credential (Face ID / Touch ID) and derive
 * the Sui address. POPUP ONLY, call from a click handler.
 *
 * The user handle is random per call on purpose: authenticators replace an existing credential
 * with the same rpId + user.id, so a fixed handle would wipe the previous passkey account.
 */
export async function createPasskey(o: CreateOptions = {}): Promise<PasskeyCreated> {
  const creds = o.creds ?? (globalThis.navigator?.credentials as CredentialsLike | undefined)
  if (!creds) throw new Error('WebAuthn is not available in this context')
  const name = o.label?.trim() || 'Rare Wallet'
  const cred = await creds.create({
    publicKey: {
      rp: { name: o.rpName ?? 'Rare Wallet', ...(o.rpId ? { id: o.rpId } : {}) },
      user: { id: randomBytes(16), name, displayName: name },
      challenge: randomBytes(32),
      pubKeyCredParams: [{ type: 'public-key', alg: -7 }], // ES256 = secp256r1
      // any authenticator: Touch ID, a phone, or a security key (YubiKey supports ES256)
      authenticatorSelection: { residentKey: 'required', requireResidentKey: true, userVerification: 'required' },
      attestation: 'none',
      timeout: 60_000,
    },
  })
  const spki = cred?.response?.getPublicKey?.()
  if (!spki) throw new Error('Passkey registration returned no public key')
  const pk = compressedKeyFromSpki(spki)
  return { publicKey: toBase64(pk), credentialId: toBase64(buf(cred.rawId)), address: addressFromPublicKey(pk) }
}

/**
 * Ask the authenticator to sign `challenge` (Face ID / Touch ID prompt). POPUP ONLY.
 * Returns the raw assertion; the Sui signature is assembled by PasskeySigner.
 */
export async function getAssertion(req: AssertionRequest, creds?: CredentialsLike): Promise<AssertionResult> {
  const c = creds ?? (globalThis.navigator?.credentials as CredentialsLike | undefined)
  if (!c) throw new Error('WebAuthn is not available in this context')
  const a = await c.get({
    publicKey: {
      challenge: fromBase64(req.challenge),
      allowCredentials: [{ type: 'public-key', id: fromBase64(req.credentialId) }],
      userVerification: 'required',
      timeout: 60_000,
    },
  })
  if (!a?.response) throw new Error('Passkey assertion was cancelled')
  return {
    authenticatorData: toBase64(buf(a.response.authenticatorData)),
    clientDataJSON: toBase64(buf(a.response.clientDataJSON)),
    signature: toBase64(buf(a.response.signature)),
  }
}
