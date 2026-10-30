import { describe, expect, it } from 'vitest'
import { Ed25519Keypair } from '@mysten/sui/keypairs/ed25519'
import { Secp256k1Keypair } from '@mysten/sui/keypairs/secp256k1'
import { Secp256r1Keypair } from '@mysten/sui/keypairs/secp256r1'
import { toBase64 } from '@mysten/sui/utils'
import { verifyPersonalMessageSignature } from '@mysten/sui/verify'
import { MemoryKV } from '../store/kv'
import { Keystore, LockedError, NeedsPasswordError } from './keystore'
import { keypairFromMnemonic, newMnemonic } from './derive'
import { WatchOnlyError } from './signer'
import { memoryVault } from '../../testing/memoryVault'

const PHRASE = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about'
const PIN = { secret: '123456', kind: 'pin' as const }
const mk = () => {
  const disk = new MemoryKV()
  const vault = memoryVault()
  return { disk, vault, ks: new Keystore({ disk, vault }) }
}

describe('derivation', () => {
  it('same phrase -> same address; index changes it; Sui path m/44/784/i/0/0', () => {
    const a = keypairFromMnemonic(PHRASE).toSuiAddress()
    expect(keypairFromMnemonic(`  ${PHRASE.toUpperCase()} `).toSuiAddress()).toBe(a)
    // known vector: Sui CLI / Slush address for the all-abandon phrase, index 0
    expect(a).toBe('0x5e93a736d04fbb25737aa40bee40171ef79f65fae833749e3c089fe7cc2161f1')
    expect(keypairFromMnemonic(PHRASE, 1).toSuiAddress()).not.toBe(a)
    expect(() => keypairFromMnemonic('not a real phrase')).toThrow()
  })
  it('24-word phrases', () => {
    expect(newMnemonic(256).split(' ')).toHaveLength(24)
    expect(newMnemonic().split(' ')).toHaveLength(12)
  })
})

describe('vault lifecycle', () => {
  it('first phrase needs a PIN, creates the vault, and signs', async () => {
    const { ks } = mk()
    await expect(ks.addPhrase(PHRASE)).rejects.toBeInstanceOf(NeedsPasswordError)
    await expect(ks.addPhrase(PHRASE, { pin: { secret: '12', kind: 'pin' } })).rejects.toThrow('6 digits')
    const acc = await ks.addPhrase(PHRASE, { pin: PIN })
    expect(acc).toMatchObject({ kind: 'native', hd: { index: 0 } })
    expect(acc.address).toBe(keypairFromMnemonic(PHRASE).toSuiAddress())
    expect(await ks.isUnlocked()).toBe(true)
    expect((await ks.getSigner(acc.address)).address).toBe(acc.address)
  })

  it('locks, refuses to sign, unlocks with the right PIN only', async () => {
    const { ks } = mk()
    const acc = await ks.addPhrase(undefined, { pin: PIN })
    await ks.lock()
    expect(await ks.isUnlocked()).toBe(false)
    expect(await ks.needsUnlock(acc.address)).toBe(true)
    await expect(ks.getSigner(acc.address)).rejects.toBeInstanceOf(LockedError)
    await expect(ks.unlock('000000')).rejects.toMatchObject({ code: 'wrong' })
    await ks.unlock('123456')
    expect((await ks.getSigner(acc.address)).address).toBe(acc.address)
  })

  it('never puts a phrase in public metadata', async () => {
    const { ks, disk } = mk()
    await ks.addPhrase(PHRASE, { pin: PIN })
    expect(JSON.stringify(await disk.get('rare.meta'))).not.toContain('abandon')
  })

  it('a UI reload picks the payload back up while the vault stays unlocked', async () => {
    const { ks, disk, vault } = mk()
    const acc = await ks.addPhrase(PHRASE, { pin: PIN })
    const revived = new Keystore({ disk, vault })
    expect((await revived.getSigner(acc.address)).address).toBe(acc.address)
  })
})

describe('accounts', () => {
  it('derives the next index from the same phrase, stores the phrase once', async () => {
    const { ks } = mk()
    const a = await ks.addPhrase(PHRASE, { pin: PIN })
    const b = await ks.deriveNext()
    const c = await ks.deriveNext({ label: 'Games' })
    expect([a.hd?.index, b.hd?.index, c.hd?.index]).toEqual([0, 1, 2])
    expect(b.address).toBe(keypairFromMnemonic(PHRASE, 1).toSuiAddress())
    expect(c.label).toBe('Games')
    expect(await ks.revealMnemonic('123456', c.address)).toBe(PHRASE)
    await ks.remove(b.address)
    expect((await ks.deriveNext()).hd?.index).toBe(1)
  })

  it('rejects duplicates and keeps the selection while locked', async () => {
    const { ks } = mk()
    const a = await ks.addPhrase(PHRASE, { pin: PIN })
    const b = await ks.addPhrase(newMnemonic(), { label: 'Second' })
    await ks.select(b.address)
    await ks.lock()
    expect((await ks.current())?.address).toBe(b.address)
    await ks.unlock('123456')
    await expect(ks.addPhrase(PHRASE)).rejects.toThrow('already exists')
    expect((await ks.listAccounts()).map((x) => x.address)).toEqual([a.address, b.address])
  })

  it('imports a suiprivkey of every scheme', async () => {
    const { ks } = mk()
    await ks.addPhrase(undefined, { pin: PIN })
    for (const kp of [new Ed25519Keypair(), new Secp256k1Keypair(), new Secp256r1Keypair()]) {
      const acc = await ks.addPrivateKey(kp.getSecretKey(), { label: 'Imported' })
      expect(acc).toMatchObject({ address: kp.toSuiAddress(), kind: 'native', label: 'Imported' })
      expect((await ks.getSigner(acc.address)).address).toBe(kp.toSuiAddress())
      expect(await ks.revealPrivateKey('123456', acc.address)).toBe(kp.getSecretKey())
      await expect(ks.revealMnemonic('123456', acc.address)).rejects.toThrow('No recovery phrase')
    }
    await expect(ks.addPrivateKey('suiprivkey1nope')).rejects.toThrow('Invalid private key')
  })

  it('watch accounts never sign; Ledger without a provider is unsupported', async () => {
    const { ks } = mk()
    const w = await ks.addWatch('0x' + 'ab'.repeat(32))
    await expect(ks.getSigner(w.address)).rejects.toBeInstanceOf(WatchOnlyError)
    const pk = toBase64(new Ed25519Keypair().getPublicKey().toRawBytes())
    const l = await ks.addLedger(pk, "m/44'/784'/0'/0'/0'")
    expect(l.kind).toBe('ledger')
    await expect(ks.getSigner(l.address)).rejects.toThrow('not available')
  })
})

describe('passkey accounts (1-of-2 multisig with a device key)', () => {
  it('signs with the device key; the multisig signature verifies for the account address', async () => {
    const { ks } = mk()
    const passkey = new Secp256r1Keypair() // stands in for the passkey's P-256 key
    const device = new Ed25519Keypair()
    const acc = await ks.addPasskey({
      credentialId: toBase64(new Uint8Array([1, 2, 3])),
      publicKey: toBase64(passkey.getPublicKey().toRawBytes()),
      deviceKey: toBase64(device.getPublicKey().toRawBytes()),
      deviceSecret: device.getSecretKey(),
      pin: PIN,
    })
    expect(acc.kind).toBe('passkey')
    expect(await ks.hasDeviceKey(acc.address)).toBe(true)
    const signer = await ks.getSigner(acc.address)
    expect(signer.address).toBe(acc.address)
    const msg = new TextEncoder().encode('rare')
    const { signature } = await signer.signPersonalMessage(msg)
    const pk = await verifyPersonalMessageSignature(msg, signature)
    expect(pk.toSuiAddress()).toBe(acc.address)
  })

  it('without the device key it needs the passkey provider', async () => {
    const { ks } = mk()
    const acc = await ks.addPasskey({
      credentialId: 'AQID',
      publicKey: toBase64(new Secp256r1Keypair().getPublicKey().toRawBytes()),
      deviceKey: toBase64(new Ed25519Keypair().getPublicKey().toRawBytes()),
    })
    expect(await ks.hasVault()).toBe(false)
    await expect(ks.getSigner(acc.address)).rejects.toThrow('not available')
  })

  it('refuses a device secret that does not match the device key', async () => {
    const { ks } = mk()
    await expect(
      ks.addPasskey({
        credentialId: 'AQID',
        publicKey: toBase64(new Secp256r1Keypair().getPublicKey().toRawBytes()),
        deviceKey: toBase64(new Ed25519Keypair().getPublicKey().toRawBytes()),
        deviceSecret: new Ed25519Keypair().getSecretKey(),
        pin: PIN,
      }),
    ).rejects.toThrow('does not match')
  })
})
