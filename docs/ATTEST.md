# Device attestation (rare-attest)

Proves a device is a genuine SuiPlay (and which Rare OS boot chain it runs) to rare.market's device
attestor, using the TPM. It is used for the Rare Device Pass and for any "verified on SuiPlay"
feature, such as trophies. Owner of this API: the rare-attest crate (`launcher/crates/rare-attest`)
and the attestor (`market` repo, `attest/`).

## What gets proven

| step | proves |
|---|---|
| EK certificate chains to AMD's pinned fTPM root (`CN=AMDTPM`) | the TPM is a real AMD fTPM, not software |
| ActivateCredential with the server's MakeCredential blob | the attestation key (AK) lives in that same TPM |
| Quote of PCR 0, 4, 7 signed by the AK over the server's nonce | the boot measurements are fresh and come from that TPM |
| PCR0 in the firmware allowlist (`attest/policy/firmware.json`) | the firmware is a SuiPlay BIOS build (2.19_S20 today) |
| PCR4 and PCR7 in the signed boot values (`attest/policy/boot.json`) | the boot went through Rare OS's shim and grub, and records the Secure Boot state |

Levels in every answer, weakest first:

- `firmware`: SuiPlay firmware, boot chain not recognised (another OS, or a Rare build whose values aren't signed yet).
- `rare-os`: SuiPlay firmware and a Rare OS boot chain, with Secure Boot off. All installs are here today.
- `rare-os-secure`: the same with Secure Boot on.

The attestor refuses a device whose PCR0 isn't SuiPlay firmware (`unknown_firmware`). The Device Pass
needs `rare-os` (attestor env `ATTEST_PASS_LEVEL`). Each consumer decides what level it needs.

Limits:
- An AYANEO 2S flashed with the SuiPlay BIOS would likely pass as a SuiPlay, because the hardware is the same.
- With Secure Boot off, the kernel and userspace aren't verified by the boot chain. `rare-os` means "booted Rare's boot chain", not "unmodified OS".

## Rust API (launcher/crates/rare-attest)

```rust
rare_attest::status() -> TpmStatus            // Ready | NoTpm | NoAccess | NoTools (cheap, no TPM commands)
rare_attest::attest(&Endpoint, &Request) -> Result<Attestation, AttestError>   // blocking, ~2-4 s on the SuiPlay

Endpoint { base: "https://rare.market".into(), testnet: false }   // testnet adds ?network=testnet
Request  { address: "0x…".into(), aud: None }                    // aud: device-id audience, default "rare"

Attestation {
    token: String,          // "rda1.<payload>.<sig>", valid 10 minutes
    device: String,         // hex, Rare-salted device id (audience "rare"; the Device Pass registry key)
    model: String,          // "SuiPlay"
    firmware: String,       // "SuiPlay0X1 BIOS 2.19_S20 (2025-02-19)"
    level: String,          // "firmware" | "rare-os" | "rare-os-secure"
    secure_boot: bool,
    first_seen_ms: u64,     // first time Rare saw this device
    expires_ms: u64,
    pass: PassOffer,        // New(Voucher) | Rebind(Voucher) | Held{..} | Unavailable{reason}
}
AttestError::message() -> String   // one plain sentence for the UI
```

Run it off the UI thread. It needs the user in group `tss`: the image's user-setup adds the uid 1000
user, and `status()` says `NoAccess` until then. It also needs tpm2-tools (in the image). Nothing is
persisted: the EK and AK are transient, and the work folder under $XDG_RUNTIME_DIR is removed when
the call returns.

CLI for scripts and ssh checks: `rare-attest --address 0x… [--testnet] [--api URL] [--aud NAME]`
prints the Attestation as JSON. `rare-attest --status` prints the TPM status.

## Claim token (for "verified on SuiPlay")

`rda1.` + base64url(payload JSON) + `.` + base64url(Ed25519 signature over `"RARE_DEVICE_CLAIM_V1\n" || payload JSON bytes`),
signed by the attestor key (`GET key`; testnet key today `5720fe8f0ea1067825e5bc217aedbce62f938749ede44f797114c09e6cc0fd2f`).

```json
{ "v": 1, "aud": "rare", "sub": "0x<rare account>", "dev": "<hex device id for aud>", "model": "SuiPlay",
  "fw": "suiplay-bios-2.19_S20", "level": "rare-os", "sb": false, "iat": 1793203200000, "exp": 1793203800000 }
```

To use it in another Rare service (the trophies attestor, for example):
1. The launcher calls `rare_attest::attest` with the player's address and sends `token` along with its own request.
2. The service checks the signature against the attestor key (or POSTs `verify`), `exp` > now,
   `sub` == the player, and `level` is at least what it requires. Store `dev` only if you need
   one-per-device rules. With your own `aud` (for example `"trophies"`), `dev` can't be linked to
   the Device Pass id.
3. Device ids are `HMAC-SHA256(salt, "RARE_DEVICE_ID_V1" || aud || 0x00 || EK modulus)`. The salt
   stays on the attestor, and no service ever sees the EK.

## Device Pass (Move, testnet)

`market/contracts/device`, package `rare_device`. Testnet: package
`0x23f19363ba81df3dcdb169aa21ded744c03574778ea08e4d8b9b9fb85f91a873`, registry
`0x068f243f99d9c3c808a0daa12614febbcb177559c2a6f3e8d7a804ca18f4bf26`. It isn't on mainnet yet.
`device_pass::claim(registry, device, model, first_seen_ms, seq, expires_ms, signature, clock)`
verifies the attestor's Ed25519 signature over
`"RARE_DEVICE_V1" || registry || sender || device || seq u64le || first_seen_ms u64le || expires_ms u64le || model`
and mints a soulbound `DevicePass` to the sender. There is one per device: a second first claim aborts.
A rebind (`seq` = record seq + 1, signed by the attestor) moves the device's pass to a new owner,
keeping its number and first-seen date. Holders are listed in the registry's `devices` table and the
`PassClaimed` events.
