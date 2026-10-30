/* Kiosk handles for PTBs: borrow caps (personal or
 * not), create kiosks, and always give borrowed personal caps back before the transaction ends. */

import type { Transaction, TransactionObjectArgument } from '@mysten/sui/transactions'
import { KIOSK_TYPE, target } from './ids'
import type { KioskRef } from './types'

export interface KioskHandle {
  kiosk: TransactionObjectArgument
  /** A KioskOwnerCap argument usable as &KioskOwnerCap. */
  cap: TransactionObjectArgument
  /** On-chain id, or null for a kiosk created in this transaction. */
  kioskId: string | null
  isPersonal: boolean
  isNew: boolean
  /** Set when an item was placed or locked into the kiosk during this transaction. */
  used: boolean
  /** For a personal kiosk created for another address: run right after create_for makes it personal. */
  onPersonal?: Array<() => void>
}

interface Entry {
  handle: KioskHandle
  release: () => void
}

/**
 * Tracks every kiosk touched by one transaction so each cap is borrowed once and returned
 * exactly once. Call `finish()` after the last command that uses a kiosk.
 */
export class KioskSession {
  private readonly byId = new Map<string, Entry>()
  private readonly order: Entry[] = []
  private finished = false

  constructor(readonly tx: Transaction, readonly sender: string) {}

  /** Borrow an existing kiosk's cap (personal caps via personal_kiosk::borrow_val). */
  open(ref: KioskRef): KioskHandle {
    this.assertOpen()
    const cached = this.byId.get(ref.kioskId)
    if (cached) return cached.handle
    const tx = this.tx
    let cap: TransactionObjectArgument
    let release = () => {}
    if (ref.isPersonal) {
      if (!ref.personalCapId) throw new Error(`Personal kiosk ${ref.kioskId} is missing its PersonalKioskCap id`)
      const personalCapId = ref.personalCapId
      const [borrowed, promise] = tx.moveCall({ target: target.personalKiosk('borrow_val'), arguments: [tx.object(personalCapId)] })
      cap = borrowed
      release = () => {
        tx.moveCall({ target: target.personalKiosk('return_val'), arguments: [tx.object(personalCapId), borrowed, promise] })
      }
    } else {
      if (!ref.ownerCapId) throw new Error(`Kiosk ${ref.kioskId} is missing its KioskOwnerCap id`)
      cap = tx.object(ref.ownerCapId)
    }
    const entry: Entry = {
      handle: { kiosk: tx.object(ref.kioskId), cap, kioskId: ref.kioskId, isPersonal: ref.isPersonal, isNew: false, used: false },
      release,
    }
    this.byId.set(ref.kioskId, entry)
    this.order.push(entry)
    return entry.handle
  }

  /** Create a kiosk owned by `owner` inside this transaction; shared (and its cap handed out) in `finish()`. */
  create(options: { personal?: boolean; owner: string }): KioskHandle {
    this.assertOpen()
    const tx = this.tx
    const owner = options.owner
    if (owner === this.sender) throw new Error('Kiosks are only created here for another address')
    const [kiosk, rawCap] = tx.moveCall({ target: '0x2::kiosk::new' })
    let handle: KioskHandle
    let release: () => void
    if (options.personal) {
      // items are locked with the raw cap first; create_for then wraps the cap into a
      // PersonalKioskCap owned by `owner` (and sets the kiosk owner)
      const onPersonal: Array<() => void> = []
      handle = { kiosk, cap: rawCap, kioskId: null, isPersonal: true, isNew: true, used: false, onPersonal }
      release = () => {
        tx.moveCall({ target: target.personalKiosk('create_for'), arguments: [kiosk, rawCap, tx.pure.address(owner)] })
        onPersonal.forEach((fn) => fn())
        tx.moveCall({ target: '0x2::transfer::public_share_object', typeArguments: [KIOSK_TYPE], arguments: [kiosk] })
      }
    } else {
      handle = { kiosk, cap: rawCap, kioskId: null, isPersonal: false, isNew: true, used: false }
      release = () => {
        tx.moveCall({ target: '0x2::kiosk::set_owner_custom', arguments: [kiosk, rawCap, tx.pure.address(owner)] })
        tx.moveCall({ target: '0x2::transfer::public_share_object', typeArguments: [KIOSK_TYPE], arguments: [kiosk] })
        tx.transferObjects([rawCap], owner)
      }
    }
    this.order.push({ handle, release })
    return handle
  }

  /** Return every borrowed cap and share every new kiosk. Safe to call once. */
  finish(): void {
    if (this.finished) return
    this.finished = true
    for (const entry of [...this.order].reverse()) entry.release()
  }

  private assertOpen() {
    if (this.finished) throw new Error('KioskSession already finished')
  }
}
