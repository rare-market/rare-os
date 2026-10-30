/**
 * Device conversations: the
 * Rust side lists and opens the Ledger (USB HID or Bluetooth) and carries APDUs. What stays: one
 * conversation at a time, the Sui app check before every conversation, ending a conversation when
 * the device disconnects, and timeouts/cancel for steps that wait on the person.
 */
import './shims/buffer'
import Sui from '@mysten/ledgerjs-hw-app-sui'
import type Transport from '@ledgerhq/hw-transport'
import { ledgerNative, type LedgerDevice, type LedgerNative } from '../native'
import { LedgerError, toLedgerError } from './errors'
import { assertSuiApp, parseAppAndVersion, type SuiLedgerApp } from './sign'
import { NativeLedgerTransport } from './transport'

export interface LedgerSession {
  app: SuiLedgerApp
  transport: NativeLedgerTransport
  device: LedgerDevice
  /** Sui app version from GET_APP_AND_VERSION, when the device answered it */
  appVersion?: string
}

// one device conversation at a time; a cancelled one may still wait on the device
let inflight: Promise<unknown> | null = null

/**
 * Read which app is open; throws a LedgerError unless it is Sui. Returns the app name and version,
 * or null when the device does not answer GET_APP_AND_VERSION (older firmware: the Sui calls
 * themselves then report a closed app).
 */
export async function checkSuiApp(transport: Pick<Transport, 'send'>): Promise<{ name: string; version: string } | null> {
  let info: { name: string; version: string }
  try {
    const res = await transport.send(0xb0, 0x01, 0x00, 0x00)
    info = parseAppAndVersion(new Uint8Array(res.subarray(0, res.length - 2)))
  } catch (e) {
    const le = toLedgerError(e)
    if (le.code === 'locked' || le.code === 'disconnected' || le.code === 'busy' || le.code === 'udev' || le.code === 'no-device') throw le
    return null
  }
  assertSuiApp(info)
  return info
}

/**
 * Open the Ledger `id` (from ledgerNative.list), check the Sui app is open, run `fn`, close.
 * A second conversation waits briefly for the first, then reports busy.
 */
export async function withLedger<T>(
  id: string,
  fn: (s: LedgerSession) => Promise<T>,
  o: { port?: LedgerNative; skipAppCheck?: boolean } = {},
): Promise<T> {
  const port = o.port ?? ledgerNative
  if (inflight) {
    const done = await Promise.race([inflight.then(() => true), new Promise<boolean>((r) => setTimeout(() => r(false), 1500))])
    if (!done) throw new LedgerError('busy', 'Your Ledger is still showing the last request. Reject it on the device, then try again.')
  }
  // `idle` settles once the device has really finished (a cancelled call may still be waiting on it)
  let release!: () => void
  const idle = new Promise<void>((r) => (release = r))
  inflight = idle
  void idle.then(() => {
    if (inflight === idle) inflight = null
  })
  let transport: NativeLedgerTransport | null = null
  try {
    let device: LedgerDevice
    try {
      device = await port.open(id)
    } catch (e) {
      throw toLedgerError(e)
    }
    const t = (transport = new NativeLedgerTransport(device, port))
    // a pending exchange may never settle when the device goes away: end the conversation on disconnect
    const gone = new Promise<never>((_, reject) => t.on('disconnect', () => reject(new LedgerError('disconnected'))))
    gone.catch(() => {})
    const info = o.skipAppCheck ? null : await Promise.race([checkSuiApp(t), gone])
    const session: LedgerSession = { app: new Sui(t) as unknown as SuiLedgerApp, transport: t, device, appVersion: info?.version }
    return await Promise.race([fn(session), gone])
  } catch (e) {
    throw toLedgerError(e)
  } finally {
    // close waits for any exchange still on the device, then frees the device for the next request
    if (transport) void transport.close().catch(() => {}).finally(release)
    else release()
  }
}

/**
 * Race a device step against a timeout and a cancel signal. The device can't be interrupted from
 * the host, so on cancel or timeout the call is left to finish (or be rejected on the device) and
 * the wallet moves on; the next request waits for it or reports busy.
 */
export function deviceStep<T>(p: Promise<T>, o: { timeoutMs: number; signal?: AbortSignal }): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const t = setTimeout(() => reject(new LedgerError('timeout')), o.timeoutMs)
    const onAbort = () => (clearTimeout(t), reject(new LedgerError('cancelled')))
    if (o.signal?.aborted) return onAbort()
    o.signal?.addEventListener('abort', onAbort, { once: true })
    p.then(
      (v) => (clearTimeout(t), o.signal?.removeEventListener('abort', onAbort), resolve(v)),
      (e) => (clearTimeout(t), o.signal?.removeEventListener('abort', onAbort), reject(toLedgerError(e))),
    )
  })
}
