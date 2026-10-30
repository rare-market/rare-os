# Passkeys in Rare Wallet

Sign-in for Rare is a passkey. No Google, Apple, zkLogin or Enoki.

## The account

A passkey account is a 1-of-2 Sui multisig:

| member | key | where it lives | weight |
| --- | --- | --- | --- |
| 0 | the person's passkey, secp256r1 (Sui passkey scheme, flag 0x06) | phone (iCloud Keychain, Google Password Manager) or a USB security key | 1 |
| 1 | an Ed25519 device key | the SuiPlay, sealed in the PIN vault | 1 |

Threshold 1, so either key alone can sign. The members, their order and weights make the
address, so they never change. Sui accepts passkey members in multisig on mainnet since
protocol version 90.

Code: `launcher/src/wallet/core/passkey/multisig.ts` (layout), `launcher/src/wallet/passkey/`
(create, sign in, signer), `launcher/src-tauri/src/wallet/passkey/` (the WebAuthn ceremony).

## Creating an account

1. The SuiPlay makes a new Ed25519 device key.
2. It registers a passkey for relying party `rare.market` with user handle
   `0x01 || device public key` (33 bytes). ES256 only, discoverable (resident) credential.
3. The passkey's public key comes back; the address is `multisig(passkey, device key)`.
4. The device key goes into the PIN vault.

## Signing in on another SuiPlay

The passkey is asked for any Rare credential (discoverable sign-in). Its answer carries:

- the user handle, which holds the device public key, and
- a P-256 signature, from which the passkey public key is recovered. Recovery gives about two
  candidate keys, so two candidate addresses.

The right address is picked without bothering the person when possible: an account already on
this device, then the candidate whose device key is still in this vault, then the only candidate
with anything on chain (gRPC balances and owned objects). Only if none of that decides does the
passkey sign once more, and the key common to both signatures wins.

A SuiPlay that does not hold the device key signs with the passkey itself.

## What needs the phone (or security key)

- Creating the account.
- Signing in on a new SuiPlay, or after the wallet was removed.
- Every signature on a SuiPlay without the device key (one scan or tap per signature).

Everyday signing on the SuiPlay that created the account uses the device key behind the PIN,
no phone.

Adding the device key of a second SuiPlay would change the address, so a second SuiPlay signs
with the passkey.

## How the ceremony runs on the SuiPlay

The webview (WebKitGTK) has no passkey support on Linux, so the launcher is the WebAuthn client,
in Rust, with `webauthn-authenticator-rs` 0.5.5 (features `cable`, `usb`). A patched copy lives in
`launcher/src-tauri/vendor/webauthn-authenticator-rs` (search "RARE PATCH"): upstream drops the
user handle from assertions, never sends `rk` for resident keys, and fails when a key leaves out
the credential for a one-entry allow list.

Phone (hybrid, also called caBLE):

1. The SuiPlay shows a `FIDO:/` QR code. The phone scans it with its camera.
2. The phone sends a Bluetooth LE advert. The SuiPlay only scans: no pairing, no advertising.
3. Both connect to a relay over the internet (websocket, TLS, plus Noise encryption end to end):
   `cable.ua5v.com` (Google, Android phones) or `cable.auth.com` (Apple, iPhones). The phone
   picks the relay.
4. The phone shows its own Face ID, fingerprint or screen lock prompt.

On the SuiPlay this needs Bluetooth on (BlueZ), D-Bus access to BlueZ (the logged-in desktop
user has it on Bazzite), and outbound HTTPS to the relay. The phone needs Bluetooth and
internet too.

USB security key: CTAP2 over USB HID. The key must have a PIN or built-in fingerprint; Rare asks
for the key's PIN when the key wants it. systemd 252 and later give the logged-in user access to
FIDO keys on its own; if not, the wallet shows a one-line udev fix.

## Secrets from the passkey (PRF)

The passkey can also give a deterministic 32-byte secret per input (WebAuthn PRF), used for
cloud-save keys. Rare's input is SHA-256("rare/" + context). The authenticator HMACs the salt
SHA-256("WebAuthn PRF" || 0x00 || input), exactly as browsers do, so rare.market can later
derive the same secret with `navigator.credentials.get({ extensions: { prf: { eval: { first: input } } } })`.

On the wire it follows Chromium (`device/fido/prf_input.cc`, `get_assertion_request_handler.cc`,
`make_credential_request_handler.cc`, `device_response_converter.cc`):

- Phones (hybrid) advertise "prf": creation sends `{"prf": {}}`, sign-in sends
  `{"prf": {"eval": {"first": salt}}}`, and the output comes back in the unsigned extension
  outputs.
- USB security keys advertise "hmac-secret": creation sends `{"hmac-secret": true}`, sign-in
  sends the salt encrypted under a fresh key agreement (PIN/UV protocol 2, else 1), and the key
  returns the encrypted output in authenticatorData.

Every account made from now on asks for PRF at creation. Passkeys that cannot do it return
error "no-prf".

