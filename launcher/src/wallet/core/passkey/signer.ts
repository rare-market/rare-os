import { PasskeyKeypair, type PasskeyProvider } from '@mysten/sui/keypairs/passkey'
import { fromBase64, toBase64 } from '@mysten/sui/utils'
import type { AccountSigner } from '../keys/signer'
import type { AssertionRequest, AssertionResult } from './webauthn'

/** Performs a WebAuthn assertion somewhere that can (the popup). Injected into the signer. */
export type AssertFn = (req: AssertionRequest) => Promise<AssertionResult>

/** Adapts an AssertFn to the provider interface the Sui SDK's PasskeyKeypair signs through. */
function providerFrom(assert: AssertFn): PasskeyProvider {
  return {
    async create() {
      throw new Error('Passkey creation happens in the popup (createPasskey), not in the signer')
    },
    async get(challenge, credentialId) {
      if (!credentialId) throw new Error('Missing passkey credential id')
      const r = await assert({ credentialId: toBase64(credentialId), challenge: toBase64(challenge) })
      return {
        response: {
          authenticatorData: fromBase64(r.authenticatorData),
          clientDataJSON: fromBase64(r.clientDataJSON),
          signature: fromBase64(r.signature),
        },
      } as any
    },
  }
}

/**
 * Signs for a passkey account. No private key exists: each signature triggers one biometric
 * assertion over blake2b(intent || tx), assembled by the SDK into the Sui passkey signature
 * (flag 0x06 || BCS PasskeyAuthenticator{authenticatorData, clientDataJson, secp256r1 sig+pk}).
 */
export class PasskeySigner implements AccountSigner {
  private kp: PasskeyKeypair
  constructor(publicKeyB64: string, credentialIdB64: string, assert: AssertFn) {
    this.kp = new PasskeyKeypair(fromBase64(publicKeyB64), providerFrom(assert), fromBase64(credentialIdB64))
  }
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
