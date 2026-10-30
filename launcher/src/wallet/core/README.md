# Rare Wallet core

Key handling for Rare Wallet: keys, passkeys, Ledger and the key-value store (`LocalKV`).

- `keys/` vault (PBKDF2 600k + AES-GCM), mnemonic/private-key derivation, signers, watch accounts
- `passkey/` passkey signer (WebAuthn)
- `ledger/` Ledger signer bridge and address paths

Phase 1 of the launcher uses watch accounts only; create/import/sign are wired in phase 2.
