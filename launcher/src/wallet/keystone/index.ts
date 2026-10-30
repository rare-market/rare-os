/**
 * Keystone 3 Pro for Rare Wallet: the API the screens call, and the signer for Keystone accounts.
 * Load it lazily (`await import('./wallet/keystone')`); it pulls in the UR libraries. The review
 * sheet's live state and the signing hash helper are in ./prompt (light).
 *
 * Signing is hash signing over USB: the Keystone shows the 32-byte hash it signs (plus address
 * and path), not the decoded transaction. See docs/KEYSTONE.md.
 */
import '../ledger/shims/buffer'
import { toSerializedSignature } from '@mysten/sui/cryptography'
import { Ed25519PublicKey } from '@mysten/sui/keypairs/ed25519'
import { fromBase64, toBase64 } from '@mysten/sui/utils'
import { verifyPersonalMessageSignature, verifyTransactionSignature } from '@mysten/sui/verify'
import { bytesToHex } from '@noble/hashes/utils.js'
import { keystoneNative, type KeystoneDevice } from '../native'
import { keystore } from '../index'
import type { RareAccount } from '../core/types'
import type { AccountSigner } from '../core/keys/signer'
import { KeystoneError, toKeystoneError, type KeystoneErrorCode } from './errors'
import { getKeystoneAccounts, keystoneSignHash, type KeystoneAccountInfo } from './usb'
import { beginKeystoneTask, resetKeystonePrompt, setKeystoneError, setKeystoneStage, suiSigningDigest, type KeystoneStage } from './prompt'

export { KeystoneError, toKeystoneError, keystoneErrorMessage, KEYSTONE_MESSAGES, type KeystoneErrorCode } from './errors'
export {
  useKeystonePrompt,
  cancelKeystone,
  resetKeystonePrompt,
  suiSigningDigest,
  suiSigningDigestHex,
  KEYSTONE_STAGE_TEXT,
  type KeystoneStage,
  type KeystonePromptState,
} from './prompt'
export { keystoneSend, getKeystoneAccount, getKeystoneAccounts, keystoneSignHash, KEYSTONE_CMD, type KeystoneAccountInfo } from './usb'
export type { KeystoneDevice }

/** How long to keep retrying while the Keystone is locked or off its home screen. */
const WAIT_MS = 3 * 60_000
const POLL_MS = 1500

export async function listKeystones(): Promise<KeystoneDevice[]> {
  try {
    return await keystoneNative.list()
  } catch (e) {
    throw toKeystoneError(e)
  }
}

const sleep = (ms: number, signal: AbortSignal) =>
  new Promise<void>((resolve, reject) => {
    const t = setTimeout(resolve, ms)
    signal.addEventListener('abort', () => (clearTimeout(t), reject(new KeystoneError('cancelled'))), { once: true })
  })

/** Rejects with "cancelled" as soon as the signal fires; the device call is left to finish. */
const cancellable = <T>(p: Promise<T>, signal: AbortSignal) =>
  new Promise<T>((resolve, reject) => {
    if (signal.aborted) return reject(new KeystoneError('cancelled'))
    const onAbort = () => reject(new KeystoneError('cancelled'))
    signal.addEventListener('abort', onAbort, { once: true })
    p.then(
      (v) => (signal.removeEventListener('abort', onAbort), resolve(v)),
      (e) => (signal.removeEventListener('abort', onAbort), reject(toKeystoneError(e))),
    )
  })

/**
 * Runs a Keystone request with the prompt store in step. While the device is locked or not on
 * its home screen it shows that and retries, so the person can fix it without starting over.
 */
async function task<T>(stage: Exclude<KeystoneStage, 'error'>, digestHex: string | null, fn: () => Promise<T>): Promise<T> {
  const signal = beginKeystoneTask()
  const deadline = Date.now() + WAIT_MS
  setKeystoneStage(stage, digestHex)
  try {
    for (;;) {
      try {
        const r = await cancellable(fn(), signal)
        setKeystoneStage('done')
        return r
      } catch (e) {
        const ke = toKeystoneError(e)
        // "usb-off" is how the Rust side reports a request refused away from the home screen
        const wait: Partial<Record<KeystoneErrorCode, 'unlock' | 'home-screen'>> = { locked: 'unlock', 'home-screen': 'home-screen', 'usb-off': 'home-screen' }
        const w = wait[ke.code]
        if (!w || Date.now() > deadline) throw ke
        setKeystoneStage(w)
        await sleep(POLL_MS, signal)
        setKeystoneStage(stage)
      }
    }
  } catch (e) {
    const ke = toKeystoneError(e)
    setKeystoneError({ code: ke.code, message: ke.message, fix: ke.fix })
    throw ke
  }
}

// ---------------------------------------------------------------- accounts

// keys read this session by index, so adding one needs no second approval on the device
const seen = new Map<number, KeystoneAccountInfo & { device?: string }>()

/** Accounts from..from+count-1 (up to 24). The person approves the export on the Keystone once. */
export async function keystoneAccounts(from = 0, count = 5): Promise<KeystoneAccountInfo[]> {
  const r = await task('approve', null, () => getKeystoneAccounts(from, count))
  // a different Keystone (another seed) replaces what was read before
  if ([...seen.values()].some((a) => a.mfp !== r.mfp)) seen.clear()
  for (const a of r.accounts) seen.set(a.index, { ...a, device: r.device })
  return r.accounts
}

/** Adds account `index` of the connected Keystone to the wallet. */
export async function addKeystoneAccount(index: number, label?: string): Promise<RareAccount> {
  if (!seen.has(index)) await keystoneAccounts(index, 1)
  const a = seen.get(index)
  if (!a) throw new KeystoneError('bad-response')
  return keystore.addKeystone(a.publicKey, a.path, a.mfp, label, a.device ?? 'Keystone 3 Pro')
}

// ---------------------------------------------------------------- signing

/**
 * Signs for a Keystone account. Holds only public data. Computes the intent digest Sui verifies,
 * has the Keystone sign that hash over USB, and checks the result against the account before
 * returning a serialized Ed25519 Sui signature.
 */
export class KeystoneSigner implements AccountSigner {
  readonly address: string
  private publicKey: Ed25519PublicKey
  constructor(private a: { publicKey: string; path: string; mfp: string }) {
    const raw = fromBase64(a.publicKey)
    if (raw.length !== 32) throw new KeystoneError('unknown', 'This Keystone account has no valid public key. Remove it and add it again.')
    this.publicKey = new Ed25519PublicKey(raw)
    this.address = this.publicKey.toSuiAddress()
  }

  private async sign(kind: 'transaction' | 'message', bytes: Uint8Array): Promise<string> {
    const digest = suiSigningDigest(kind, bytes)
    const signature = await task('confirm', bytesToHex(digest), () =>
      keystoneSignHash(this.a.path, this.a.mfp, this.address, digest, this.publicKey.toRawBytes()),
    )
    return toSerializedSignature({ signature, signatureScheme: 'ED25519', publicKey: this.publicKey })
  }

  async signTransaction(txBytes: Uint8Array): Promise<string> {
    const signature = await this.sign('transaction', txBytes)
    await verifyTransactionSignature(txBytes, signature, { address: this.address }).catch(() => {
      throw new KeystoneError('mismatch')
    })
    resetKeystonePrompt()
    return signature
  }

  async signPersonalMessage(message: Uint8Array): Promise<{ bytes: string; signature: string }> {
    const signature = await this.sign('message', message)
    await verifyPersonalMessageSignature(message, signature, { address: this.address }).catch(() => {
      throw new KeystoneError('mismatch')
    })
    resetKeystonePrompt()
    return { bytes: toBase64(message), signature }
  }
}

/** Keystore provider for kind "keystone". */
export async function keystoneSigner(account: RareAccount, publicKey: string): Promise<AccountSigner> {
  if (!account.keystone) throw new KeystoneError('unknown', 'This account is not a Keystone account.')
  return new KeystoneSigner({ publicKey, path: account.keystone.path, mfp: account.keystone.mfp })
}

keystore.setProvider('keystone', keystoneSigner)
