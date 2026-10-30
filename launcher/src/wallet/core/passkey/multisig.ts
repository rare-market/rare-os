// A Rare passkey account is a 1-of-2 Sui multisig:
//   member 0: the person's passkey (secp256r1, Sui's passkey scheme, flag 0x06), weight 1
//   member 1: an Ed25519 key kept on the device, sealed in the PIN vault, weight 1
// threshold 1. Either member alone can sign. Everyday signing (Buy, games) uses the device key
// behind the PIN; the passkey signs on a device that does not hold the key (phone tap), and keeps
// the account recoverable if the handheld is lost. Sui accepts passkey members in multisig on
// mainnet since protocol version 90 (accept_passkey_in_multisig).
//
// The member order and weights are part of the address, so they never change.

import type { Keypair } from '@mysten/sui/cryptography'
import { Ed25519PublicKey } from '@mysten/sui/keypairs/ed25519'
import { PasskeyPublicKey } from '@mysten/sui/keypairs/passkey'
import { MultiSigPublicKey } from '@mysten/sui/multisig'
import { fromBase64, toBase64 } from '@mysten/sui/utils'
import type { RareAccount } from '../types'
import type { AccountSigner } from '../keys/signer'

export function passkeyMultisig(passkeyPublicKey: Uint8Array, devicePublicKey: Uint8Array): MultiSigPublicKey {
  if (passkeyPublicKey.length !== 33) throw new Error('Expected a compressed secp256r1 passkey key (33 bytes)')
  if (devicePublicKey.length !== 32) throw new Error('Expected an Ed25519 device key (32 bytes)')
  return MultiSigPublicKey.fromPublicKeys({
    threshold: 1,
    publicKeys: [
      { publicKey: new PasskeyPublicKey(passkeyPublicKey), weight: 1 },
      { publicKey: new Ed25519PublicKey(devicePublicKey), weight: 1 },
    ],
  })
}

export const accountMultisig = (a: RareAccount) => {
  if (!a.passkey) throw new Error('Not a passkey account')
  return passkeyMultisig(fromBase64(a.passkey.publicKey), fromBase64(a.passkey.deviceKey))
}

/** Wraps any member signer's serialized signature into the account's multisig signature. */
export class MultisigMemberSigner implements AccountSigner {
  private ms: MultiSigPublicKey
  constructor(
    account: RareAccount,
    private member: { signTransaction(b: Uint8Array): Promise<string>; signPersonalMessage(b: Uint8Array): Promise<string> },
  ) {
    this.ms = accountMultisig(account)
  }
  get address() {
    return this.ms.toSuiAddress()
  }
  async signTransaction(txBytes: Uint8Array) {
    return this.ms.combinePartialSignatures([await this.member.signTransaction(txBytes)])
  }
  async signPersonalMessage(message: Uint8Array) {
    return { bytes: toBase64(message), signature: this.ms.combinePartialSignatures([await this.member.signPersonalMessage(message)]) }
  }
}

/** Signs with the device member (no phone needed). */
export class MultisigDeviceSigner extends MultisigMemberSigner {
  constructor(account: RareAccount, device: Keypair) {
    super(account, {
      signTransaction: async (b) => (await device.signTransaction(b)).signature,
      signPersonalMessage: async (b) => (await device.signPersonalMessage(b)).signature,
    })
  }
}
