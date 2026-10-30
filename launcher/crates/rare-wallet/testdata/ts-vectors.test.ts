// Cross-check vectors between the TypeScript wallet (launcher/src/wallet) and the Rust
// rare-wallet crate. This file runs the TS wallet's own code over fixed inputs and compares the
// result with ts-vectors.json; the Rust tests (src/keys/vectors.rs) read the same JSON and must
// produce the same bytes. So a change on either side that breaks byte parity fails a test.
//
//   npm test -- crates/rare-wallet                      check (part of the launcher's vitest run)
//   RARE_WRITE_VECTORS=1 npm test -- crates/rare-wallet   rewrite ts-vectors.json

import { describe, expect, it } from "vitest";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { bcs } from "@mysten/sui/bcs";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { Secp256k1Keypair } from "@mysten/sui/keypairs/secp256k1";
import { Secp256r1Keypair } from "@mysten/sui/keypairs/secp256r1";
import { PasskeyKeypair, type PasskeyProvider } from "@mysten/sui/keypairs/passkey";
import { toBase64, fromBase64, toHex } from "@mysten/sui/utils";
import { p256 } from "@noble/curves/nist.js";
import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex } from "@noble/hashes/utils.js";
import { Keystore, phraseId } from "../../../src/wallet/core/keys/keystore";
import { keypairFromMnemonic, keypairFromSecret } from "../../../src/wallet/core/keys/derive";
import { MemoryKV } from "../../../src/wallet/core/store/kv";
import { memoryVault } from "../../../src/wallet/testing/memoryVault";
import { passkeyMultisig, MultisigMemberSigner, MultisigDeviceSigner } from "../../../src/wallet/core/passkey/multisig";
import { DERIVE_SALT, deriveInfo, deriveMessage, ed25519SignatureBytes, finish, prfInput } from "../../../src/wallet/secret";
import type { RareAccount } from "../../../src/wallet/core/types";
import { kindOfBytes, needsSponsor, gameCategory } from "../../../src/wallet/sponsor";
import Sui from "@mysten/ledgerjs-hw-app-sui";
import { FakeLedgerTransport } from "../../../src/wallet/ledger/_testkit";
import { clearSigningObjectIds, ledgerSignPersonalMessage, ledgerSignTransaction, readLedgerAccount, type SuiLedgerApp } from "../../../src/wallet/ledger/sign";
import { checkSuiApp } from "../../../src/wallet/ledger/device";
import { keyDerivationUR, signHashUR, encodeUR } from "../../../src/wallet/keystone/ur";
import { mockKeystoneSend, mockKeystoneDevice } from "../../../src/wallet/keystone/mock";
import { suiSigningDigest } from "../../../src/wallet/keystone/prompt";
import { SuiSignature } from "@keystonehq/bc-ur-registry-sui";
import { recoverPasskeyKeys, userHandleFor } from "../../../src/wallet/passkey/index";
import { Transaction, coinWithBalance } from "@mysten/sui/transactions";
import { mapGameRequest } from "../../../src/wallet/ui/sign/webgame";

const FILE = fileURLToPath(new URL("./ts-vectors.json", import.meta.url));
const enc = new TextEncoder();

const PHRASE = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const PHRASE24 =
  "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title";
const ADDR = (n: number) => "0x" + n.toString(16).padStart(2, "0").repeat(32);

/** A complete TransactionData (V1, programmable) with no network needed. */
function txBytes(o: { sender: string; gasOwner: string; gasCoin?: boolean }): Uint8Array {
  const ref = { objectId: ADDR(0x11), version: "7", digest: "4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi" };
  const commands = o.gasCoin
    ? [{ SplitCoins: { coin: { GasCoin: true }, amounts: [{ Input: 0 }] } }, { TransferObjects: { objects: [{ NestedResult: [0, 0] }], address: { Input: 1 } } }]
    : [
        {
          MoveCall: {
            package: ADDR(0x22),
            module: "game",
            function: "play",
            typeArguments: [],
            arguments: [{ Input: 0 }, { Input: 1 }],
          },
        },
      ];
  return bcs.TransactionData.serialize({
    V1: {
      kind: {
        ProgrammableTransaction: {
          inputs: [{ Pure: { bytes: toBase64(bcs.u64().serialize(1_000_000n).toBytes()) } }, { Pure: { bytes: toBase64(bcs.Address.serialize(o.sender).toBytes()) } }],
          commands,
        },
      },
      sender: o.sender,
      gasData: { payment: [ref], owner: o.gasOwner, price: "1000", budget: "5000000" },
      expiration: { None: true },
    },
  } as never).toBytes();
}

/** A WebAuthn assertion over `challenge` from a fixed P-256 key, as a phone would return it. */
function assertion(secret: Uint8Array, challenge: Uint8Array, highS: boolean) {
  const authenticatorData = new Uint8Array([...sha256(enc.encode("rare.market")), 0x05, 0, 0, 0, 0]);
  const b64url = toBase64(challenge).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  const clientDataJSON = enc.encode(`{"type":"webauthn.get","challenge":"${b64url}","origin":"https://rare.market","crossOrigin":false}`);
  const msg = new Uint8Array([...authenticatorData, ...sha256(clientDataJSON)]);
  let sig = p256.Signature.fromBytes(p256.sign(msg, secret, { lowS: true }), "compact");
  if (highS) sig = new p256.Signature(sig.r, p256.Point.Fn.neg(sig.s));
  return { authenticatorData, clientDataJSON, signature: sig.toBytes("der") };
}

async function generate() {
  const out: Record<string, unknown> = {};

  // ---- phrases: derivation path m/44'/784'/i'/0'/0'
  out.phrases = [PHRASE, PHRASE24].map((words) => ({
    words,
    // what a person might type: case and spacing are normalized away
    typed: `  ${words.toUpperCase().split(" ").join("   ")} `,
    phraseId: phraseId(words),
    accounts: [0, 1, 2, 7, 0x7fffffff].map((index) => {
      const kp = keypairFromMnemonic(words, index);
      return { index, address: kp.toSuiAddress(), publicKey: toBase64(kp.getPublicKey().toRawBytes()), secretKey: kp.getSecretKey() };
    }),
  }));

  // ---- imported keys of every scheme
  const seeds = { ED25519: sha256(enc.encode("rare ed25519")), Secp256k1: sha256(enc.encode("rare k1")), Secp256r1: sha256(enc.encode("rare r1")) };
  const kps = {
    ED25519: Ed25519Keypair.fromSecretKey(seeds.ED25519),
    Secp256k1: Secp256k1Keypair.fromSecretKey(seeds.Secp256k1),
    Secp256r1: Secp256r1Keypair.fromSecretKey(seeds.Secp256r1),
  };
  const sender = keypairFromMnemonic(PHRASE).toSuiAddress();
  const tx = txBytes({ sender, gasOwner: sender });
  const message = enc.encode("Sign in to Rare\n\nNonce: 42");
  out.tx = toBase64(tx);
  out.message = toBase64(message);
  const keys = [];
  for (const [scheme, kp] of Object.entries(kps)) {
    keys.push({
      scheme,
      seedHex: bytesToHex(seeds[scheme as keyof typeof seeds]),
      secretKey: kp.getSecretKey(),
      address: kp.toSuiAddress(),
      publicKey: toBase64(kp.getPublicKey().toRawBytes()),
      txSignature: (await kp.signTransaction(tx)).signature,
      messageSignature: (await kp.signPersonalMessage(message)).signature,
    });
  }
  // a phrase account signs too
  const phraseKp = keypairFromMnemonic(PHRASE, 0);
  keys.push({
    scheme: "phrase",
    secretKey: phraseKp.getSecretKey(),
    address: phraseKp.toSuiAddress(),
    publicKey: toBase64(phraseKp.getPublicKey().toRawBytes()),
    txSignature: (await phraseKp.signTransaction(tx)).signature,
    messageSignature: (await phraseKp.signPersonalMessage(message)).signature,
  });
  out.keys = keys;
  // bare 32-byte hex Ed25519 secrets (older Sui Wallet exports)
  out.hexImport = { hex: "0x" + bytesToHex(seeds.ED25519), address: keypairFromSecret("0x" + bytesToHex(seeds.ED25519)).toSuiAddress() };

  // ---- passkey accounts: 1-of-2 multisig of the passkey (secp256r1) and a device key (Ed25519)
  const passkeySecret = sha256(enc.encode("rare passkey"));
  const passkeyPk = p256.getPublicKey(passkeySecret, true);
  const device = Ed25519Keypair.fromSecretKey(sha256(enc.encode("rare device")));
  const ms = passkeyMultisig(passkeyPk, device.getPublicKey().toRawBytes());
  const account: RareAccount = {
    address: ms.toSuiAddress(),
    label: "Passkey 1",
    kind: "passkey",
    passkey: { credentialId: toBase64(new Uint8Array([9, 8, 7, 6])), publicKey: toBase64(passkeyPk), deviceKey: toBase64(device.getPublicKey().toRawBytes()) },
  };
  const msTx = txBytes({ sender: account.address, gasOwner: account.address });
  const deviceSigner = new MultisigDeviceSigner(account, device);
  const assertions: { challenge: string; authenticatorData: string; clientDataJSON: string; signature: string }[] = [];
  const provider = (highS: boolean): PasskeyProvider => ({
    create: async () => {
      throw new Error("no");
    },
    get: async (challenge) => {
      const a = assertion(passkeySecret, challenge, highS);
      assertions.push({ challenge: toBase64(challenge), authenticatorData: toBase64(a.authenticatorData), clientDataJSON: toBase64(a.clientDataJSON), signature: toBase64(a.signature) });
      return { response: { authenticatorData: a.authenticatorData, clientDataJSON: a.clientDataJSON, signature: a.signature } } as never;
    },
  });
  const phone = (highS: boolean) => new PasskeyKeypair(passkeyPk, provider(highS), fromBase64(account.passkey!.credentialId));
  const passkeyAlone = await phone(false).signTransaction(msTx);
  const passkeyHighS = await phone(true).signPersonalMessage(message);
  const viaPhone = new MultisigMemberSigner(account, {
    signTransaction: async (b) => (await phone(true).signTransaction(b)).signature,
    signPersonalMessage: async (b) => (await phone(false).signPersonalMessage(b)).signature,
  });
  out.passkey = {
    passkeySecretHex: bytesToHex(passkeySecret),
    deviceSecretKey: device.getSecretKey(),
    account,
    multisigPublicKey: toBase64(ms.toRawBytes()),
    passkeyAddress: new PasskeyKeypair(passkeyPk, provider(false)).toSuiAddress(),
    tx: toBase64(msTx),
    device: { txSignature: await deviceSigner.signTransaction(msTx), messageSignature: (await deviceSigner.signPersonalMessage(message)).signature },
    // the passkey alone (its own address), low-S and high-S DER from the authenticator
    alone: { txSignature: passkeyAlone.signature, txAssertion: assertions[0], messageSignature: passkeyHighS.signature, messageAssertion: assertions[1] },
    // the passkey as the account's multisig member (a phone tap on a device without the key)
    member: {
      txSignature: await viaPhone.signTransaction(msTx),
      txAssertion: assertions[2],
      messageSignature: (await viaPhone.signPersonalMessage(message)).signature,
      messageAssertion: assertions[3],
    },
  };

  // P-256 key recovery from an assertion (sign in on a new device)
  {
    const a = assertions[0];
    const recovered = recoverPasskeyKeys(
      { credentialId: "AQID", authenticatorData: a.authenticatorData, clientDataJSON: a.clientDataJSON, signature: a.signature, userHandle: null, prf: null },
      fromBase64(a.challenge),
    );
    (out.passkey as Record<string, unknown>).recovered = recovered.map((k) => toBase64(k.toRawBytes()));
    (out.passkey as Record<string, unknown>).userHandle = toBase64(userHandleFor(device.getPublicKey().toRawBytes()));
  }

  // ---- the keystore end to end: payload and account metadata as TS writes them
  const disk = new MemoryKV();
  const vault = memoryVault();
  const ks = new Keystore({ disk, vault });
  const a0 = await ks.addPhrase(PHRASE, { pin: { secret: "123456", kind: "pin" } });
  const a1 = await ks.deriveNext({ label: "Games" });
  const imported = await ks.addPrivateKey(kps.Secp256k1.getSecretKey());
  await ks.addWatch("0x" + "ab".repeat(32));
  await ks.addLedger(toBase64(kps.ED25519.getPublicKey().toRawBytes()), "m/44'/784'/3'/0'/0'", undefined, "Nano S Plus");
  await ks.addKeystone(toBase64(Ed25519Keypair.fromSecretKey(sha256(enc.encode("keystone"))).getPublicKey().toRawBytes()), "m/44'/784'/0'/0'/0'", "1250B6BC", "My Keystone");
  await ks.addPasskey({ ...account.passkey!, deviceSecret: device.getSecretKey(), authenticator: "iPhone" });
  await ks.select(a1.address);
  out.keystore = {
    payload: JSON.parse((await vault.read())!),
    meta: await disk.get("rare.meta"),
    derive: {
      phrase: bytesToHex(await ks.deriveFromKey(a0.address, DERIVE_SALT, deriveInfo("cloud-saves", a0.address))),
      imported: bytesToHex(await ks.deriveFromKey(imported.address, DERIVE_SALT, deriveInfo("cloud-saves", imported.address))),
    },
  };

  // ---- secret.ts building blocks
  const ledgerSig = (await phraseKp.signPersonalMessage(deriveMessage("cloud-saves", sender))).signature;
  out.secret = {
    deriveSalt: toHex(DERIVE_SALT),
    info: toHex(deriveInfo("cloud-saves", sender.toUpperCase().replace("0X", "0x"))),
    prfInput: toHex(prfInput("cloud-saves")),
    deriveMessage: toHex(deriveMessage("cloud-saves", sender)),
    ledgerSignature: ledgerSig,
    ledgerFinish: toHex(finish(ed25519SignatureBytes(ledgerSig), "cloud-saves", sender)),
    finishFromPrf: toHex(finish(sha256(enc.encode("prf output")), "cloud-saves", sender)),
  };
  // ---- sponsor.ts: the kind a game's full bytes are sponsored by, and the gas coin refusal
  const gameBytes = txBytes({ sender, gasOwner: sender });
  const gasBytes = txBytes({ sender, gasOwner: sender, gasCoin: true });
  out.sponsor = {
    gameBytes: toBase64(gameBytes),
    gameKind: (kindOfBytes(gameBytes) as { kind: string }).kind,
    gasBytes: toBase64(gasBytes),
    gasResult: kindOfBytes(gasBytes),
    needs: [0, 14_000_000, 16_000_000].map((b) => needsSponsor(BigInt(b))),
    categories: ["pawtato-land", "Bad Slug", "-x", "a".repeat(64), "a".repeat(65)].map((s) => gameCategory(s)),
  };
  // ---- Ledger: the exact APDUs the Sui app client sends, and what comes back
  {
    const t = new FakeLedgerTransport();
    const app = new Sui(t) as unknown as SuiLedgerApp;
    const path = "m/44'/784'/0'/0'/0'";
    const info = await checkSuiApp(t);
    const account = await readLedgerAccount(app, path, true);
    const pk = account.publicKey;
    const objects = [new Uint8Array(150).fill(0xaa), new Uint8Array(40).fill(0xbb)];
    const txSignature = await ledgerSignTransaction(app, { path, publicKey: pk, txBytes: gameBytes, objects: async () => objects });
    const messageSignature = await ledgerSignPersonalMessage(app, { path, publicKey: pk, message });
    out.ledger = {
      path,
      appInfo: info,
      address: account.address,
      publicKey: toBase64(pk),
      objectIds: clearSigningObjectIds(gameBytes),
      objects: objects.map((o) => toBase64(o)),
      txSignature,
      messageSignature,
      apdus: t.dev.apdus,
    };
  }
  // ---- Keystone: the URs Rare Wallet sends, and the device's answers (from the TS mock device)
  {
    mockKeystoneDevice().promptDelayMs = 0;
    const paths = ["m/44'/784'/0'/0'/0'", "m/44'/784'/5'/0'/0'"];
    const derivation = keyDerivationUR(paths);
    const accountsAnswer = await mockKeystoneSend(2, derivation);
    const digest = suiSigningDigest("transaction", gameBytes);
    const requestId = "0b4f2f6e-3c4a-4d6e-9a1b-2c3d4e5f6a7b";
    const kp0 = keypairFromMnemonic(PHRASE, 0);
    const signRequest = signHashUR({ digest, path: paths[0], mfp: "73c5da0a", address: kp0.toSuiAddress(), requestId });
    const signAnswer = await mockKeystoneSend(2, signRequest.ur);
    const bare = encodeUR(new SuiSignature(Buffer.from(new Uint8Array(64).fill(9))).toUR());
    out.keystone = {
      paths,
      derivation,
      accountsAnswer,
      digest: toHex(digest),
      messageDigest: toHex(suiSigningDigest("message", message)),
      requestId,
      signRequest: signRequest.ur,
      signAnswer,
      bareSignature: bare,
    };
  }
  // ---- web games: Transaction JSON as a game sends it (toJSON), and what getData() says it is
  {
    const games: Transaction[] = [];
    {
      const tx = new Transaction();
      tx.setSender(sender);
      const [c] = tx.splitCoins(tx.gas, [tx.pure.u64(1000)]);
      tx.transferObjects([c], tx.pure.address(ADDR(0xde)));
      games.push(tx);
    }
    {
      const tx = new Transaction();
      tx.setSender(sender);
      const r = tx.moveCall({ target: `${ADDR(0x9a)}::game::play`, typeArguments: ["0x2::sui::SUI"], arguments: [tx.object(ADDR(0x77)), tx.pure.u64(5), tx.pure.string("hi")] });
      const v = tx.makeMoveVec({ type: "u64", elements: [tx.pure.u64(1), tx.pure.u64(2)] });
      tx.moveCall({ target: `${ADDR(0x9a)}::game::take`, arguments: [r, v] });
      tx.mergeCoins(tx.gas, [tx.object(ADDR(0x78))]);
      games.push(tx);
    }
    {
      const tx = new Transaction();
      tx.setSender(sender);
      const usdc = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";
      tx.transferObjects([coinWithBalance({ type: usdc, balance: 2500000 })], tx.pure.address(ADDR(0xde)));
      games.push(tx);
    }
    {
      const tx = new Transaction();
      tx.setSender(sender);
      tx.moveCall({ target: `${ADDR(0x9a)}::game::play`, arguments: [tx.object(ADDR(0x77))] });
      tx.setGasBudget(3_000_000);
      tx.setGasPrice(1000);
      tx.setGasOwner(ADDR(0x55));
      tx.setGasPayment([{ objectId: ADDR(0x56), version: "9", digest: "4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi" }]);
      games.push(tx);
    }
    const out2 = [];
    // toJSON needs a client for coin intents (a game can't send those without one): use getData's JSON there
    for (const tx of games) {
      const data = JSON.parse(JSON.stringify(tx.getData(), (_k, v) => (typeof v === "bigint" ? v.toString() : v)));
      const intents = data.commands.some((c: { $kind: string }) => c.$kind === "$Intent");
      out2.push({ json: intents ? JSON.stringify(data) : await tx.toJSON(), data });
    }
    const errs = [
      ["signTransaction", {}],
      ["signPersonalMessage", {}],
      ["signTransaction", { transaction: "{not json" }],
      ["foo", {}],
    ].map(([method, params]) => mapGameRequest({ method: method as never, params: params as never }));
    out.webgame = { transactions: out2, errors: errs };
  }
  return out;
}

describe("TS/Rust wallet vectors", () => {
  it("the TS wallet still produces ts-vectors.json", async () => {
    const got = JSON.parse(JSON.stringify(await generate()));
    if (process.env.RARE_WRITE_VECTORS) writeFileSync(FILE, JSON.stringify(got, null, 2) + "\n");
    expect(got).toEqual(JSON.parse(readFileSync(FILE, "utf8")));
  });
});
