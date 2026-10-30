// Ported from rare.market:
//   market/web/lib/chain/listing.ts   (buildList, the "native" LISTING_MODE path)
//   market/web/lib/chain/kiosk.ts     (KioskScope, sender-owned kiosks only)
//   market/web/lib/chain/purchase.ts  (listInOwnKiosk, delistFromOwnKiosk)
//
// rare.market lists with 0x2::kiosk::list in the seller's own kiosk (zero fee) while its own
// contract ids are empty, so only that path is here. Wallet items are placed (or locked, when the
// policy has a lock rule) into a kiosk first, creating one in the same PTB when the seller has none
// (personal when the policy needs it). Tradeport and Drip listings are not delisted here.

import { Transaction, type TransactionObjectArgument } from "@mysten/sui/transactions";
import { KIOSK_EXT } from "./ids";
import type { OwnedKioskRef, TransferPolicy } from "./model";
import { readPolicy } from "./policy";

const KIOSK_TYPE = "0x2::kiosk::Kiosk";
const personalKiosk = (fn: string) => `${KIOSK_EXT.latest}::personal_kiosk::${fn}`;

const VENUE_LABEL: Record<string, string> = { market: "Rare", tradeport: "Tradeport", drip: "Drip", kiosk: "Kiosk" };

export interface KioskArg {
  kiosk: TransactionObjectArgument;
  /** Usable as &KioskOwnerCap. */
  cap: TransactionObjectArgument;
  /** On-chain id, or null for a kiosk created in this transaction. */
  kioskId: string | null;
  isPersonal: boolean;
  isNew: boolean;
  /** Set once an item was placed or locked into it in this transaction. */
  used: boolean;
}

interface Slot {
  arg: KioskArg;
  close: () => void;
}

/**
 * Every kiosk one transaction touches. `open` and `create` hand out kiosk arguments; `close`
 * returns borrowed caps and shares new kiosks, in reverse order. Call `close` once, after the last
 * command that uses a kiosk.
 */
export class KioskScope {
  private readonly opened = new Map<string, Slot>();
  private readonly slots: Slot[] = [];
  private closed = false;

  constructor(
    readonly tx: Transaction,
    readonly sender: string,
  ) {}

  open(ref: OwnedKioskRef): KioskArg {
    this.ensureOpen();
    const hit = this.opened.get(ref.kioskId);
    if (hit) return hit.arg;
    const tx = this.tx;
    let cap: TransactionObjectArgument;
    let close = () => {};
    if (ref.isPersonal) {
      if (!ref.personalCapId) throw new Error(`Personal kiosk ${ref.kioskId} has no PersonalKioskCap id`);
      const personalCapId = ref.personalCapId;
      const [borrowed, promise] = tx.moveCall({ target: personalKiosk("borrow_val"), arguments: [tx.object(personalCapId)] });
      cap = borrowed;
      close = () => {
        tx.moveCall({ target: personalKiosk("return_val"), arguments: [tx.object(personalCapId), borrowed, promise] });
      };
    } else {
      if (!ref.ownerCapId) throw new Error(`Kiosk ${ref.kioskId} has no KioskOwnerCap id`);
      cap = tx.object(ref.ownerCapId);
    }
    const slot: Slot = { arg: { kiosk: tx.object(ref.kioskId), cap, kioskId: ref.kioskId, isPersonal: ref.isPersonal, isNew: false, used: false }, close };
    this.opened.set(ref.kioskId, slot);
    this.slots.push(slot);
    return slot.arg;
  }

  create(options: { personal?: boolean } = {}): KioskArg {
    this.ensureOpen();
    const tx = this.tx;
    const [kiosk, rawCap] = tx.moveCall({ target: "0x2::kiosk::new" });
    let arg: KioskArg;
    let close: () => void;
    if (options.personal) {
      const personalCap = tx.moveCall({ target: personalKiosk("new"), arguments: [kiosk, rawCap] });
      const [borrowed, promise] = tx.moveCall({ target: personalKiosk("borrow_val"), arguments: [personalCap] });
      arg = { kiosk, cap: borrowed, kioskId: null, isPersonal: true, isNew: true, used: false };
      close = () => {
        tx.moveCall({ target: personalKiosk("return_val"), arguments: [personalCap, borrowed, promise] });
        tx.moveCall({ target: personalKiosk("transfer_to_sender"), arguments: [personalCap] });
        tx.moveCall({ target: "0x2::transfer::public_share_object", typeArguments: [KIOSK_TYPE], arguments: [kiosk] });
      };
    } else {
      arg = { kiosk, cap: rawCap, kioskId: null, isPersonal: false, isNew: true, used: false };
      close = () => {
        tx.moveCall({ target: "0x2::transfer::public_share_object", typeArguments: [KIOSK_TYPE], arguments: [kiosk] });
        tx.transferObjects([rawCap], this.sender);
      };
    }
    const slot: Slot = { arg, close };
    this.slots.push(slot);
    return arg;
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    for (const slot of [...this.slots].reverse()) slot.close();
  }

  private ensureOpen() {
    if (this.closed) throw new Error("Kiosk scope already closed");
  }
}

/** List an item in the sender's own kiosk with kiosk::list (no marketplace contract). */
export function listInOwnKiosk(tx: Transaction, kiosk: KioskArg, item: { nftId: string; nftType: string; priceMist: bigint }): void {
  tx.moveCall({ target: "0x2::kiosk::list", typeArguments: [item.nftType], arguments: [kiosk.kiosk, kiosk.cap, tx.pure.id(item.nftId), tx.pure.u64(item.priceMist)] });
}

export function delistFromOwnKiosk(tx: Transaction, kiosk: KioskArg, item: { nftId: string; nftType: string }): void {
  tx.moveCall({ target: "0x2::kiosk::delist", typeArguments: [item.nftType], arguments: [kiosk.kiosk, kiosk.cap, tx.pure.id(item.nftId)] });
}

export interface ListingItem {
  nftId: string;
  nftType: string;
  priceMist: bigint;
  /** The seller's kiosk the item is in, or null when it is in the wallet. */
  kiosk: OwnedKioskRef | null;
  policy: TransferPolicy | null;
  /** Existing listing of this item, if any. */
  listing?: { venue: string; orderKey?: string };
}

/** Pick (or create) the kiosk a wallet item goes into, honouring the collection's policy. */
function homeFor(scope: KioskScope, item: ListingItem, kiosks: OwnedKioskRef[], made: { plain?: KioskArg; personal?: KioskArg }): KioskArg {
  const shape = readPolicy(item.policy);
  const personal = kiosks.find((k) => k.isPersonal && k.personalCapId);
  const plain = kiosks.find((k) => !k.isPersonal && k.ownerCapId);
  if (shape.personal) {
    if (personal) return scope.open(personal);
    return (made.personal ??= scope.create({ personal: true }));
  }
  const existing = plain ?? personal;
  if (existing) return scope.open(existing);
  return (made.plain ??= scope.create());
}

function stow(tx: Transaction, k: KioskArg, item: ListingItem): void {
  const shape = readPolicy(item.policy);
  if (shape.lock) {
    if (!item.policy) throw new Error("This collection locks items but its policy is unknown.");
    tx.moveCall({ target: "0x2::kiosk::lock", typeArguments: [item.nftType], arguments: [k.kiosk, k.cap, tx.object(item.policy.id), tx.object(item.nftId)] });
  } else {
    tx.moveCall({ target: "0x2::kiosk::place", typeArguments: [item.nftType], arguments: [k.kiosk, k.cap, tx.object(item.nftId)] });
  }
  k.used = true;
}

/** One PTB listing every item natively in the seller's kiosk. A native relist delists and lists again. */
export function buildList(sender: string, items: ListingItem[], kiosks: OwnedKioskRef[], tx = new Transaction()): Transaction {
  if (!items.length) throw new Error("Nothing to list.");
  tx.setSender(sender);
  const scope = new KioskScope(tx, sender);
  const made: { plain?: KioskArg; personal?: KioskArg } = {};
  for (const item of items) {
    if (item.priceMist <= 0n) throw new Error("Price must be above zero.");
    const existing = item.listing;
    if (existing) {
      if (existing.venue !== "kiosk") throw new Error(`Delist it on ${VENUE_LABEL[existing.venue] ?? existing.venue} first.`);
      if (!item.kiosk) throw new Error("Your wallet doesn't hold the kiosk this item is in.");
      const k = scope.open(item.kiosk);
      delistFromOwnKiosk(tx, k, item);
      listInOwnKiosk(tx, k, item);
      continue;
    }
    let k: KioskArg;
    if (item.kiosk) k = scope.open(item.kiosk);
    else {
      k = homeFor(scope, item, kiosks, made);
      stow(tx, k, item);
    }
    listInOwnKiosk(tx, k, item);
  }
  scope.close();
  return tx;
}
