import { bcs } from '@mysten/sui/bcs'
import { messageWithIntent, toSerializedSignature } from '@mysten/sui/cryptography'
import { Ed25519PublicKey } from '@mysten/sui/keypairs/ed25519'
import { Transaction } from '@mysten/sui/transactions'
import { normalizeSuiAddress } from '@mysten/sui/utils'
import { LedgerError, toLedgerError } from './errors'
import { addressFromPublicKey } from '../core/ledger/path'

/** The slice of @mysten/ledgerjs-hw-app-sui Rare Wallet uses (tests pass the real class over a mock transport). */
export interface SuiLedgerApp {
  getPublicKey(path: string, displayOnDevice?: boolean): Promise<{ publicKey: Uint8Array; address: Uint8Array | null }>
  signTransaction(path: string, txn: Uint8Array, options?: { bcsObjects: Uint8Array[] }): Promise<{ signature: Uint8Array }>
  getVersion(): Promise<{ major: number; minor: number; patch: number }>
}

/** Bytes the device signs for a transaction: intent (TransactionData, V0, Sui) || tx bytes. The app hashes them itself. */
export const txSigningPayload = (txBytes: Uint8Array) => messageWithIntent('TransactionData', txBytes)

/** Bytes the device signs for a personal message: intent (PersonalMessage) || BCS vector<u8>(message). */
export const personalMessageSigningPayload = (message: Uint8Array) =>
  messageWithIntent('PersonalMessage', bcs.byteVector().serialize(message).toBytes())

/** Device signature -> Sui serialized signature, base64 of flag(0x00 Ed25519) || sig(64) || pubkey(32). */
export function serializeLedgerSignature(signature: Uint8Array, publicKey: Uint8Array): string {
  if (signature.length !== 64) throw new LedgerError('unknown', 'Your Ledger returned a malformed signature. Try again.')
  return toSerializedSignature({ signature, signatureScheme: 'ED25519', publicKey: new Ed25519PublicKey(publicKey) })
}

/** Object ids the Sui app wants as BCS for clear signing: gas coins + owned/immutable inputs. */
export function clearSigningObjectIds(txBytes: Uint8Array): string[] {
  const data = Transaction.from(txBytes).getData()
  const gas = data.gasData.payment?.map((o) => o.objectId) ?? []
  const inputs = data.inputs
    .map((i) => (i.$kind === 'Object' && i.Object.$kind === 'ImmOrOwnedObject' ? i.Object.ImmOrOwnedObject.objectId : null))
    .filter((id): id is string => !!id)
  return [...new Set([...gas, ...inputs])]
}

/** Reads object BCS for clear signing; `getObjects` is client.core.getObjects of a SuiGrpcClient. */
export type ObjectBcsReader = (ids: string[]) => Promise<Uint8Array[]>

export function grpcObjectBcsReader(client: {
  core: { getObjects(o: { objectIds: string[]; include: { objectBcs: true } }): Promise<{ objects: unknown[] }> }
}): ObjectBcsReader {
  return async (objectIds) => {
    if (!objectIds.length) return []
    const r = await client.core.getObjects({ objectIds, include: { objectBcs: true } })
    return r.objects
      .filter((o) => !(o instanceof Error))
      .map((o) => (o as { objectBcs?: Uint8Array | null }).objectBcs)
      .filter((b): b is Uint8Array => !!b)
  }
}

/**
 * The device must hold the stored key at `path`. Checked (without a device prompt) before every
 * signature, so a different Ledger or seed fails with a clear message instead of a bad signature.
 */
export async function assertLedgerKey(app: SuiLedgerApp, path: string, publicKey: Uint8Array): Promise<void> {
  const { publicKey: got } = await app.getPublicKey(path, false)
  if (got.length !== publicKey.length || got.some((b, i) => b !== publicKey[i])) throw new LedgerError('mismatch')
}

export async function readLedgerAccount(app: SuiLedgerApp, path: string, display = false): Promise<{ publicKey: Uint8Array; address: string }> {
  try {
    const { publicKey, address } = await app.getPublicKey(path, display)
    const derived = addressFromPublicKey(publicKey)
    // the app also reports the address; a disagreement means a broken or unexpected app
    if (address && address.length === 32 && normalizeSuiAddress(`0x${toHex(address)}`) !== derived)
      throw new LedgerError('unknown', 'Your Ledger reported an address that does not match its key. Update the Sui app and try again.')
    return { publicKey, address: derived }
  } catch (e) {
    throw toLedgerError(e)
  }
}

/**
 * Sign transaction bytes on the device as the sender. Clear signing gets the input objects' BCS
 * (the app shows coin types and amounts from them); when they can't be read the app falls back to
 * its own checks and may ask for blind signing. Sponsored transactions sign the same way: the
 * Ledger signs as sender, the sponsor adds its own signature separately.
 */
export async function ledgerSignTransaction(
  app: SuiLedgerApp,
  o: { path: string; publicKey: Uint8Array; txBytes: Uint8Array; objects?: ObjectBcsReader },
): Promise<string> {
  try {
    await assertLedgerKey(app, o.path, o.publicKey)
    let bcsObjects: Uint8Array[] = []
    if (o.objects) {
      try {
        bcsObjects = await o.objects(clearSigningObjectIds(o.txBytes))
      } catch {
        bcsObjects = []
      }
    }
    const { signature } = await app.signTransaction(o.path, txSigningPayload(o.txBytes), { bcsObjects })
    return serializeLedgerSignature(signature, o.publicKey)
  } catch (e) {
    throw toLedgerError(e)
  }
}

export async function ledgerSignPersonalMessage(app: SuiLedgerApp, o: { path: string; publicKey: Uint8Array; message: Uint8Array }): Promise<string> {
  try {
    await assertLedgerKey(app, o.path, o.publicKey)
    const { signature } = await app.signTransaction(o.path, personalMessageSigningPayload(o.message))
    return serializeLedgerSignature(signature, o.publicKey)
  } catch (e) {
    throw toLedgerError(e)
  }
}

/** Response of the OS-level GET_APP_AND_VERSION (CLA 0xb0, INS 0x01), status word stripped or not. */
export function parseAppAndVersion(res: Uint8Array): { name: string; version: string } {
  if (res.length < 3 || res[0] !== 0x01) throw new Error('Unexpected app info')
  const dec = new TextDecoder()
  let i = 1
  const nameLen = res[i++]
  const name = dec.decode(res.subarray(i, i + nameLen))
  i += nameLen
  const verLen = res[i++] ?? 0
  const version = dec.decode(res.subarray(i, i + verLen))
  return { name, version }
}

/** Throws a LedgerError unless the open app is Sui. The dashboard reports itself as "BOLOS". */
export function assertSuiApp(app: { name: string }) {
  if (app.name === 'Sui') return
  if (app.name === 'BOLOS' || app.name === '') throw new LedgerError('app-closed')
  throw new LedgerError('wrong-app', `Your Ledger has the ${app.name} app open. Close it, open the Sui app and try again.`)
}

const toHex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('')
