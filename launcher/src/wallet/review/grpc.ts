import type { SuiClientTypes } from '@mysten/sui/client'
import { Transaction } from '@mysten/sui/transactions'
import { isCoinObject, normId } from './packages'
import type { MoveCallRef } from './analyze'
import type { Owner, SimObjectChange, Simulation, Simulator, SimFailure } from './types'

type Sim = SuiClientTypes.SimulateTransactionResult<{ balanceChanges: true; effects: true; objectTypes: true }>

/** The slice of SuiGrpcClient the simulator uses (injectable in tests). */
export interface SimClient {
  simulateTransaction(o: { transaction: Uint8Array; include: { balanceChanges: true; effects: true; objectTypes: true } }): Promise<Sim>
  getObjects?(o: { objectIds: string[]; include: { display: true } }): Promise<{ objects: unknown[] }>
  getCoinMetadata?(o: { coinType: string }): Promise<{ coinMetadata: { decimals: number; symbol?: string; name?: string; iconUrl?: string | null } | null }>
  /** protobuf-ts UnaryCall is PromiseLike<{ response }> */
  movePackageService?: { getPackage(o: { packageId: string }): PromiseLike<{ response: { package?: { originalId?: string } } }> }
}

function mapOwner(o: SuiClientTypes.ObjectOwner | null | undefined): Owner | null {
  if (!o) return null
  switch (o.$kind) {
    case 'AddressOwner':
      return { kind: 'address', address: o.AddressOwner }
    case 'ConsensusAddressOwner':
      return { kind: 'address', address: o.ConsensusAddressOwner.owner }
    case 'ObjectOwner':
      return { kind: 'object', id: o.ObjectOwner }
    case 'Shared':
      return { kind: 'shared' }
    case 'Immutable':
      return { kind: 'immutable' }
    default:
      return { kind: 'unknown' }
  }
}

export function mapFailure(err: SuiClientTypes.ExecutionError | null | undefined): SimFailure {
  if (!err) return { message: 'Transaction failed' }
  const f: SimFailure = { message: err.message, kind: err.$kind, command: err.command }
  if (err.$kind === 'MoveAbort' && err.MoveAbort) {
    const a = err.MoveAbort
    f.abort = {
      code: String(a.abortCode),
      package: a.location?.package,
      module: a.location?.module,
      function: a.location?.functionName,
      constant: a.cleverError?.constantName,
    }
  }
  return f
}

/** Map the SDK's simulate result into a Simulation (pure; exported for tests). */
export function fromSdk(r: Sim): Simulation {
  const t = r.$kind === 'Transaction' ? r.Transaction : r.FailedTransaction
  const eff = t.effects
  const types = t.objectTypes ?? {}
  const objects: SimObjectChange[] = (eff?.changedObjects ?? []).map((o) => ({
    objectId: o.objectId,
    type: types[o.objectId] ?? null,
    inputOwner: mapOwner(o.inputOwner),
    outputOwner: mapOwner(o.outputOwner),
    idOperation: o.idOperation,
    inputExists: o.inputState === 'Exists',
    outputExists: o.outputState === 'ObjectWrite' || o.outputState === 'PackageWrite',
    outputKind: o.outputState === 'PackageWrite' ? 'package' : o.outputState === 'AccumulatorWriteV1' ? 'accumulator' : o.outputState === 'ObjectWrite' ? 'object' : 'unknown',
  }))
  const gasObj = eff?.gasObject ?? null
  const gasOwner = gasObj ? mapOwner(gasObj.inputOwner) : null
  return {
    status: t.status.success ? 'success' : 'failure',
    failure: t.status.success ? undefined : mapFailure(t.status.error),
    source: 'node',
    balanceChanges: (t.balanceChanges ?? []).map((b) => ({ coinType: b.coinType, address: b.address, amount: String(b.amount) })),
    objects,
    gas: {
      computation: String(eff?.gasUsed.computationCost ?? '0'),
      storage: String(eff?.gasUsed.storageCost ?? '0'),
      rebate: String(eff?.gasUsed.storageRebate ?? '0'),
    },
    gasOwner: gasOwner?.kind === 'address' ? gasOwner.address : null,
    gasObjectId: gasObj?.objectId ?? null,
  }
}

/** gRPC codes that mean "the node checked it and it would not run", not "the node is down". */
const REJECTED = new Set(['INVALID_ARGUMENT', 'FAILED_PRECONDITION', 'NOT_FOUND', 'OUT_OF_RANGE'])

/** Move calls in built transaction bytes, for the calls list and the unknown-contract check. */
export function callsOf(bytes: Uint8Array): MoveCallRef[] {
  try {
    const d = Transaction.from(bytes).getData()
    return d.commands.flatMap((c) => (c.$kind === 'MoveCall' ? [{ package: c.MoveCall.package, module: c.MoveCall.module, function: c.MoveCall.function }] : []))
  } catch {
    return []
  }
}

function packagesCalled(bytes: Uint8Array): string[] {
  return [...new Set(callsOf(bytes).map((c) => normId(c.package)))]
}

async function settle<T>(p: Promise<T>, ms: number): Promise<T | null> {
  return Promise.race([p.catch(() => null), new Promise<null>((r) => setTimeout(() => r(null), ms))])
}

/** Best-effort extras on a simulation: names/images of objects leaving, coin decimals, original package ids. */
export async function addExtras(client: SimClient, sim: Simulation, bytes: Uint8Array, extrasMs = 2500): Promise<void> {
  // owned objects and kiosk items (owned by a dynamic field), not the field wrappers themselves
  const leaving = sim.objects
    .filter((o) => o.inputExists && !isCoinObject(o.type) && (o.inputOwner?.kind === 'address' || (o.inputOwner?.kind === 'object' && !/::dynamic_field::Field</.test(o.type ?? ''))) && o.outputKind !== 'package')
    .map((o) => o.objectId)
  const coinTypes = [...new Set(sim.balanceChanges.map((b) => b.coinType))]
  const pkgs = packagesCalled(bytes)
  const [objs, metas, origs] = await Promise.all([
    leaving.length && client.getObjects ? settle(client.getObjects({ objectIds: leaving.slice(0, 50), include: { display: true } }), extrasMs) : null,
    client.getCoinMetadata ? Promise.all(coinTypes.map((coinType) => settle(client.getCoinMetadata!({ coinType }), extrasMs).then((m) => [coinType, m?.coinMetadata ?? null] as const))) : null,
    client.movePackageService
      ? Promise.all(
          pkgs.map((packageId) =>
            settle(Promise.resolve(client.movePackageService!.getPackage({ packageId })), extrasMs).then((r) => [packageId, r?.response.package?.originalId ?? null] as const),
          ),
        )
      : null,
  ])
  if (objs) {
    const display: NonNullable<Simulation['display']> = {}
    for (const o of objs.objects) {
      const obj = o as { objectId?: string; display?: { output?: Record<string, unknown> | null } | null }
      const out = obj?.display?.output
      if (!obj?.objectId || !out) continue
      const str = (k: string) => (typeof out[k] === 'string' ? (out[k] as string) : undefined)
      display[obj.objectId] = { name: str('name'), image: str('image_url') ?? str('img_url') ?? str('image') }
    }
    sim.display = display
  }
  if (metas) {
    sim.coins = {}
    for (const [t, m] of metas) if (m) sim.coins[t] = { decimals: m.decimals, symbol: m.symbol, name: m.name, iconUrl: m.iconUrl ?? null }
  }
  if (origs) {
    sim.packages = {}
    for (const [id, orig] of origs) if (orig) sim.packages[id] = normId(orig)
  }
}

/** Simulator backed by a Sui fullnode (gRPC-web). Simulates the exact bytes; never executes. */
export function createGrpcSimulator(opts: { client: SimClient; extrasMs?: number }): Simulator {
  const client = opts.client
  const extrasMs = opts.extrasMs ?? 2500
  return {
    async simulate(bytes) {
      let sim: Simulation
      try {
        sim = fromSdk(await client.simulateTransaction({ transaction: bytes, include: { balanceChanges: true, effects: true, objectTypes: true } }))
      } catch (e) {
        const code = (e as { code?: string; cause?: { code?: string } })?.code ?? (e as { cause?: { code?: string } })?.cause?.code
        if (code && REJECTED.has(code)) {
          const message = e instanceof Error ? e.message : String(e)
          return { source: 'node', status: 'failure', failure: { message, kind: 'Rejected' }, balanceChanges: [], objects: [], gas: { computation: '0', storage: '0', rebate: '0' }, gasOwner: null }
        }
        throw e
      }

      await addExtras(client, sim, bytes, extrasMs)
      return sim
    },
  }
}
