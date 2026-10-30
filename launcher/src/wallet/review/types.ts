/* Normalized pre-sign simulation result. Produced by a Simulator (gRPC
 * against the fullnode, fixed results in tests) and read by analyze()/riskChecks(). */

export type Owner =
  | { kind: 'address'; address: string }
  /** dynamic field / child of another object (kiosks, tables, wrapped storage) */
  | { kind: 'object'; id: string }
  | { kind: 'shared' }
  | { kind: 'immutable' }
  | { kind: 'unknown' }

export interface SimObjectChange {
  objectId: string
  /** full Move type, e.g. `0x2::coin::Coin<0x2::sui::SUI>`; null when the node didn't say */
  type: string | null
  inputOwner: Owner | null
  outputOwner: Owner | null
  /** Created / Deleted come from the node; None = the object existed before and after (or was wrapped) */
  idOperation: 'Created' | 'Deleted' | 'None' | 'Unknown'
  inputExists: boolean
  outputExists: boolean
  /** PackageWrite (publish/upgrade) and AccumulatorWriteV1 (address balances) are not user objects */
  outputKind?: 'object' | 'package' | 'accumulator' | 'unknown'
}

export interface SimBalanceChange {
  coinType: string
  address: string
  /** signed base units */
  amount: string
}

export interface SimFailure {
  /** node message, e.g. `MoveAbort in 1st command, abort code: 2, in '0x2::balance::split'` */
  message: string
  kind?: string
  command?: number
  abort?: { code: string; package?: string; module?: string; function?: string; constant?: string }
}

export interface Simulation {
  /** who produced this; Rare only simulates on a Sui fullnode */
  source?: 'node'
  status: 'success' | 'failure'
  failure?: SimFailure
  balanceChanges: SimBalanceChange[]
  objects: SimObjectChange[]
  /** base units of SUI (MIST) */
  gas: { computation: string; storage: string; rebate: string }
  /** owner of the gas coin before the tx; the signer unless sponsored */
  gasOwner: string | null
  gasObjectId?: string | null
  /** optional extras fetched after the simulation (best effort) */
  display?: Record<string, { name?: string; image?: string }>
  /** called package id -> its original (first version) id */
  packages?: Record<string, string>
  /** on-chain coin metadata for coins in the balance changes */
  coins?: Record<string, { symbol?: string; decimals: number; name?: string; iconUrl?: string | null }>
}

export interface Simulator {
  /** Simulate exact signed-to-be bytes. Throws only when the node can't be reached. */
  simulate(txBytes: Uint8Array): Promise<Simulation>
}
