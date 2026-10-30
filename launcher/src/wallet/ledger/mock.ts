/**
 * Browser stand-in for the Ledger (native.ts with `?mockhw`): the fake Sui app from _testkit with
 * the fixed test phrase, so screenshots show real addresses and a sign flow completes. It waits a
 * moment on prompts so "Confirm on your Ledger" is visible. Never used in the app on the device.
 */
import { Buffer } from './shims/buffer'
import { FakeLedgerTransport } from './_testkit'

let fake: FakeLedgerTransport | null = null

/** The simulated device, for tests that want to lock it, reject, or switch apps. */
export function mockLedgerDevice(): FakeLedgerTransport {
  return (fake ??= new FakeLedgerTransport({ promptDelayMs: 1200 }))
}

export async function mockLedgerExchange(apdu: Uint8Array): Promise<Uint8Array> {
  const res = await mockLedgerDevice().exchange(Buffer.from(apdu))
  return new Uint8Array(res)
}
