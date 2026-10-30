# Keystone 3 Pro in Rare Wallet

## How it works

- Accounts: Rare Wallet sends a `qr-hardware-call` KeyDerivation (version 1, ed25519, SLIP-10,
  chain "SUI") for paths `m/44'/784'/i'/0'/0'`. The Keystone asks the person to approve and enter
  its password, then returns `crypto-multi-accounts`: the master fingerprint and one 32-byte
  public key per path. Up to 24 accounts per request.
- Signing: Rare Wallet computes `blake2b-256(intent || data)`, exactly the 32 bytes a Sui Ed25519
  key signs (transaction: intent `00 00 00` and the transaction bytes; personal message: intent
  `03 00 00` and the BCS bytes of the message). It sends that hash with the path, master
  fingerprint and address. The Keystone shows the hash, address and path, and returns a
  `sui-signature` (64-byte signature and the public key). Rare Wallet checks the request id, the
  key, and the signature before it uses it.

## Requirements

- Keystone 3 Pro with the multi-coin firmware.
- USB turned on: on the Keystone, Settings > Connection.
- The Keystone unlocked and on its home screen when adding accounts (the firmware refuses key
  export from other pages; Rare Wallet asks the person to go home and retries).
- Linux: a udev rule so the app can open the device without sudo (USB vendor `1209`, product
  `3001`). When it is missing, Rare Wallet shows the one-line command to add it.

