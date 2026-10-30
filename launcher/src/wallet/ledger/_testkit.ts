/**
 * For tests and the browser `?mockhw` stand-in (mock.ts): a fake Ledger
 * running a fake Sui app behind a real @ledgerhq Transport, so tests drive the real
 * @mysten/ledgerjs-hw-app-sui client (APDU framing, the chunked block protocol, BIP32 payloads)
 * end to end. Keys come from a test phrase; signatures are real Ed25519 over
 * blake2b-256 of the intent message, exactly what the device signs.
 */
import { Buffer } from './shims/buffer'
import Transport from '@ledgerhq/hw-transport'
import { blake2b } from '@noble/hashes/blake2.js'
import { sha256 } from '@noble/hashes/sha2.js'
import { Ed25519Keypair } from '@mysten/sui/keypairs/ed25519'

export const TEST_PHRASE = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about'

export interface SignCall {
  path: string
  /** the bytes the device was asked to sign (intent message) */
  payload: Uint8Array
  /** BCS objects sent for clear signing */
  objects: Uint8Array[]
}

export interface FakeDevice {
  app: string
  locked: boolean
  /** device answers the next prompt (sign / show address) with a "no" */
  reject: boolean
  /** clear signing impossible without objects: answer like the Sui app with blind signing off */
  blindOff: boolean
  phrase: string
  signs: SignCall[]
  shown: string[]
  /** every APDU sent, hex */
  apdus: string[]
  /** ms the device "waits for the person" on a sign or show-address prompt (mock UI only) */
  promptDelayMs: number
}

const u32le = (b: Uint8Array, o: number) => (b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24)) >>> 0
const hex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('')
const sw = (code: number) => Uint8Array.of(code >> 8, code & 0xff)
const ok = (body: Uint8Array) => Buffer.from([...body, 0x90, 0x00])
const status = (code: number) => Buffer.from(sw(code))

export function parseBip32(p: Uint8Array): string {
  const n = p[0]
  const parts: string[] = []
  for (let i = 0; i < n; i++) {
    const v = u32le(p, 1 + i * 4)
    parts.push(v >= 0x80000000 ? `${v - 0x80000000}'` : String(v))
  }
  return `m/${parts.join('/')}`
}

export const deviceKey = (phrase: string, path: string) => Ed25519Keypair.deriveKeypair(phrase, path)

export class FakeLedgerTransport extends Transport {
  dev: FakeDevice
  private session: { ins: number; params: Uint8Array[]; want: Uint8Array[]; cur: number; acc: number[] } | null = null

  constructor(o: Partial<FakeDevice> = {}) {
    super()
    this.dev = { app: 'Sui', locked: false, reject: false, blindOff: false, phrase: TEST_PHRASE, signs: [], shown: [], apdus: [], promptDelayMs: 0, ...o }
  }

  async exchange(apdu: Buffer): Promise<Buffer> {
    this.dev.apdus.push(hex(apdu))
    const [cla, ins] = apdu
    const data = new Uint8Array(apdu.subarray(5, 5 + apdu[4]))
    if (this.dev.locked) return status(0x5515)
    if (cla === 0xb0 && ins === 0x01) {
      const name = new TextEncoder().encode(this.dev.app)
      const ver = new TextEncoder().encode('1.4.0')
      return ok(Uint8Array.of(0x01, name.length, ...name, ver.length, ...ver, 0x01, 0x00))
    }
    if (this.dev.app !== 'Sui') return status(0x6e00)
    if (cla !== 0x00) return status(0x6e00)
    return this.block(ins, data)
  }

  /** The Sui app's chunked block protocol: START lists param hashes, the device pulls each chain. */
  private async block(ins: number, data: Uint8Array): Promise<Buffer> {
    const op = data[0]
    if (op === 0) {
      const want: Uint8Array[] = []
      for (let i = 1; i + 32 <= data.length; i += 32) want.push(data.subarray(i, i + 32))
      this.session = { ins, params: [], want, cur: 0, acc: [] }
      return this.next()
    }
    if (op === 1 && this.session) {
      const linked = data.subarray(1)
      const s = this.session
      const expect = s.want[s.cur]
      if (hex(sha256(linked)) !== hex(expect)) return status(0x6a80)
      s.acc.push(...linked.subarray(32))
      const nextHash = linked.subarray(0, 32)
      if (nextHash.every((b) => b === 0)) {
        s.params.push(Uint8Array.from(s.acc))
        s.acc = []
        s.cur++
      } else {
        s.want[s.cur] = Uint8Array.from(nextHash)
      }
      return this.next()
    }
    return status(0x6a80)
  }

  private async next(): Promise<Buffer> {
    const s = this.session!
    if (s.cur < s.want.length) return ok(Uint8Array.of(0x02, ...s.want[s.cur]))
    this.session = null
    return this.answer(s.ins, s.params)
  }

  private async answer(ins: number, params: Uint8Array[]): Promise<Buffer> {
    if (ins === 0x00) return ok(Uint8Array.of(0x01, 1, 4, 0))
    if (ins === 0x01 || ins === 0x02) {
      const path = parseBip32(params[0])
      if (ins === 0x01) {
        await this.wait()
        if (this.dev.reject) return status(0x6e04)
        this.dev.shown.push(path)
      }
      const kp = deviceKey(this.dev.phrase, path)
      const pk = kp.getPublicKey().toRawBytes()
      const addr = Uint8Array.from(Buffer.from(kp.toSuiAddress().slice(2), 'hex'))
      return ok(Uint8Array.of(0x01, pk.length, ...pk, addr.length, ...addr))
    }
    if (ins === 0x03) {
      const txp = params[0]
      const len = u32le(txp, 0)
      const payload = txp.subarray(4, 4 + len)
      const path = parseBip32(params[1])
      const objects: Uint8Array[] = []
      if (params[2]) {
        const n = u32le(params[2], 0)
        let o = 4
        for (let i = 0; i < n; i++) {
          const l = u32le(params[2], o)
          objects.push(params[2].subarray(o + 4, o + 4 + l))
          o += 4 + l
        }
      }
      this.dev.signs.push({ path, payload: Uint8Array.from(payload), objects })
      if (this.dev.blindOff && payload[0] === 0 && objects.length === 0) return status(0x6808)
      await this.wait()
      if (this.dev.reject) return status(0x6e04)
      const sig = await deviceKey(this.dev.phrase, path).sign(blake2b(payload, { dkLen: 32 }))
      return ok(Uint8Array.of(0x01, ...sig))
    }
    return status(0x6d00)
  }

  private wait() {
    return this.dev.promptDelayMs ? new Promise((r) => setTimeout(r, this.dev.promptDelayMs)) : Promise.resolve()
  }

  async close() {}
}
