/**
 * Ledger for Rare Wallet: the API the screens call, and the signer for Ledger accounts.
 * Load it lazily (`await import('./wallet/ledger')`); it pulls in the Ledger libraries.
 * The review sheet's live state is in ./prompt (light, safe to import anywhere).
 */
import './shims/buffer'
import { toBase64, fromBase64 } from '@mysten/sui/utils'
import { verifyPersonalMessageSignature, verifyTransactionSignature } from '@mysten/sui/verify'
import { ledgerNative, type LedgerDevice, type LedgerModel } from '../native'
import { grpcClient, keystore } from '../index'
import type { RareAccount } from '../core/types'
import type { AccountSigner } from '../core/keys/signer'
import { addressFromPublicKey, suiLedgerPath } from '../core/ledger/path'
import { LedgerError, toLedgerError, type LedgerErrorCode } from './errors'
import { deviceStep, withLedger, type LedgerSession } from './device'
import {
  clearSigningObjectIds,
  grpcObjectBcsReader,
  ledgerSignPersonalMessage,
  ledgerSignTransaction,
  readLedgerAccount,
  type ObjectBcsReader,
} from './sign'
import { beginLedgerTask, resetLedgerPrompt, setLedgerError, setLedgerStage } from './prompt'

export { LedgerError, toLedgerError, ledgerErrorMessage, LEDGER_MESSAGES, type LedgerErrorCode } from './errors'
export { useLedgerPrompt, cancelLedger, resetLedgerPrompt, LEDGER_STAGE_TEXT, type LedgerStage, type LedgerPromptState } from './prompt'
export type { LedgerDevice, LedgerModel }

/** How long the person has to confirm on the device. */
const CONFIRM_MS = 5 * 60_000
/** How long to wait for "Unlock your Ledger" / "Open the Sui app" before giving up. */
const APP_WAIT_MS = 3 * 60_000
const POLL_MS = 1500

// ---------------------------------------------------------------- devices

let current: LedgerDevice | null = null

/** USB Ledgers now; with `ble`, also scans Bluetooth (Nano X, Stax, Flex) for `scanMs`. */
export async function listLedgers(o: { ble?: boolean; scanMs?: number } = {}): Promise<LedgerDevice[]> {
  try {
    return await ledgerNative.list(o)
  } catch (e) {
    throw toLedgerError(e)
  }
}

export interface ConnectedLedger {
  id: string
  model: LedgerModel
  name: string
  transport: 'usb' | 'ble'
  /** Sui app version, or null on firmware that does not report it */
  appVersion: string | null
}

/** The Ledger the last connectLedger picked (null before). */
export const connectedLedger = (): LedgerDevice | null => current

const sleep = (ms: number, signal?: AbortSignal) =>
  new Promise<void>((resolve, reject) => {
    const t = setTimeout(resolve, ms)
    signal?.addEventListener('abort', () => (clearTimeout(t), reject(new LedgerError('cancelled'))), { once: true })
  })

/**
 * A device conversation that waits while the Ledger is locked or the Sui app is closed, showing
 * "Unlock your Ledger" / "Open the Sui app" and retrying, so the person can fix it without
 * starting over.
 */
async function session<T>(id: string, fn: (s: LedgerSession) => Promise<T>, signal: AbortSignal): Promise<T> {
  const deadline = Date.now() + APP_WAIT_MS
  for (;;) {
    if (signal.aborted) throw new LedgerError('cancelled')
    try {
      return await withLedger(id, fn)
    } catch (e) {
      const le = toLedgerError(e)
      const wait: Partial<Record<LedgerErrorCode, 'unlock' | 'open-app'>> = { locked: 'unlock', 'app-closed': 'open-app', 'wrong-app': 'open-app' }
      const stage = wait[le.code]
      if (!stage || Date.now() > deadline) throw le
      setLedgerStage(stage)
      await sleep(POLL_MS, signal)
      setLedgerStage('connecting')
    }
  }
}

/** Runs a Ledger task with the prompt store kept in step; errors land in the store and are rethrown. */
async function task<T>(device: string | null, fn: (signal: AbortSignal) => Promise<T>): Promise<T> {
  const signal = beginLedgerTask()
  setLedgerStage('connecting', device)
  try {
    const r = await fn(signal)
    setLedgerStage('done')
    return r
  } catch (e) {
    const le = toLedgerError(e)
    setLedgerError({ code: le.code, message: le.message, fix: le.fix })
    throw le
  }
}

/** Opens the Ledger, checks the Sui app is open (waiting for it), and remembers it for the calls below. */
export async function connectLedger(id: string): Promise<ConnectedLedger> {
  return task(null, (signal) =>
    session(
      id,
      async (s) => {
        current = s.device
        return { id: s.device.id, model: s.device.model, name: s.device.name, transport: s.device.transport, appVersion: s.appVersion ?? null }
      },
      signal,
    ),
  )
}

/** The connected Ledger, else the first one found (see pickDevice). */
const need = (): Promise<LedgerDevice> => (current ? Promise.resolve(current) : pickDevice())

// ---------------------------------------------------------------- accounts

export interface LedgerAccountInfo {
  index: number
  path: string
  /** base64 32-byte Ed25519 key */
  publicKey: string
  address: string
}

// keys read this session, so adding one needs no second device round trip
const seen = new Map<string, LedgerAccountInfo>()
const seenKey = (d: LedgerDevice, index: number) => `${d.id}|${index}`

/** Public keys and addresses of accounts from..from+count-1 on m/44'/784'/i'/0'/0'. No device prompt. */
export async function ledgerAccounts(from = 0, count = 5): Promise<LedgerAccountInfo[]> {
  const d = await need()
  return task(d.name, (signal) =>
    session(
      d.id,
      async (s) => {
        const out: LedgerAccountInfo[] = []
        for (let index = from; index < from + count; index++) {
          const path = suiLedgerPath(index)
          const { publicKey, address } = await readLedgerAccount(s.app, path)
          const info = { index, path, publicKey: toBase64(publicKey), address }
          seen.set(seenKey(d, index), info)
          out.push(info)
        }
        return out
      },
      signal,
    ),
  )
}

/** Shows account `index`'s address on the Ledger for the person to compare; resolves once they confirm. */
export async function verifyLedgerAddress(index: number): Promise<LedgerAccountInfo> {
  const d = await need()
  return task(d.name, (signal) =>
    session(
      d.id,
      async (s) => {
        const path = suiLedgerPath(index)
        setLedgerStage('verify')
        const { publicKey, address } = await deviceStep(readLedgerAccount(s.app, path, true), { timeoutMs: CONFIRM_MS, signal })
        const info = { index, path, publicKey: toBase64(publicKey), address }
        seen.set(seenKey(d, index), info)
        return info
      },
      signal,
    ),
  )
}

/** Adds account `index` of the connected Ledger to the wallet. */
export async function addLedgerAccount(index: number, label?: string): Promise<RareAccount> {
  const d = await need()
  const info = seen.get(seenKey(d, index)) ?? (await ledgerAccounts(index, 1))[0]
  return keystore.addLedger(info.publicKey, info.path, label, d.name)
}

// ---------------------------------------------------------------- signing

/**
 * The Ledger to sign with: the connected one, else the first USB Ledger, else a Bluetooth one
 * (the one the account was added with when its name matches).
 */
async function pickDevice(name?: string): Promise<LedgerDevice> {
  if (current) return current
  const usb = await listLedgers()
  if (usb.length) return (current = usb.find((d) => d.name === name) ?? usb[0])
  const all = await listLedgers({ ble: true })
  const ble = all.filter((d) => d.transport === 'ble')
  const d = ble.find((x) => x.name === name) ?? ble.find((x) => x.paired) ?? ble[0]
  if (!d) throw new LedgerError('no-device')
  return (current = d)
}

/** Opens the right Ledger for a signature; a stale remembered device falls back to discovery once. */
async function signSession<T>(name: string | undefined, fn: (s: LedgerSession, signal: AbortSignal) => Promise<T>): Promise<T> {
  return task(name ?? null, async (signal) => {
    let d = await pickDevice(name)
    showDevice(d)
    try {
      return await session(d.id, (s) => fn(s, signal), signal)
    } catch (e) {
      const le = toLedgerError(e)
      if (le.code !== 'no-device' && le.code !== 'disconnected') throw le
      current = null
      d = await pickDevice(name)
      showDevice(d)
      return await session(d.id, (s) => fn(s, signal), signal)
    }
  })
}

const showDevice = (d: LedgerDevice) => setLedgerStage('connecting', d.name)

const MISMATCH = 'The signature from your Ledger does not match this account. Connect the Ledger you added it with.'

/**
 * Signs for a Ledger account. Holds only public data. Transactions get the input objects' BCS
 * (read over gRPC before the device opens) for clear signing. Every signature is verified
 * against the account before it is returned.
 */
export class HardwareLedgerSigner implements AccountSigner {
  readonly address: string
  private publicKey: Uint8Array
  constructor(
    private a: { publicKey: string; path: string; device?: string },
    private objects: ObjectBcsReader = grpcObjectBcsReader(grpcClient),
  ) {
    this.publicKey = fromBase64(a.publicKey)
    if (this.publicKey.length !== 32) throw new LedgerError('unknown', 'This Ledger account has no valid public key. Remove it and add it again.')
    this.address = addressFromPublicKey(this.publicKey)
  }

  async signTransaction(txBytes: Uint8Array): Promise<string> {
    const bcsObjects = await this.objects(clearSigningObjectIds(txBytes)).catch(() => [] as Uint8Array[])
    const signature = await signSession(this.a.device, (s, signal) => {
      setLedgerStage('confirm')
      return deviceStep(ledgerSignTransaction(s.app, { path: this.a.path, publicKey: this.publicKey, txBytes, objects: async () => bcsObjects }), {
        timeoutMs: CONFIRM_MS,
        signal,
      })
    })
    await verifyTransactionSignature(txBytes, signature, { address: this.address }).catch(() => {
      throw new LedgerError('mismatch', MISMATCH)
    })
    resetLedgerPrompt()
    return signature
  }

  async signPersonalMessage(message: Uint8Array): Promise<{ bytes: string; signature: string }> {
    const signature = await signSession(this.a.device, (s, signal) => {
      setLedgerStage('confirm')
      return deviceStep(ledgerSignPersonalMessage(s.app, { path: this.a.path, publicKey: this.publicKey, message }), { timeoutMs: CONFIRM_MS, signal })
    })
    await verifyPersonalMessageSignature(message, signature, { address: this.address }).catch(() => {
      throw new LedgerError('mismatch', MISMATCH)
    })
    resetLedgerPrompt()
    return { bytes: toBase64(message), signature }
  }
}

/** Keystore provider for kind "ledger". */
export async function ledgerSigner(account: RareAccount, publicKey: string): Promise<AccountSigner> {
  if (!account.ledger) throw new LedgerError('unknown', 'This account is not a Ledger account.')
  return new HardwareLedgerSigner({ publicKey, path: account.ledger.path, device: account.ledger.device })
}

keystore.setProvider('ledger', ledgerSigner)

/** Forget the remembered Ledger (after unplugging, or to pick another). */
export function forgetLedger() {
  current = null
  seen.clear()
}
