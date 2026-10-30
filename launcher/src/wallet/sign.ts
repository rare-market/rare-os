// Rare Wallet's signing API. Every signature in the launcher goes through here: Buy in the
// Marketplace, sends from the wallet, and web games over Wallet Standard. Each call queues a
// request; the global review sheet (ui/ReviewSheet) shows what leaves and what arrives and the
// gas, the person approves (PIN, phone tap, or hardware confirm as the account needs), and only
// then is anything signed. Nothing is ever signed without a review.
//
//   import { signAndExecute } from "../wallet/sign";
//   const { digest } = await signAndExecute(tx, { origin: { name: "Rare Marketplace", kind: "market" } });
//
// Rejection throws SignRejected. `dryRun` signs and simulates but never submits (tests, demos).

import { Transaction } from "@mysten/sui/transactions";
import { fromBase64, toBase64 } from "@mysten/sui/utils";
import { create } from "zustand";
import { grpcClient, keystore } from "./index";
import { gameFeeNote, runSponsored, sponsorForGame, sponsorOffer, sponsorTransaction, type AnyCategory, type SponsorCategory, type SponsorDeps, type Sponsorship } from "./sponsor";
import { devSign } from "./ui/dev";

export interface SignOrigin {
  /** "Rare Marketplace", a game's title, or "Rare Wallet" for the wallet's own sends */
  name: string;
  /** web origin for games */
  url?: string;
  icon?: string;
  kind: "wallet" | "market" | "game";
}

export class SignRejected extends Error {
  constructor(message = "You declined the request") {
    super(message);
    this.name = "SignRejected";
  }
}

export type SignKind = "transaction" | "message";

export interface SignRequest {
  id: number;
  kind: SignKind;
  /** account that signs (address) */
  account: string;
  origin: SignOrigin;
  /** transaction requests: the Transaction (sender set here) or built bytes from a game */
  tx?: Transaction | Uint8Array;
  /** message requests */
  message?: Uint8Array;
  /** submit after signing (signAndExecute) */
  execute: boolean;
  dryRun: boolean;
  /** title override for the sheet ("Buy Popkins #18736", "Send 5 SUI") */
  title?: string;
  /** Rare pays the fee: `tx` holds the sponsored bytes; Approve signs them and rare.market submits */
  sponsored?: SponsoredSign;
  /** a line under the network fee ("<game>'s gas budget is used up, so you pay this fee.") */
  feeNote?: string;
  resolve: (r: SignResult) => void;
  reject: (e: Error) => void;
}

export interface SignResult {
  /** base64 transaction bytes or message bytes */
  bytes: string;
  signature: string;
  /** set when executed (or dry-run digest) */
  digest?: string;
  /** "success" | "failure" after execution */
  status?: string;
  /** base64 BCS TransactionEffects after execution (Wallet Standard signAndExecuteTransaction) */
  effects?: string;
  dryRun?: boolean;
}

export interface SponsoredSign {
  sponsor: string;
  category: AnyCategory;
  /** who pays, for the review sheet: "Rare", or the game whose gas budget does */
  payer?: string;
  /** the transaction the kind came from, to ask again if the sponsorship expires */
  source: Transaction;
  first: Sponsorship;
}

interface Queue {
  requests: SignRequest[];
}

/** Pending requests, oldest first. The review sheet shows requests[0]. */
export const useSignRequests = create<Queue>(() => ({ requests: [] }));

let seq = 0;

function enqueue(r: Omit<SignRequest, "id" | "resolve" | "reject">): Promise<SignResult> {
  return new Promise((resolve, reject) => {
    const req: SignRequest = { ...r, id: ++seq, resolve, reject };
    useSignRequests.setState((s) => ({ requests: [...s.requests, req] }));
  });
}

function finish(id: number) {
  useSignRequests.setState((s) => ({ requests: s.requests.filter((r) => r.id !== id) }));
}

async function signerAccount(address?: string): Promise<string> {
  if (address) return address;
  const cur = await keystore.current();
  if (!cur) throw new Error("Add a Rare Wallet account first");
  if (cur.kind === "watch" && !devSign) throw new Error("This account is watch only. Add a signing account to continue.");
  return cur.address;
}

export interface SignOptions {
  /** signing account (address); the selected account by default */
  account?: string;
  origin?: SignOrigin;
  title?: string;
  dryRun?: boolean;
  /** shown under the network fee in the review sheet */
  feeNote?: string;
}

const WALLET: SignOrigin = { name: "Rare Wallet", kind: "wallet" };

/** Review, sign and submit. Resolves with the digest once the network has the transaction. */
export async function signAndExecute(tx: Transaction | Uint8Array, o: SignOptions = {}): Promise<SignResult> {
  const account = await signerAccount(o.account);
  return enqueue({ kind: "transaction", account, origin: o.origin ?? WALLET, tx, execute: true, dryRun: !!o.dryRun, title: o.title, feeNote: o.feeNote });
}

/**
 * Review, sign and submit a transaction whose fee Rare covers. `sponsorship` comes from
 * sponsorTransaction(tx, ...); the review shows those exact bytes, and rare.market submits them
 * with both signatures. An expired sponsorship is asked for again once at Approve.
 */
export async function signAndExecuteSponsored(tx: Transaction, sponsorship: Sponsorship, category: AnyCategory, o: SignOptions & { payer?: string } = {}): Promise<SignResult> {
  const account = await signerAccount(o.account);
  return enqueue({
    kind: "transaction",
    account,
    origin: o.origin ?? WALLET,
    tx: fromBase64(sponsorship.bytes),
    execute: true,
    dryRun: !!o.dryRun,
    title: o.title,
    sponsored: { sponsor: sponsorship.sponsor, category, source: tx, first: sponsorship, payer: o.payer },
  });
}

/**
 * A transaction a game asked for (a web game's signAndExecute, or a Local API sell, buy or claim):
 * the game's gas budget pays the fee when it covers it (category "game:<slug>"); otherwise
 * `fallback` runs (default: the wallet pays), with a note in the review when the budget is used up.
 * `tx` may be the full bytes a web game built (its gas data is dropped when the game pays).
 */
export async function signAndExecuteForGame(
  tx: Transaction | Uint8Array,
  game: { slug: string | null; name: string },
  o: SignOptions = {},
  fallback?: (o: SignOptions) => Promise<SignResult>,
  d?: SponsorDeps,
): Promise<SignResult> {
  const account = await signerAccount(o.account);
  let feeNote = o.feeNote;
  if (game.slug) {
    const s = await sponsorForGame(tx, account, game.slug, d);
    if (s.ok) return signAndExecuteSponsored(s.source, s, s.category, { ...o, account, payer: game.name });
    feeNote = gameFeeNote(s, game.name) ?? feeNote;
  }
  const next = { ...o, account, feeNote };
  return fallback ? fallback(next) : signAndExecute(tx, next);
}

/**
 * For trophy claims and Walrus uploads (and any other sponsored category): Rare covers the fee
 * when the wallet is short of SUI and rare.market offers `category` to it; otherwise, or when the
 * sponsor says no, the wallet pays as usual. Build `tx` without the gas coin (no `tx.gas`, and
 * `useGasCoin: false` on SUI coins), or it can only go out with the wallet paying.
 */
export async function signAndExecuteWithCover(tx: Transaction, category: SponsorCategory, o: SignOptions = {}): Promise<SignResult> {
  const account = await signerAccount(o.account);
  tx.setSenderIfNotSet(account);
  const balance = async () => BigInt((await grpcClient.core.getBalance({ owner: account, coinType: "0x2::sui::SUI" })).balance.balance);
  if (await sponsorOffer(account, category, { suiBalanceMist: balance })) {
    const s = await sponsorTransaction(tx, account, category);
    if (s.ok) return signAndExecuteSponsored(tx, s, category, { ...o, account });
  }
  return signAndExecute(tx, { ...o, account });
}

/** Review and sign without submitting (games that submit themselves). */
export async function signTransaction(tx: Transaction | Uint8Array, o: SignOptions = {}): Promise<SignResult> {
  const account = await signerAccount(o.account);
  return enqueue({ kind: "transaction", account, origin: o.origin ?? WALLET, tx, execute: false, dryRun: !!o.dryRun, title: o.title });
}

export async function signPersonalMessage(message: Uint8Array, o: SignOptions = {}): Promise<SignResult> {
  const account = await signerAccount(o.account);
  return enqueue({ kind: "message", account, origin: o.origin ?? WALLET, message, execute: false, dryRun: !!o.dryRun, title: o.title });
}

/** The sheet's Cancel. */
export function rejectRequest(id: number, reason?: string) {
  const r = useSignRequests.getState().requests.find((x) => x.id === id);
  if (!r) return;
  finish(id);
  r.reject(new SignRejected(reason));
}

/** Transaction bytes for a request: builds a Transaction with the sender set (gas, coins resolved over gRPC). */
export async function requestBytes(r: SignRequest): Promise<Uint8Array> {
  if (!r.tx) throw new Error("No transaction");
  if (r.tx instanceof Uint8Array) return r.tx;
  r.tx.setSenderIfNotSet(r.account);
  return r.tx.build({ client: grpcClient });
}

/**
 * The sheet's Approve, after the review and once the account can sign (vault unlocked when the
 * account needs it; the UI asks for the PIN first). Hardware and passkey signers prompt on
 * their own device; the UI shows their live state from those modules' stores.
 */
export async function approveRequest(id: number, built?: Uint8Array): Promise<SignResult> {
  const r = useSignRequests.getState().requests.find((x) => x.id === id);
  if (!r) throw new Error("Request is gone");
  try {
    const acc = (await keystore.listAccounts()).find((a) => a.address === r.account);
    // UI development only (?devsign in a browser): a watch account "signs" with a dummy and never submits
    const fake = devSign && acc?.kind === "watch";
    if (fake) r.dryRun = true;
    const signer = fake
      ? { address: r.account, signTransaction: async () => "dev-signature", signPersonalMessage: async (m: Uint8Array) => ({ bytes: toBase64(m), signature: "dev-signature" }) }
      : await keystore.getSigner(r.account);
    let result: SignResult;
    if (r.kind === "message") {
      const s = await signer.signPersonalMessage(r.message!);
      result = { bytes: s.bytes, signature: s.signature };
    } else if (r.sponsored && r.execute && !r.dryRun) {
      // rare.market adds the sponsor's signature and submits
      const sp = r.sponsored;
      const out = await runSponsored({ tx: sp.source, sender: r.account, category: sp.category, first: sp.first, sign: (b) => signer.signTransaction(b) });
      if (!out.ok) {
        if (out.reason === "failed") {
          finish(id);
          r.reject(new Error(out.message || "The network rejected this transaction"));
          return { bytes: sp.first.bytes, signature: "", status: "failure" };
        }
        throw new Error(out.message);
      }
      result = { bytes: out.bytes, signature: out.signature, digest: out.digest, status: "success" };
    } else {
      const bytes = built ?? (await requestBytes(r));
      const signature = await signer.signTransaction(bytes);
      result = { bytes: toBase64(bytes), signature };
      if (r.execute && !r.dryRun) {
        const res = await grpcClient.core.executeTransaction({ transaction: bytes, signatures: [signature], include: { effects: true } });
        const t = res.$kind === "Transaction" ? res.Transaction : res.FailedTransaction;
        const fx = t.effects?.bcs;
        result = { ...result, digest: t.digest, status: res.$kind === "Transaction" ? "success" : "failure", ...(fx ? { effects: toBase64(fx) } : {}) };
        if (res.$kind === "FailedTransaction") {
          finish(id);
          r.reject(new Error("The network rejected this transaction"));
          return result;
        }
      } else if (r.execute && r.dryRun) {
        const sim = await grpcClient.core.simulateTransaction({ transaction: bytes, include: { effects: true } });
        const t = sim.$kind === "Transaction" ? sim.Transaction : sim.FailedTransaction;
        const ok = sim.$kind === "Transaction" && t?.effects?.status.success !== false;
        result = { ...result, digest: await Transaction.from(bytes).getDigest(), status: ok ? "success" : "failure", dryRun: true };
      }
    }
    finish(id);
    r.resolve(result);
    return result;
  } catch (e) {
    // signer errors (wrong device, timeout) keep the request open so the sheet can retry
    throw e;
  }
}
