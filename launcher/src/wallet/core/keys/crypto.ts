/** Password-based encryption at rest: PBKDF2-SHA256 -> AES-256-GCM (WebCrypto, works in SW and Node). */

export const DEFAULT_ITERATIONS = 600_000

export interface VaultBlob {
  v: 1
  iter: number
  salt: string
  iv: string
  ct: string
}

export class WrongPasswordError extends Error {
  constructor() {
    super('Wrong password')
    this.name = 'WrongPasswordError'
  }
}

const subtle = () => globalThis.crypto.subtle
const rand = (n: number): Uint8Array<ArrayBuffer> => globalThis.crypto.getRandomValues(new Uint8Array(n))

export const b64 = (u: Uint8Array): string => {
  let s = ''
  for (const b of u) s += String.fromCharCode(b)
  return btoa(s)
}
export const unb64 = (s: string): Uint8Array<ArrayBuffer> => Uint8Array.from(atob(s), (c) => c.charCodeAt(0))

export async function deriveKey(password: string, salt: Uint8Array<ArrayBuffer>, iter: number): Promise<CryptoKey> {
  const base = await subtle().importKey('raw', new TextEncoder().encode(password), 'PBKDF2', false, ['deriveKey'])
  return subtle().deriveKey(
    { name: 'PBKDF2', hash: 'SHA-256', salt, iterations: iter },
    base,
    { name: 'AES-GCM', length: 256 },
    true,
    ['encrypt', 'decrypt'],
  )
}

export const exportKey = async (k: CryptoKey): Promise<string> => b64(new Uint8Array(await subtle().exportKey('raw', k)))
export const importKey = (raw: string): Promise<CryptoKey> =>
  subtle().importKey('raw', unb64(raw), 'AES-GCM', true, ['encrypt', 'decrypt'])

export async function seal(key: CryptoKey, plain: unknown, meta: { iter: number; salt: string }): Promise<VaultBlob> {
  const iv = rand(12)
  const ct = await subtle().encrypt({ name: 'AES-GCM', iv }, key, new TextEncoder().encode(JSON.stringify(plain)))
  return { v: 1, iter: meta.iter, salt: meta.salt, iv: b64(iv), ct: b64(new Uint8Array(ct)) }
}

export async function open<T>(key: CryptoKey, blob: VaultBlob): Promise<T> {
  try {
    const pt = await subtle().decrypt({ name: 'AES-GCM', iv: unb64(blob.iv) }, key, unb64(blob.ct))
    return JSON.parse(new TextDecoder().decode(pt)) as T
  } catch {
    throw new WrongPasswordError()
  }
}

export async function sealWithPassword(password: string, plain: unknown, iter = DEFAULT_ITERATIONS) {
  const salt = rand(16)
  const key = await deriveKey(password, salt, iter)
  return { key, blob: await seal(key, plain, { iter, salt: b64(salt) }) }
}

export async function openWithPassword<T>(password: string, blob: VaultBlob) {
  const key = await deriveKey(password, unb64(blob.salt), blob.iter)
  return { key, value: await open<T>(key, blob) }
}
