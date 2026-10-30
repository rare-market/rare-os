import type { RareAccount } from '../types'
import type { KV } from '../store/kv'
import type { SecretKind, VaultPort } from '../../native'
import { isValidMnemonic, keypairFromMnemonic, keypairFromSecret, newMnemonic, normalizeMnemonic, publicKeyB64 } from './derive'
import type { Keypair } from '@mysten/sui/cryptography'
import { Ed25519Keypair, Ed25519PublicKey } from '@mysten/sui/keypairs/ed25519'
import { isValidSuiAddress, normalizeSuiAddress, fromBase64, toBase64 } from '@mysten/sui/utils'
import { NativeSigner, UnsupportedAccountKind, WatchOnlyError, type AccountSigner } from './signer'
import { addressFromPublicKey, isSuiLedgerPath } from '../ledger/path'
import { blake2b } from '@noble/hashes/blake2.js'
import { bytesToHex } from '@noble/hashes/utils.js'
import { hkdf } from '@noble/hashes/hkdf.js'
import { sha256 } from '@noble/hashes/sha2.js'
import { decodeSuiPrivateKey } from '@mysten/sui/cryptography'
import { passkeyMultisig, MultisigDeviceSigner } from '../passkey/multisig'

/**
 * Secret material, sealed in the vault (Rust: Argon2id + AES-256-GCM).
 * - phrase: one recovery phrase, shared by every account derived from it (by index)
 * - key: an imported private key (`suiprivkey1…`), one account
 * - device: the on-device member key of a passkey account
 */
type Secret = { kind: 'phrase'; mnemonic: string } | { kind: 'key'; secretKey: string } | { kind: 'device'; secretKey: string }

interface Payload {
  v: 2
  /** phrases by id (hash of the phrase, so the same phrase is never stored twice) */
  phrases: Record<string, string>
  /** per-account secrets for imported keys and passkey device keys */
  keys: Record<string, Secret>
}

interface Meta {
  accounts: RareAccount[]
  publicKeys: Record<string, string>
  selected: string | null
}

const K_META = 'rare.meta'
const EMPTY = (): Payload => ({ v: 2, phrases: {}, keys: {} })

/** Adding the first local key to a watch/Ledger-only wallet: a PIN must create the vault. */
export class NeedsPasswordError extends Error {
  constructor() {
    super('Set a PIN to add this wallet')
    this.name = 'NeedsPasswordError'
  }
}

export class LockedError extends Error {
  constructor() {
    super('Rare Wallet is locked')
    this.name = 'LockedError'
  }
}

/** Builds signers for account kinds whose keys live elsewhere (Ledger, Keystone, a passkey on a phone). */
export type SignerProvider = (account: RareAccount, publicKey: string) => Promise<AccountSigner>

export interface KeystoreOptions {
  /** public account data (addresses, names, public keys); never secrets */
  disk: KV
  /** the encrypted vault (Rust on the device, WebCrypto stand-in in a browser) */
  vault: VaultPort
  signerProviders?: Partial<Record<RareAccount['kind'], SignerProvider>>
}

/** Stable id for a phrase without storing it: first 8 bytes of blake2b(phrase), hex. */
export const phraseId = (mnemonic: string) => bytesToHex(blake2b(new TextEncoder().encode(normalizeMnemonic(mnemonic)), { dkLen: 8 }))

export interface PinOptions {
  secret: string
  kind: SecretKind
}

export class Keystore {
  private payload: Payload | null = null
  constructor(private o: KeystoreOptions) {}

  /** Lets hardware and passkey modules plug in their signers after construction. */
  setProvider(kind: RareAccount['kind'], p: SignerProvider) {
    this.o.signerProviders = { ...this.o.signerProviders, [kind]: p }
  }

  // ---- state
  async isInitialized() {
    return (await this.hasVault()) || (await this.meta()).accounts.length > 0
  }
  async hasVault() {
    return (await this.o.vault.status()).exists
  }
  /** No vault (watch/hardware only) means nothing to unlock. */
  async isUnlocked() {
    const st = await this.o.vault.status()
    if (!st.exists) return true
    return (await this.load()) !== null
  }

  // ---- lifecycle
  /**
   * The unlocked payload for adding a local key. A wallet without a vault creates one now,
   * which needs `pin`.
   */
  private async vaultFor(pin?: PinOptions): Promise<Payload> {
    if (await this.hasVault()) {
      const p = await this.load()
      if (!p) throw new LockedError()
      return p
    }
    if (!pin) throw new NeedsPasswordError()
    if (pin.kind === 'pin' && !/^\d{6,}$/.test(pin.secret)) throw new Error('Use at least 6 digits')
    if (pin.kind === 'passphrase' && pin.secret.length < 8) throw new Error('Use at least 8 characters')
    const p = EMPTY()
    await this.o.vault.create(pin.secret, pin.kind, JSON.stringify(p))
    this.payload = p
    return p
  }

  async unlock(secret: string): Promise<void> {
    this.payload = parse(await this.o.vault.unlock(secret))
  }

  async lock(): Promise<void> {
    this.payload = null
    await this.o.vault.lock()
  }

  async changeSecret(current: string, next: string, kind: SecretKind) {
    if (kind === 'pin' && !/^\d{6,}$/.test(next)) throw new Error('Use at least 6 digits')
    if (kind === 'passphrase' && next.length < 8) throw new Error('Use at least 8 characters')
    await this.o.vault.changeSecret(current, next, kind)
  }

  /** Removes every account and the vault from this device. */
  async wipe() {
    this.payload = null
    await this.o.vault.reset()
    await this.o.disk.remove(K_META)
  }

  // ---- accounts (public data: readable while locked)
  private async meta(): Promise<Meta> {
    return (await this.o.disk.get<Meta>(K_META)) ?? { accounts: [], publicKeys: {}, selected: null }
  }
  private async setMeta(m: Meta) {
    await this.o.disk.set(K_META, m)
  }
  async listAccounts() {
    return (await this.meta()).accounts
  }
  async current(): Promise<RareAccount | null> {
    const m = await this.meta()
    return m.accounts.find((a) => a.address === m.selected) ?? m.accounts[0] ?? null
  }
  async select(address: string) {
    const m = await this.meta()
    if (!m.accounts.some((a) => a.address === address)) throw new Error('Unknown account')
    await this.setMeta({ ...m, selected: address })
  }
  async publicKey(address: string): Promise<string | undefined> {
    return (await this.meta()).publicKeys[address]
  }

  private async addAccount(account: RareAccount, publicKey: string | null) {
    const m = await this.meta()
    if (m.accounts.some((a) => a.address === account.address)) throw new Error('Account already exists')
    await this.setMeta({
      accounts: [...m.accounts, account],
      publicKeys: publicKey ? { ...m.publicKeys, [account.address]: publicKey } : m.publicKeys,
      selected: m.selected ?? account.address,
    })
    return account
  }

  private defaultLabel(m: Meta, kind: RareAccount['kind']) {
    const n = m.accounts.filter((a) => a.kind === kind).length + 1
    const base = { native: 'Account', passkey: 'Passkey', watch: 'Watch', ledger: 'Ledger', keystone: 'Keystone', social: 'Account' }[kind]
    return `${base} ${n}`
  }

  // ---- phrase and key accounts (need unlock, or a PIN for the first one)
  /** First run or another phrase: account 0 of `words` (generated when omitted). */
  async addPhrase(words?: string, o: { label?: string; pin?: PinOptions; index?: number } = {}): Promise<RareAccount> {
    const mnemonic = normalizeMnemonic(words ?? newMnemonic())
    if (!isValidMnemonic(mnemonic)) throw new Error('Invalid recovery phrase')
    const index = o.index ?? 0
    const kp = keypairFromMnemonic(mnemonic, index)
    const address = kp.toSuiAddress()
    if ((await this.meta()).accounts.some((a) => a.address === address)) throw new Error('Account already exists')
    const p = await this.vaultFor(o.pin)
    const id = phraseId(mnemonic)
    p.phrases[id] = mnemonic
    await this.persist(p)
    const m = await this.meta()
    return this.addAccount({ address, label: o.label?.trim() || this.defaultLabel(m, 'native'), kind: 'native', hd: { index, phraseId: id } }, publicKeyB64(kp))
  }

  /** "Add account": the next unused index of an existing phrase (the first phrase when not given). */
  async deriveNext(o: { phraseId?: string; label?: string } = {}): Promise<RareAccount> {
    const p = await this.load()
    if (!p) throw new LockedError()
    const id = o.phraseId ?? Object.keys(p.phrases)[0]
    const mnemonic = id ? p.phrases[id] : undefined
    if (!mnemonic) throw new Error('No recovery phrase on this device. Create or import one first.')
    const used = new Set((await this.meta()).accounts.filter((a) => a.hd?.phraseId === id).map((a) => a.hd!.index))
    let index = 0
    while (used.has(index)) index++
    return this.addPhrase(mnemonic, { index, label: o.label })
  }

  /** Imported private key (`suiprivkey1…` of any scheme, or 32-byte hex Ed25519). */
  async addPrivateKey(key: string, o: { label?: string; pin?: PinOptions } = {}): Promise<RareAccount> {
    const kp = keypairFromSecret(key)
    const address = kp.toSuiAddress()
    if ((await this.meta()).accounts.some((a) => a.address === address)) throw new Error('Account already exists')
    const p = await this.vaultFor(o.pin)
    p.keys[address] = { kind: 'key', secretKey: kp.getSecretKey() }
    await this.persist(p)
    const m = await this.meta()
    return this.addAccount({ address, label: o.label?.trim() || this.defaultLabel(m, 'native'), kind: 'native' }, publicKeyB64(kp))
  }

  /** View-only account: the address and nothing else. Needs no unlock. */
  async addWatch(address: string, label?: string): Promise<RareAccount> {
    const raw = address.trim()
    if (!/^0x[0-9a-fA-F]{1,64}$/.test(raw) || !isValidSuiAddress(normalizeSuiAddress(raw))) throw new Error('Invalid Sui address')
    const m = await this.meta()
    return this.addAccount({ address: normalizeSuiAddress(raw), label: label?.trim() || this.defaultLabel(m, 'watch'), kind: 'watch' }, null)
  }

  /** Ledger account: the device's Ed25519 public key and path. The key never leaves the device. */
  async addLedger(publicKey: string, path: string, label?: string, device?: string): Promise<RareAccount> {
    let address: string
    try {
      address = addressFromPublicKey(publicKey)
    } catch {
      throw new Error('Invalid Ledger public key')
    }
    if (!isSuiLedgerPath(path)) throw new Error('Unsupported Ledger path')
    const m = await this.meta()
    return this.addAccount(
      { address, label: label?.trim() || this.defaultLabel(m, 'ledger'), kind: 'ledger', ledger: { path: path.trim(), ...(device ? { device: device.slice(0, 40) } : {}) } },
      publicKey,
    )
  }

  /** Keystone account (over USB): Ed25519 public key, path and master fingerprint. */
  async addKeystone(publicKey: string, path: string, mfp: string, label?: string, device?: string): Promise<RareAccount> {
    let address: string
    try {
      address = addressFromPublicKey(publicKey)
    } catch {
      throw new Error('Invalid Keystone public key')
    }
    if (!isSuiLedgerPath(path)) throw new Error('Unsupported path')
    if (!/^[0-9a-f]{8}$/i.test(mfp)) throw new Error('Invalid master fingerprint')
    const m = await this.meta()
    return this.addAccount(
      { address, label: label?.trim() || this.defaultLabel(m, 'keystone'), kind: 'keystone', keystone: { path: path.trim(), mfp: mfp.toLowerCase(), ...(device ? { device: device.slice(0, 40) } : {}) } },
      publicKey,
    )
  }

  /**
   * Passkey account: a 1-of-2 multisig of the passkey (secp256r1) and a device key (Ed25519).
   * `deviceSecret` is the device member's private key when this device holds it (created here,
   * or restored); without it the account signs with the passkey only (a phone tap each time).
   */
  async addPasskey(
    o: { credentialId: string; publicKey: string; deviceKey: string; deviceSecret?: string; label?: string; authenticator?: string; pin?: PinOptions },
  ): Promise<RareAccount> {
    const ms = passkeyMultisig(fromBase64(o.publicKey), fromBase64(o.deviceKey))
    const address = ms.toSuiAddress()
    if ((await this.meta()).accounts.some((a) => a.address === address)) throw new Error('Account already exists')
    if (o.deviceSecret) {
      const kp = Ed25519Keypair.fromSecretKey(o.deviceSecret)
      if (toBase64(kp.getPublicKey().toRawBytes()) !== o.deviceKey) throw new Error('Device key does not match')
      const p = await this.vaultFor(o.pin)
      p.keys[address] = { kind: 'device', secretKey: kp.getSecretKey() }
      await this.persist(p)
    }
    const m = await this.meta()
    return this.addAccount(
      {
        address,
        label: o.label?.trim() || this.defaultLabel(m, 'passkey'),
        kind: 'passkey',
        passkey: { credentialId: o.credentialId, publicKey: o.publicKey, deviceKey: o.deviceKey, ...(o.authenticator ? { authenticator: o.authenticator } : {}) },
      },
      toBase64(ms.toRawBytes()),
    )
  }

  /** Whether this device holds the passkey account's device key (quick signing with the PIN). */
  async hasDeviceKey(address: string): Promise<boolean> {
    const p = await this.load()
    return p?.keys[address]?.kind === 'device'
  }

  async rename(address: string, label: string): Promise<RareAccount> {
    const name = label.trim()
    if (!name) throw new Error('Name cannot be empty')
    const m = await this.meta()
    const acc = m.accounts.find((a) => a.address === address)
    if (!acc) throw new Error('Unknown account')
    const next = { ...acc, label: name.slice(0, 40) }
    await this.setMeta({ ...m, accounts: m.accounts.map((a) => (a === acc ? next : a)) })
    return next
  }

  /** Removes one account. Its secret goes too, unless other accounts share the phrase. */
  async remove(address: string): Promise<void> {
    const m = await this.meta()
    const acc = m.accounts.find((a) => a.address === address)
    if (!acc) return
    const rest = m.accounts.filter((a) => a.address !== address)
    if (acc.kind === 'native' || acc.kind === 'passkey') {
      const p = await this.load()
      if (!p) throw new LockedError()
      delete p.keys[address]
      if (acc.hd && !rest.some((a) => a.hd?.phraseId === acc.hd!.phraseId)) delete p.phrases[acc.hd.phraseId]
      await this.persist(p)
    }
    const { [address]: _, ...publicKeys } = m.publicKeys
    void _
    await this.setMeta({ accounts: rest, publicKeys, selected: m.selected === address ? (rest[0]?.address ?? null) : m.selected })
  }

  // ---- signing / secrets
  /** Whether signing with this account needs the vault unlocked first. */
  async needsUnlock(address: string): Promise<boolean> {
    const acc = (await this.meta()).accounts.find((a) => a.address === address)
    if (!acc) return false
    if (acc.kind === 'native') return !(await this.load())
    if (acc.kind === 'passkey') return (await this.hasVault()) && !(await this.load())
    return false
  }

  async getSigner(address: string): Promise<AccountSigner> {
    const m = await this.meta()
    const account = m.accounts.find((a) => a.address === address)
    if (!account) throw new Error('Unknown account')
    if (account.kind === 'watch') throw new WatchOnlyError()
    const pk = m.publicKeys[address] ?? ''
    if (account.kind === 'passkey') {
      const p = await this.load()
      const dev = p?.keys[address]
      if (dev?.kind === 'device') return new MultisigDeviceSigner(account, Ed25519Keypair.fromSecretKey(dev.secretKey))
      // no device key here (or locked): sign with the passkey itself
      const prov = this.o.signerProviders?.passkey
      if (!prov) throw (await this.hasVault()) && !p ? new LockedError() : new UnsupportedAccountKind('passkey')
      return prov(account, pk)
    }
    if (account.kind !== 'native') {
      const prov = this.o.signerProviders?.[account.kind]
      if (!prov) throw new UnsupportedAccountKind(account.kind)
      return prov(account, pk)
    }
    const p = await this.load()
    if (!p) throw new LockedError()
    return new NativeSigner(this.keypair(p, account))
  }

  private keypair(p: Payload, account: RareAccount): Keypair {
    if (account.hd) {
      const words = p.phrases[account.hd.phraseId]
      if (!words) throw new Error('Missing key material')
      return keypairFromMnemonic(words, account.hd.index)
    }
    const s = p.keys[account.address]
    if (!s || s.kind !== 'key') throw new Error('Missing key material')
    return keypairFromSecret(s.secretKey)
  }

  /** Asks for the PIN again; the only path that returns a phrase. */
  async revealMnemonic(secret: string, address: string): Promise<string> {
    const p = parse(await this.o.vault.verify(secret))
    const acc = (await this.meta()).accounts.find((a) => a.address === address)
    const words = acc?.hd ? p.phrases[acc.hd.phraseId] : undefined
    if (!words) throw new Error('No recovery phrase for this account')
    return words
  }

  /** Asks for the PIN again; the account's private key as `suiprivkey1…`. */
  async revealPrivateKey(secret: string, address: string): Promise<string> {
    const p = parse(await this.o.vault.verify(secret))
    const acc = (await this.meta()).accounts.find((a) => a.address === address)
    if (!acc || acc.kind !== 'native') throw new Error('No private key for this account')
    return this.keypair(p, acc).getSecretKey()
  }

  /**
   * 32 secret bytes for `info`, deterministic per account: HKDF-SHA256 over the account's private
   * key. The key itself never leaves the keystore. Phrase and imported-key accounts only.
   */
  async deriveFromKey(address: string, salt: Uint8Array, info: Uint8Array): Promise<Uint8Array> {
    const p = await this.load()
    if (!p) throw new LockedError()
    const acc = (await this.meta()).accounts.find((a) => a.address === address)
    if (!acc || acc.kind !== 'native') throw new Error('No local key for this account')
    const { secretKey } = decodeSuiPrivateKey(this.keypair(p, acc).getSecretKey())
    return hkdf(sha256, secretKey, salt, info, 32)
  }

  /** Whether this account came from a recovery phrase (public metadata, works while locked). */
  async hasPhrase(address: string): Promise<boolean> {
    return !!(await this.meta()).accounts.find((a) => a.address === address)?.hd
  }

  // ---- internals
  private async load(): Promise<Payload | null> {
    if (this.payload) return this.payload
    // the UI reloaded while Rust kept the vault unlocked
    const raw = await this.o.vault.read().catch(() => null)
    if (!raw) return null
    this.payload = parse(raw)
    return this.payload
  }

  private async persist(p: Payload) {
    this.payload = p
    await this.o.vault.save(JSON.stringify(p))
  }
}

function parse(raw: string): Payload {
  const v = JSON.parse(raw) as Partial<Payload>
  return { v: 2, phrases: v.phrases ?? {}, keys: v.keys ?? {} }
}

/** Ed25519 public key (base64) -> address, for checks. */
export const ed25519Address = (pk: string) => new Ed25519PublicKey(fromBase64(pk)).toSuiAddress()
