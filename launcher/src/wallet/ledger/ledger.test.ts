import { describe, expect, it } from 'vitest'
import Sui from '@mysten/ledgerjs-hw-app-sui'
import { Transaction, Inputs } from '@mysten/sui/transactions'
import { bcs } from '@mysten/sui/bcs'
import { fromBase64, toBase64 } from '@mysten/sui/utils'
import { verifyPersonalMessageSignature, verifyTransactionSignature } from '@mysten/sui/verify'
import { TransportStatusError } from '@ledgerhq/errors'
import { FakeLedgerTransport, TEST_PHRASE, deviceKey } from './_testkit'
import { addressFromPublicKey, isSuiLedgerPath, ledgerPathIndex, suiLedgerPath } from '../core/ledger/path'
import { LEDGER_MESSAGES, LedgerError, toLedgerError } from './errors'
import { NativeError, parseNativeError } from '../native'
import {
  assertSuiApp,
  clearSigningObjectIds,
  ledgerSignPersonalMessage,
  ledgerSignTransaction,
  parseAppAndVersion,
  readLedgerAccount,
  serializeLedgerSignature,
  type SuiLedgerApp,
} from './sign'
import { checkSuiApp } from './device'

const app = (t: FakeLedgerTransport) => new Sui(t) as unknown as SuiLedgerApp
const OBJ = (n: number) => '0x' + n.toString(16).padStart(64, '0')
const DIGEST = '11111111111111111111111111111111'

function txBytes(sender: string, o: { gasOwner?: string } = {}) {
  const tx = new Transaction()
  tx.setSender(sender)
  if (o.gasOwner) tx.setGasOwner(o.gasOwner)
  tx.setGasPrice(1000)
  tx.setGasBudget(5_000_000)
  tx.setGasPayment([{ objectId: OBJ(1), version: '7', digest: DIGEST }])
  tx.transferObjects([tx.object(Inputs.ObjectRef({ objectId: OBJ(2), version: '3', digest: DIGEST }))], tx.pure.address(OBJ(0xbeef)))
  return tx.build()
}

describe('derivation paths', () => {
  it("builds m/44'/784'/i'/0'/0' and reads the index back", () => {
    expect(suiLedgerPath(0)).toBe("m/44'/784'/0'/0'/0'")
    expect(suiLedgerPath(7)).toBe("m/44'/784'/7'/0'/0'")
    expect(ledgerPathIndex(suiLedgerPath(12))).toBe(12)
    expect(isSuiLedgerPath("m/44'/784'/0'/0'/0'")).toBe(true)
  })
  it('rejects other paths and bad indexes', () => {
    expect(() => suiLedgerPath(-1)).toThrow()
    expect(() => suiLedgerPath(1.5)).toThrow()
    expect(() => suiLedgerPath(0x80000000)).toThrow()
    expect(ledgerPathIndex("m/44'/60'/0'/0/0")).toBeNull()
    expect(ledgerPathIndex("m/44'/784'/0'/0/0")).toBeNull()
    expect(ledgerPathIndex("m/44'/784'/0'/0'/0'/1")).toBeNull()
  })
})

describe('address from public key', () => {
  it('matches the Sui Ed25519 address for raw and base64 keys', () => {
    const kp = deviceKey(TEST_PHRASE, suiLedgerPath(3))
    const raw = kp.getPublicKey().toRawBytes()
    expect(addressFromPublicKey(raw)).toBe(kp.toSuiAddress())
    expect(addressFromPublicKey(toBase64(raw))).toBe(kp.toSuiAddress())
    expect(() => addressFromPublicKey(new Uint8Array(33))).toThrow()
  })
})

describe('Sui app over a mocked transport', () => {
  it('lists the first 3 addresses at the standard paths', async () => {
    const t = new FakeLedgerTransport()
    for (let i = 0; i < 3; i++) {
      const r = await readLedgerAccount(app(t), suiLedgerPath(i))
      expect(r.address).toBe(deviceKey(TEST_PHRASE, suiLedgerPath(i)).toSuiAddress())
    }
    expect(t.dev.shown).toEqual([])
  })

  it('shows an address on the device only when asked', async () => {
    const t = new FakeLedgerTransport()
    await readLedgerAccount(app(t), suiLedgerPath(1), true)
    expect(t.dev.shown).toEqual([suiLedgerPath(1)])
    t.dev.reject = true
    await expect(readLedgerAccount(app(t), suiLedgerPath(1), true)).rejects.toMatchObject({ code: 'rejected' })
  })

  it('signs a transaction: intent payload, path, clear-signing objects, serialized flag||sig||pubkey', async () => {
    const t = new FakeLedgerTransport()
    const path = suiLedgerPath(0)
    const kp = deviceKey(TEST_PHRASE, path)
    const pk = kp.getPublicKey().toRawBytes()
    const bytes = await txBytes(kp.toSuiAddress())
    const seen: string[][] = []
    const sig = await ledgerSignTransaction(app(t), {
      path,
      publicKey: pk,
      txBytes: bytes,
      objects: async (ids) => (seen.push(ids), ids.map((_, i) => Uint8Array.of(0xaa, i))),
    })
    // request formatting
    const call = t.dev.signs[0]
    expect(call.path).toBe(path)
    expect([...call.payload.subarray(0, 3)]).toEqual([0, 0, 0]) // TransactionData, V0, Sui
    expect(toBase64(call.payload.subarray(3))).toBe(toBase64(bytes))
    expect(seen[0]).toEqual([OBJ(1), OBJ(2)])
    expect(call.objects.map((o) => [...o])).toEqual([[0xaa, 0], [0xaa, 1]])
    // response -> Sui signature
    const raw = fromBase64(sig)
    expect(raw.length).toBe(1 + 64 + 32)
    expect(raw[0]).toBe(0x00)
    expect(toBase64(raw.subarray(65))).toBe(toBase64(pk))
    await expect(verifyTransactionSignature(bytes, sig, { address: kp.toSuiAddress() })).resolves.toBeTruthy()
  })

  it('still signs when the objects for clear signing cannot be read', async () => {
    const t = new FakeLedgerTransport()
    const kp = deviceKey(TEST_PHRASE, suiLedgerPath(0))
    const bytes = await txBytes(kp.toSuiAddress())
    const sig = await ledgerSignTransaction(app(t), {
      path: suiLedgerPath(0),
      publicKey: kp.getPublicKey().toRawBytes(),
      txBytes: bytes,
      objects: async () => {
        throw new Error('network down')
      },
    })
    expect(t.dev.signs[0].objects).toEqual([])
    await expect(verifyTransactionSignature(bytes, sig, { address: kp.toSuiAddress() })).resolves.toBeTruthy()
  })

  it('signs a sponsored transaction as the sender; the sponsor signs separately', async () => {
    const t = new FakeLedgerTransport()
    const kp = deviceKey(TEST_PHRASE, suiLedgerPath(0))
    const sponsor = deviceKey(TEST_PHRASE, suiLedgerPath(9))
    const bytes = await txBytes(kp.toSuiAddress(), { gasOwner: sponsor.toSuiAddress() })
    const senderSig = await ledgerSignTransaction(app(t), { path: suiLedgerPath(0), publicKey: kp.getPublicKey().toRawBytes(), txBytes: bytes })
    const sponsorSig = (await sponsor.signTransaction(bytes)).signature
    await expect(verifyTransactionSignature(bytes, senderSig, { address: kp.toSuiAddress() })).resolves.toBeTruthy()
    await expect(verifyTransactionSignature(bytes, sponsorSig, { address: sponsor.toSuiAddress() })).resolves.toBeTruthy()
    expect(Transaction.from(bytes).getData().gasData.owner).toBe(sponsor.toSuiAddress())
  })

  it('signs a personal message with the PersonalMessage intent over BCS bytes', async () => {
    const t = new FakeLedgerTransport()
    const kp = deviceKey(TEST_PHRASE, suiLedgerPath(2))
    const msg = new TextEncoder().encode('Sign in to Rare')
    const sig = await ledgerSignPersonalMessage(app(t), { path: suiLedgerPath(2), publicKey: kp.getPublicKey().toRawBytes(), message: msg })
    const p = t.dev.signs[0].payload
    expect([...p.subarray(0, 3)]).toEqual([3, 0, 0])
    expect(toBase64(p.subarray(3))).toBe(toBase64(bcs.byteVector().serialize(msg).toBytes()))
    expect(t.dev.signs[0].objects).toEqual([])
    await expect(verifyPersonalMessageSignature(msg, sig, { address: kp.toSuiAddress() })).resolves.toBeTruthy()
  })

  it('refuses when the device holds a different key at that path', async () => {
    const t = new FakeLedgerTransport({ phrase: 'legal winner thank year wave sausage worth useful legal winner thank yellow' })
    const kp = deviceKey(TEST_PHRASE, suiLedgerPath(0))
    await expect(
      ledgerSignTransaction(app(t), { path: suiLedgerPath(0), publicKey: kp.getPublicKey().toRawBytes(), txBytes: await txBytes(kp.toSuiAddress()) }),
    ).rejects.toMatchObject({ code: 'mismatch' })
    expect(t.dev.signs).toEqual([])
  })

  it('maps device answers: rejected, blind signing off, locked', async () => {
    const kp = deviceKey(TEST_PHRASE, suiLedgerPath(0))
    const base = { path: suiLedgerPath(0), publicKey: kp.getPublicKey().toRawBytes(), txBytes: await txBytes(kp.toSuiAddress()) }
    await expect(ledgerSignTransaction(app(new FakeLedgerTransport({ reject: true })), base)).rejects.toMatchObject({ code: 'rejected' })
    await expect(ledgerSignTransaction(app(new FakeLedgerTransport({ blindOff: true })), base)).rejects.toMatchObject({ code: 'blind-signing' })
    await expect(ledgerSignTransaction(app(new FakeLedgerTransport({ locked: true })), base)).rejects.toMatchObject({ code: 'locked' })
  })

  it('checks the open app: Sui passes, dashboard and other apps say what to do', async () => {
    await expect(checkSuiApp(new FakeLedgerTransport())).resolves.toEqual({ name: 'Sui', version: '1.4.0' })
    await expect(checkSuiApp(new FakeLedgerTransport({ app: 'BOLOS' }))).rejects.toMatchObject({ code: 'app-closed' })
    const wrong = await checkSuiApp(new FakeLedgerTransport({ app: 'Bitcoin' })).catch((e) => e)
    expect(wrong).toMatchObject({ code: 'wrong-app' })
    expect(wrong.message).toContain('Bitcoin app open')
    await expect(checkSuiApp(new FakeLedgerTransport({ locked: true }))).rejects.toMatchObject({ code: 'locked' })
  })
})

describe('signature formatting', () => {
  it('serializes Ed25519 device signatures and refuses malformed ones', () => {
    const pk = new Uint8Array(32).fill(7)
    const s = fromBase64(serializeLedgerSignature(new Uint8Array(64).fill(1), pk))
    expect([s[0], s.length]).toEqual([0, 97])
    expect(() => serializeLedgerSignature(new Uint8Array(63), pk)).toThrow(LedgerError)
  })
  it('parses GET_APP_AND_VERSION and judges the app', () => {
    const name = new TextEncoder().encode('Sui')
    const ver = new TextEncoder().encode('1.4.0')
    expect(parseAppAndVersion(Uint8Array.of(1, 3, ...name, 5, ...ver, 1, 0))).toEqual({ name: 'Sui', version: '1.4.0' })
    expect(() => assertSuiApp({ name: 'Sui' })).not.toThrow()
    expect(() => assertSuiApp({ name: 'BOLOS' })).toThrow('Open the Sui app')
  })
  it('lists gas and owned inputs for clear signing, once each', async () => {
    const bytes = await txBytes(OBJ(5))
    expect(clearSigningObjectIds(bytes)).toEqual([OBJ(1), OBJ(2)])
  })
})

describe('error mapping', () => {
  const code = (e: unknown) => toLedgerError(e).code
  it('maps status words', () => {
    expect(code(new TransportStatusError(0x5515))).toBe('locked')
    expect(code({ statusCode: 0x6b0c })).toBe('locked')
    expect(code({ statusCode: 0x6985 })).toBe('rejected')
    expect(code({ statusCode: 0x6e04 })).toBe('rejected')
    expect(code({ statusCode: 0x6808 })).toBe('blind-signing')
    expect(code({ statusCode: 0x6e00 })).toBe('app-closed')
    expect(code({ statusCode: 0x6d00 })).toBe('app-closed')
    expect(code({ statusCode: 0x6511 })).toBe('app-closed')
    const odd = toLedgerError({ statusCode: 0x6f42 })
    expect([odd.code, odd.message.includes('0x6f42')]).toEqual(['unknown', true])
  })
  it('maps transport errors', () => {
    expect(code({ name: 'LockedDeviceError' })).toBe('locked')
    expect(code({ name: 'TransportRaceCondition' })).toBe('busy')
    expect(code({ name: 'TransportError', id: 'TransportLocked' })).toBe('busy')
    expect(code({ name: 'DisconnectedDeviceDuringOperation' })).toBe('disconnected')
    expect(code({ name: 'DisconnectedDevice' })).toBe('disconnected')
    expect(code({ name: 'UpdateYourApp' })).toBe('update-app')
    expect(code(new Error('boom'))).toBe('unknown')
  })
  it('maps Rust transport errors and keeps the udev fix', () => {
    const fix = 'echo \'SUBSYSTEMS=="usb", ATTRS{idVendor}=="2c97", MODE="0660", TAG+="uaccess"\' | pkexec tee /etc/udev/rules.d/20-ledger.rules'
    const udev = toLedgerError(new NativeError('udev', 'no access', { fix }))
    expect([udev.code, udev.fix]).toEqual(['udev', fix])
    expect(udev.message).toMatch(/permission/)
    expect(code(new NativeError('ble-off', 'x'))).toBe('ble-off')
    expect(code(new NativeError('ble-pairing', 'x'))).toBe('ble-pairing')
    expect(code(new NativeError('busy', 'x'))).toBe('busy')
    expect(code(new NativeError('notfound', 'x'))).toBe('no-device')
    expect(code(new NativeError('disconnected', 'x'))).toBe('disconnected')
    expect(code(new NativeError('timeout', 'x'))).toBe('timeout')
    expect(code(new NativeError('cancelled', 'x'))).toBe('cancelled')
    expect(code(new NativeError('weird', 'x'))).toBe('unknown')
    // as Rust sends it, through parseNativeError
    expect(toLedgerError(parseNativeError(`udev|No access|${JSON.stringify({ fix })}`)).fix).toBe(fix)
  })
  it('keeps LedgerErrors and gives plain messages without em dashes', () => {
    const e = new LedgerError('timeout')
    expect(toLedgerError(e)).toBe(e)
    for (const c of Object.keys(LEDGER_MESSAGES) as (keyof typeof LEDGER_MESSAGES)[]) {
      const m = new LedgerError(c).message
      expect(m.length).toBeGreaterThan(10)
      expect(m).not.toMatch(/—/)
    }
    expect(new LedgerError('blind-signing').message).toMatch(/Settings.*Blind signing/)
  })
})
