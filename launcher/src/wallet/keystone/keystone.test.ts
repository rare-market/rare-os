import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('../native', async (orig) => {
  const real = await orig<typeof import('../native')>()
  return {
    ...real,
    keystoneNative: {
      list: async () => [{ id: 'usb:mock', name: 'Keystone 3 Pro' }],
      send: async (command: number, data: string) => (await import('./mock')).mockKeystoneSend(command, data),
    },
  }
})

vi.mock('../index', async () => {
  const { Keystore } = await import('../core/keys/keystore')
  const { MemoryKV } = await import('../core/store/kv')
  const { memoryVault } = await import('../testing/memoryVault')
  return { keystore: new Keystore({ disk: new MemoryKV(), vault: memoryVault() }), grpcClient: {} }
})

import { Buffer } from '../ledger/shims/buffer'
import { CryptoKeypath, Curve, DataItem, DerivationAlgorithm, KeyDerivation, QRHardwareCall, extend } from '@keystonehq/bc-ur-registry'
import { SuiSignHashRequest, SuiSignature } from '@keystonehq/bc-ur-registry-sui'
import { Ed25519Keypair } from '@mysten/sui/keypairs/ed25519'
import { Transaction, Inputs } from '@mysten/sui/transactions'
import { bcs } from '@mysten/sui/bcs'
import { fromBase64, toBase64 } from '@mysten/sui/utils'
import { verifyPersonalMessageSignature, verifyTransactionSignature } from '@mysten/sui/verify'
import { blake2b } from '@noble/hashes/blake2.js'
import { bytesToHex } from '@noble/hashes/utils.js'
import { keystore } from '../index'
import { NativeError, parseNativeError } from '../native'
import { suiLedgerPath } from '../core/ledger/path'
import { KEYSTONE_MESSAGES, KeystoneError, toKeystoneError } from './errors'
import { decodeUR, encodeUR, keyDerivationUR, parseMultiAccounts, parseSuiSignature, responsePayload, signHashUR } from './ur'
import { KEYSTONE_TEST_MFP, KEYSTONE_TEST_PHRASE, mockKeystoneDevice } from './mock'
import { getKeystoneAccount, keystoneSignHash } from './usb'
import { suiSigningDigest, suiSigningDigestHex } from './prompt'
import { addKeystoneAccount, cancelKeystone, keystoneAccounts, listKeystones, useKeystonePrompt, type KeystoneStage } from './index'

const key = (i: number) => Ed25519Keypair.deriveKeypair(KEYSTONE_TEST_PHRASE, suiLedgerPath(i))
const OBJ = (n: number) => '0x' + n.toString(16).padStart(64, '0')
const DIGEST = '11111111111111111111111111111111'

function txBytes(sender: string) {
  const tx = new Transaction()
  tx.setSender(sender)
  tx.setGasPrice(1000)
  tx.setGasBudget(5_000_000)
  tx.setGasPayment([{ objectId: OBJ(1), version: '7', digest: DIGEST }])
  tx.transferObjects([tx.object(Inputs.ObjectRef({ objectId: OBJ(2), version: '3', digest: DIGEST }))], tx.pure.address(OBJ(0xbeef)))
  return tx.build()
}

/** The raw CBOR map of a UR, keys as numbers. */
const cborMap = (ur: string) => extend.decodeToDataItem(decodeUR(ur).cbor).getData() as Record<number, unknown>

const waitFor = async (f: () => boolean, ms = 4000) => {
  const end = Date.now() + ms
  while (!f()) {
    if (Date.now() > end) throw new Error('timed out')
    await new Promise((r) => setTimeout(r, 20))
  }
}

beforeEach(() => {
  Object.assign(mockKeystoneDevice(), { phrase: KEYSTONE_TEST_PHRASE, mfp: KEYSTONE_TEST_MFP, locked: false, reject: false, away: false, promptDelayMs: 0, requests: [], hashes: [] })
})

describe('key derivation call (qr-hardware-call)', () => {
  it('round trips: version 1, origin, ed25519 + SLIP-10 + chain SUI per Sui path', () => {
    const paths = [suiLedgerPath(0), suiLedgerPath(5)]
    const text = keyDerivationUR(paths)
    expect(text.startsWith('UR:QR-HARDWARE-CALL/')).toBe(true)
    const ur = decodeUR(text)
    expect(ur.type).toBe('qr-hardware-call')
    const call = QRHardwareCall.fromCBOR(ur.cbor)
    expect(call.getType()).toBe(0)
    expect(call.getOrigin()).toBe('Rare Wallet')
    expect(call.getVersion()).toBe(1)
    const schemas = (call.getParams() as KeyDerivation).getSchemas()
    expect(schemas.map((s) => s.getKeypath().getPath())).toEqual(["44'/784'/0'/0'/0'", "44'/784'/5'/0'/0'"])
    for (const s of schemas) {
      expect(s.getCurve()).toBe(Curve.ed25519)
      expect(s.getAlgo()).toBe(DerivationAlgorithm.slip10)
      expect(s.getChainType()).toBe('SUI')
    }
    // CBOR: {1: type 0, 2: key-derivation-call (tag 1301) of key-derivation-schema (tag 1302), 3: origin, 4: version 1}
    const m = cborMap(text)
    expect(m[1]).toBe(0)
    expect((m[2] as DataItem).getTag()).toBe(1301)
    const schema0 = ((m[2] as DataItem).getData() as Record<number, DataItem[]>)[1][0]
    expect(schema0.getTag()).toBe(1302)
    expect((schema0.getData() as Record<number, unknown>)[4]).toBe('SUI')
    expect(m[3]).toBe('Rare Wallet')
    expect(m[4]).toBe(1)
  })

  it('parses crypto-multi-accounts: master fingerprint and 32-byte keys on the asked paths', async () => {
    const answer = await (await import('./mock')).mockKeystoneSend(2, keyDerivationUR([suiLedgerPath(0), suiLedgerPath(1)]))
    const got = parseMultiAccounts(decodeUR(responsePayload(answer)))
    expect(got.mfp).toBe(KEYSTONE_TEST_MFP)
    expect(got.device).toBe('Keystone 3 Pro')
    expect(got.keys.map((k) => k.path)).toEqual([suiLedgerPath(0), suiLedgerPath(1)])
    expect(toBase64(got.keys[1].publicKey)).toBe(key(1).getPublicKey().toBase64())
  })
})

describe('sui-sign-hash-request', () => {
  it('round trips: request id (uuid, tag 37), message hash, keypath with source fingerprint, address', () => {
    const kp = key(0)
    const digest = blake2b(new Uint8Array([1, 2, 3]), { dkLen: 32 })
    const { ur, requestId } = signHashUR({ digest, path: suiLedgerPath(0), mfp: KEYSTONE_TEST_MFP, address: kp.toSuiAddress() })
    expect(requestId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    expect(ur.startsWith('UR:SUI-SIGN-HASH-REQUEST/')).toBe(true)

    const req = SuiSignHashRequest.fromCBOR(decodeUR(ur).cbor)
    expect(req.getMessageHash()).toBe(bytesToHex(digest))
    expect(req.getDerivationPaths()).toEqual(["44'/784'/0'/0'/0'"])
    expect(bytesToHex(req.getRequestId()!)).toBe(requestId.replace(/-/g, ''))
    expect('0x' + bytesToHex(req.getAddresses()![0])).toBe(kp.toSuiAddress())
    expect(req.getOrigin()).toBe('Rare Wallet')

    const m = cborMap(ur)
    expect((m[1] as DataItem).getTag()).toBe(37)
    expect(bytesToHex((m[1] as DataItem).getData() as Uint8Array)).toBe(requestId.replace(/-/g, ''))
    expect(m[2]).toBe(bytesToHex(digest))
    const paths = m[3] as DataItem[]
    expect(paths[0].getTag()).toBe(304)
    const keypath = CryptoKeypath.fromDataItem(paths[0])
    expect(keypath.getPath()).toBe("44'/784'/0'/0'/0'")
    expect(bytesToHex(keypath.getSourceFingerprint())).toBe(KEYSTONE_TEST_MFP)
    expect(bytesToHex((m[4] as Uint8Array[])[0])).toBe(kp.toSuiAddress().slice(2))
    expect(m[5]).toBe('Rare Wallet')
  })

  it('refuses digests that are not 32 bytes', () => {
    expect(() => signHashUR({ digest: new Uint8Array(31), path: suiLedgerPath(0), mfp: KEYSTONE_TEST_MFP, address: key(0).toSuiAddress() })).toThrow()
  })
})

describe('sui-signature parsing', () => {
  it('reads signature, public key and request id', () => {
    const sig = new Uint8Array(64).fill(9)
    const pk = key(0).getPublicKey().toRawBytes()
    const id = Buffer.from('0123456789abcdef0123456789abcdef', 'hex')
    const ur = encodeUR(new SuiSignature(Buffer.from(sig), id, Buffer.from(pk)).toUR())
    const got = parseSuiSignature(decodeUR(ur))
    expect([...got.signature]).toEqual([...sig])
    expect(toBase64(got.publicKey!)).toBe(toBase64(pk))
    expect(got.requestId).toBe('01234567-89ab-cdef-0123-456789abcdef')
  })

  it('refuses malformed answers', () => {
    const id = Buffer.alloc(16, 1)
    const short = encodeUR(new SuiSignature(Buffer.alloc(63), id).toUR())
    expect(() => parseSuiSignature(decodeUR(short))).toThrow(KeystoneError)
    expect(() => parseSuiSignature(decodeUR(keyDerivationUR([suiLedgerPath(0)])))).toThrow(KeystoneError)
    expect(() => decodeUR('hello')).toThrow(KeystoneError)
  })

  it('takes the device answer as JSON, a JSON string, or a bare UR', () => {
    expect(responsePayload('{"payload":"UR:X/Y"}')).toBe('UR:X/Y')
    expect(responsePayload('"ur:x/y"')).toBe('ur:x/y')
    expect(responsePayload('UR:X/Y')).toBe('UR:X/Y')
    expect(() => responsePayload('{"status":1}')).toThrow(KeystoneError)
  })
})

describe('signing digest', () => {
  it('is blake2b-256(intent || tx bytes), the exact bytes a Sui Ed25519 key signs', async () => {
    const kp = key(0)
    const bytes = await txBytes(kp.toSuiAddress())
    const digest = suiSigningDigest('transaction', bytes)
    expect(bytesToHex(digest)).toBe(bytesToHex(blake2b(Uint8Array.from([0, 0, 0, ...bytes]), { dkLen: 32 })))
    expect(suiSigningDigestHex('transaction', bytes)).toBe(bytesToHex(digest))
    // Ed25519 is deterministic: signing the digest equals the SDK's transaction signature
    const sdk = fromBase64((await kp.signTransaction(bytes)).signature).subarray(1, 65)
    expect(bytesToHex(await kp.sign(digest))).toBe(bytesToHex(sdk))
  })

  it('personal messages use the PersonalMessage intent over BCS bytes', async () => {
    const kp = key(0)
    const msg = new TextEncoder().encode('Sign in to Rare')
    const digest = suiSigningDigest('message', msg)
    expect(bytesToHex(digest)).toBe(bytesToHex(blake2b(Uint8Array.from([3, 0, 0, ...bcs.byteVector().serialize(msg).toBytes()]), { dkLen: 32 })))
    const sdk = fromBase64((await kp.signPersonalMessage(msg)).signature).subarray(1, 65)
    expect(bytesToHex(await kp.sign(digest))).toBe(bytesToHex(sdk))
  })
})

describe('Keystone over USB through the mock device', () => {
  it('reads one account and signs a hash, verified against the key', async () => {
    expect(await listKeystones()).toEqual([{ id: 'usb:mock', name: 'Keystone 3 Pro' }])
    const a = await getKeystoneAccount(2)
    expect(a).toMatchObject({ index: 2, path: suiLedgerPath(2), mfp: KEYSTONE_TEST_MFP, address: key(2).toSuiAddress() })
    const digest = blake2b(new Uint8Array([7]), { dkLen: 32 })
    const sig = await keystoneSignHash(a.path, a.mfp, a.address, digest)
    expect(await key(2).getPublicKey().verify(digest, sig)).toBe(true)
  })

  it('refuses a signature from another key and a wrong master fingerprint', async () => {
    const a = await getKeystoneAccount(0)
    const digest = blake2b(new Uint8Array([8]), { dkLen: 32 })
    mockKeystoneDevice().phrase = 'legal winner thank year wave sausage worth useful legal winner thank yellow'
    await expect(keystoneSignHash(a.path, a.mfp, a.address, digest, fromBase64(a.publicKey))).rejects.toMatchObject({ code: 'mismatch' })
    mockKeystoneDevice().phrase = KEYSTONE_TEST_PHRASE
    await expect(keystoneSignHash(a.path, 'deadbeef', a.address, digest)).rejects.toMatchObject({ code: 'mismatch' })
  })

  it('full flow: list accounts, add one, sign a transaction and a message through the keystore', async () => {
    const list = await keystoneAccounts(0, 3)
    expect(list.map((a) => a.address)).toEqual([0, 1, 2].map((i) => key(i).toSuiAddress()))
    expect(mockKeystoneDevice().requests).toEqual(['qr-hardware-call'])

    const acc = await addKeystoneAccount(1, 'Cold')
    expect(acc).toMatchObject({ kind: 'keystone', label: 'Cold', keystone: { path: suiLedgerPath(1), mfp: KEYSTONE_TEST_MFP, device: 'Keystone 3 Pro' } })
    expect(mockKeystoneDevice().requests).toHaveLength(1) // no second approval

    const signer = await keystore.getSigner(acc.address)
    const bytes = await txBytes(acc.address)
    const seen: { stage: KeystoneStage; digestHex: string | null }[] = []
    const unsub = useKeystonePrompt.subscribe((s) => seen.push({ stage: s.stage, digestHex: s.digestHex }))
    const sig = await signer.signTransaction(bytes)
    unsub()
    await expect(verifyTransactionSignature(bytes, sig, { address: acc.address })).resolves.toBeTruthy()
    const want = bytesToHex(blake2b(Uint8Array.from([0, 0, 0, ...bytes]), { dkLen: 32 }))
    expect(mockKeystoneDevice().hashes).toEqual([want])
    expect(seen.find((s) => s.stage === 'confirm')?.digestHex).toBe(want)
    expect(useKeystonePrompt.getState().stage).toBe('idle')

    const msg = new TextEncoder().encode('Sign in to Rare')
    const r = await signer.signPersonalMessage(msg)
    await expect(verifyPersonalMessageSignature(msg, r.signature, { address: acc.address })).resolves.toBeTruthy()
  })

  it('waits while locked, reports rejection, and cancels', async () => {
    const acc = (await keystore.listAccounts()).find((a) => a.kind === 'keystone')!
    const signer = await keystore.getSigner(acc.address)
    const bytes = await txBytes(acc.address)

    mockKeystoneDevice().locked = true
    const p = signer.signTransaction(bytes)
    await waitFor(() => useKeystonePrompt.getState().stage === 'unlock')
    expect(useKeystonePrompt.getState().message).toBe('Unlock your Keystone')
    mockKeystoneDevice().locked = false
    await expect(p).resolves.toBeTruthy()

    mockKeystoneDevice().reject = true
    await expect(signer.signTransaction(bytes)).rejects.toMatchObject({ code: 'rejected' })
    expect(useKeystonePrompt.getState()).toMatchObject({ stage: 'error', message: 'Rejected on your Keystone.' })

    mockKeystoneDevice().reject = false
    mockKeystoneDevice().promptDelayMs = 300
    const q = signer.signTransaction(bytes)
    await waitFor(() => useKeystonePrompt.getState().stage === 'confirm')
    cancelKeystone()
    await expect(q).rejects.toMatchObject({ code: 'cancelled' })
  })

  it('asks for the home screen when the device refuses key export elsewhere', async () => {
    mockKeystoneDevice().away = true
    const p = keystoneAccounts(0, 1)
    await waitFor(() => useKeystonePrompt.getState().stage === 'home-screen')
    mockKeystoneDevice().away = false
    await expect(p).resolves.toHaveLength(1)
  })
})

describe('error mapping', () => {
  const code = (e: unknown) => toKeystoneError(e).code
  it('maps Rust transport codes and device text, keeps the udev fix', () => {
    const fix = 'pkexec sh -c "..."'
    expect(toKeystoneError(parseNativeError(`udev|No access|${JSON.stringify({ fix })}`)).fix).toBe(fix)
    expect(code(new NativeError('notfound', 'x'))).toBe('no-device')
    expect(code(new NativeError('locked', 'x'))).toBe('locked')
    expect(code(new NativeError('rejected', 'x'))).toBe('rejected')
    expect(code(new NativeError('timeout', 'x'))).toBe('timeout')
    expect(code(new NativeError('usb-off', 'x'))).toBe('usb-off')
    expect(code(new NativeError('rejected', 'Export address is just allowed on specific pages'))).toBe('home-screen')
    expect(code(new NativeError('error', 'this view type is not supported'))).toBe('unsupported')
    expect(code(new NativeError('error', 'Waiting for user approval'))).toBe('busy')
    expect(code(new Error('boom'))).toBe('unknown')
    expect(new KeystoneError('usb-off').message).toMatch(/Settings > Connection/)
    // as the Rust transport (eapdu.rs status_error) sends them
    expect(code(parseNativeError('usb-off|Open the Keystone home screen and turn on USB in Settings > Connection'))).toBe('usb-off')
    expect(code(parseNativeError('mismatch|This Keystone holds a different wallet'))).toBe('mismatch')
    expect(code(parseNativeError('unsupported|Your Keystone does not support this yet. Update its firmware'))).toBe('unsupported')
    expect(code(parseNativeError('busy|Finish or reject the request on your Keystone first'))).toBe('busy')
    expect(code(parseNativeError('disconnected|Your Keystone was disconnected'))).toBe('disconnected')
    expect(toKeystoneError(parseNativeError('rejected|Wrong password on your Keystone')).message).toMatch(/Wrong password/)
  })
  it('plain messages without em dashes', () => {
    for (const m of Object.values(KEYSTONE_MESSAGES)) {
      expect(m.length).toBeGreaterThan(10)
      expect(m).not.toMatch(/—/)
    }
  })
})
