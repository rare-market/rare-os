import type { Keypair } from '@mysten/sui/cryptography'
import { toBase64 } from '@mysten/sui/utils'

/** Anything that can sign for an account. Native keypairs now; passkey/social slot in via SignerProvider. */
export interface AccountSigner {
  address: string
  /** returns base64 serialized Sui signature over tx bytes */
  signTransaction(txBytes: Uint8Array): Promise<string>
  signPersonalMessage(message: Uint8Array): Promise<{ bytes: string; signature: string }>
}

export class NativeSigner implements AccountSigner {
  constructor(private kp: Keypair) {}
  get address() {
    return this.kp.toSuiAddress()
  }
  async signTransaction(txBytes: Uint8Array) {
    return (await this.kp.signTransaction(txBytes)).signature
  }
  async signPersonalMessage(message: Uint8Array) {
    const r = await this.kp.signPersonalMessage(message)
    return { bytes: toBase64(message), signature: r.signature }
  }
}

export class UnsupportedAccountKind extends Error {
  constructor(kind: string) {
    super(`Account kind "${kind}" is not available yet`)
    this.name = 'UnsupportedAccountKind'
  }
}

/** Watch-only accounts hold no key and can never produce a signature. */
export class WatchOnlyError extends Error {
  constructor() {
    super('Watch-only account cannot sign. Import it to sign.')
    this.name = 'WatchOnlyError'
  }
}
