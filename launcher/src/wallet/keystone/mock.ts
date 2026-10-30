/**
 * Browser stand-in for a Keystone 3 Pro over USB (native.ts with `?mockhw`) and the device for
 * tests. Decodes the request UR like the firmware does and answers key derivation calls and Sui
 * hash requests from a fixed test phrase, with real Ed25519 signatures over the digest. It checks
 * the master fingerprint like firmware sui_check_sign_hash_request, and fails with the same
 * "code|message" errors the Rust transport sends (eapdu.rs status_error). Never used on the device.
 */
import { Buffer } from '../ledger/shims/buffer'
import { CryptoHDKey, CryptoMultiAccounts, KeyDerivation, QRHardwareCall } from '@keystonehq/bc-ur-registry'
import { SuiSignHashRequest, SuiSignature } from '@keystonehq/bc-ur-registry-sui'
import { Ed25519Keypair } from '@mysten/sui/keypairs/ed25519'
import { NativeError } from '../native'
import { decodeUR, encodeUR, toKeypath, withM } from './ur'

export const KEYSTONE_TEST_PHRASE = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about'
/** BIP32 master fingerprint of the test phrase (BIP84 test vectors) */
export const KEYSTONE_TEST_MFP = '73c5da0a'

export interface MockKeystone {
  phrase: string
  mfp: string
  locked: boolean
  reject: boolean
  /** not on the home screen: key export is refused like the firmware does */
  away: boolean
  /** ms the device "waits for the person" (mock UI only) */
  promptDelayMs: number
  /** every request UR type received */
  requests: string[]
  /** message hashes it was asked to sign */
  hashes: string[]
}

const state: MockKeystone = {
  phrase: KEYSTONE_TEST_PHRASE,
  mfp: KEYSTONE_TEST_MFP,
  locked: false,
  reject: false,
  away: false,
  promptDelayMs: 1200,
  requests: [],
  hashes: [],
}

export const mockKeystoneDevice = () => state

const hexToBytes = (h: string) => Uint8Array.from(h.match(/../g) ?? [], (x) => parseInt(x, 16))
const answer = (ur: string) => JSON.stringify({ payload: ur })
const wait = () => (state.promptDelayMs ? new Promise((r) => setTimeout(r, state.promptDelayMs)) : Promise.resolve())

export async function mockKeystoneSend(command: number, data: string): Promise<string> {
  if (command === 1) return JSON.stringify({ payload: data })
  if (command === 3) return JSON.stringify({ payload: state.locked ? 'true' : 'false' })
  if (command === 5) return JSON.stringify({ payload: JSON.stringify({ firmwareVersion: '2.1.4', walletMFP: state.mfp }) })
  if (command !== 2) throw new NativeError('error', 'unknown command')
  if (state.locked) throw new NativeError('locked', 'Unlock your Keystone')

  const ur = decodeUR(data)
  state.requests.push(ur.type)
  if (ur.type === 'qr-hardware-call') {
    if (state.away) throw new NativeError('usb-off', 'Open the Keystone home screen and turn on USB in Settings > Connection')
    const call = QRHardwareCall.fromCBOR(ur.cbor)
    const params = call.getParams() as KeyDerivation
    await wait()
    if (state.reject) throw new NativeError('rejected', 'Rejected on your Keystone')
    const keys = params.getSchemas().map((s) => {
      const path = withM(s.getKeypath().getPath())
      const pk = Ed25519Keypair.deriveKeypair(state.phrase, path).getPublicKey().toRawBytes()
      return new CryptoHDKey({ isMaster: false, key: Buffer.from(pk), origin: toKeypath(path) })
    })
    const accounts = new CryptoMultiAccounts(Buffer.from(state.mfp, 'hex'), keys, 'Keystone 3 Pro', undefined, '2.1.4')
    return answer(encodeUR(accounts.toUR()))
  }
  if (ur.type === 'sui-sign-hash-request') {
    const req = SuiSignHashRequest.fromCBOR(ur.cbor)
    const paths = (req as unknown as { derivationPaths: { getPath(): string; getSourceFingerprint(): Uint8Array | undefined }[] }).derivationPaths
    const fp = paths[0]?.getSourceFingerprint()
    if (!fp || Buffer.from(fp).toString('hex') !== state.mfp) throw new NativeError('mismatch', 'This Keystone holds a different wallet')
    const path = withM(paths[0].getPath())
    const hash = req.getMessageHash()
    state.hashes.push(hash)
    await wait()
    if (state.reject) throw new NativeError('rejected', 'Rejected on your Keystone')
    const kp = Ed25519Keypair.deriveKeypair(state.phrase, path)
    const sig = await kp.sign(hexToBytes(hash))
    const out = new SuiSignature(Buffer.from(sig), req.getRequestId(), Buffer.from(kp.getPublicKey().toRawBytes()))
    return answer(encodeUR(out.toUR()))
  }
  throw new NativeError('error', 'Keystone: this view type is not supported')
}
