import { Ed25519Keypair } from '@mysten/sui/keypairs/ed25519'
import { generateMnemonic, validateMnemonic } from '@scure/bip39'
import { wordlist } from '@scure/bip39/wordlists/english.js'
import { Secp256k1Keypair } from '@mysten/sui/keypairs/secp256k1'
import { Secp256r1Keypair } from '@mysten/sui/keypairs/secp256r1'
import { decodeSuiPrivateKey, type Keypair } from '@mysten/sui/cryptography'
import { fromHex, toBase64 } from '@mysten/sui/utils'

/** 12 words (128 bits) by default, 24 words with 256. */
export const newMnemonic = (strength: 128 | 256 = 128): string => generateMnemonic(wordlist, strength)

export const WORDLIST: readonly string[] = wordlist

export const normalizeMnemonic = (words: string): string => words.trim().toLowerCase().split(/\s+/).join(' ')

export const isValidMnemonic = (words: string): boolean => validateMnemonic(normalizeMnemonic(words), wordlist)

/** Sui standard path m/44'/784'/i'/0'/0' (Slush, Nightly and Sui CLI agree). Same phrase and index -> same address. */
export const suiPath = (index = 0) => `m/44'/784'/${index}'/0'/0'`

export function keypairFromMnemonic(words: string, index = 0): Ed25519Keypair {
  const m = normalizeMnemonic(words)
  if (!validateMnemonic(m, wordlist)) throw new Error('Invalid recovery phrase')
  if (!Number.isInteger(index) || index < 0 || index >= 0x80000000) throw new Error('Invalid account index')
  return Ed25519Keypair.deriveKeypair(m, suiPath(index))
}

export const publicKeyB64 = (kp: Keypair): string => toBase64(kp.getPublicKey().toRawBytes())

/**
 * A private key as Sui wallets export it: bech32 `suiprivkey1…` (Ed25519, Secp256k1 or
 * Secp256r1), or a bare 32-byte hex Ed25519 secret (older Sui Wallet exports).
 */
export function keypairFromSecret(key: string): Keypair {
  const k = key.trim()
  try {
    if (k.startsWith('suiprivkey')) {
      const { scheme, secretKey } = decodeSuiPrivateKey(k)
      if (scheme === 'ED25519') return Ed25519Keypair.fromSecretKey(secretKey)
      if (scheme === 'Secp256k1') return Secp256k1Keypair.fromSecretKey(secretKey)
      if (scheme === 'Secp256r1') return Secp256r1Keypair.fromSecretKey(secretKey)
    } else {
      const hex = k.replace(/^0x/i, '')
      if (/^[0-9a-fA-F]{64}$/.test(hex)) return Ed25519Keypair.fromSecretKey(fromHex(hex))
    }
  } catch {
    /* fall through */
  }
  throw new Error('Invalid private key')
}
