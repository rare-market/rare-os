/**
 * Ledger failures as a small set of codes with plain messages. Errors come from three places:
 * the device (status words), @ledgerhq transports (named errors), and WebHID (DOMExceptions).
 * Several copies of @ledgerhq/errors can be bundled, so everything is matched by shape, never
 * by instanceof.
 */
export type LedgerErrorCode =
  | 'no-device'
  | 'locked'
  | 'app-closed'
  | 'wrong-app'
  | 'rejected'
  | 'blind-signing'
  | 'disconnected'
  | 'busy'
  | 'mismatch'
  | 'timeout'
  | 'cancelled'
  | 'update-app'
  | 'unsupported-browser'
  | 'no-window'
  | 'unknown'

export const LEDGER_MESSAGES: Record<LedgerErrorCode, string> = {
  'no-device': 'No Ledger found. Connect it, unlock it and open the Sui app.',
  locked: 'Your Ledger is locked. Unlock it with your PIN and try again.',
  'app-closed': 'Open the Sui app on your Ledger and try again.',
  'wrong-app': 'Another app is open on your Ledger. Close it, open the Sui app and try again.',
  rejected: 'Rejected on your Ledger.',
  'blind-signing':
    "Your Ledger can't show this transaction in full, so it needs blind signing. On your Ledger, open the Sui app, go to Settings and turn on Blind signing. Then try again.",
  disconnected: 'Your Ledger disconnected. Reconnect it and try again.',
  busy: 'Your Ledger is busy with another request. Finish or reject it on the device, close Ledger Live or other wallets using it, then try again.',
  mismatch: 'This Ledger does not hold this account. Connect the Ledger you added it with.',
  timeout: 'Your Ledger did not answer in time. Try again.',
  cancelled: 'Cancelled. If your Ledger still shows the request, reject it there.',
  'update-app': 'Update the Sui app in Ledger Live and try again.',
  'unsupported-browser': "This browser can't reach a Ledger. Connect it over USB-C.",
  'no-window': 'Rare Wallet closed before your Ledger answered. Try again and keep the window open.',
  unknown: 'Your Ledger did not respond. Reconnect it and try again.',
}

export class LedgerError extends Error {
  readonly code: LedgerErrorCode
  /** status word from the device, when there was one */
  readonly statusCode?: number
  constructor(code: LedgerErrorCode, message?: string, statusCode?: number) {
    super(message ?? LEDGER_MESSAGES[code])
    this.name = 'LedgerError'
    this.code = code
    this.statusCode = statusCode
  }
}

export const isLedgerError = (e: unknown): e is LedgerError =>
  !!e && typeof e === 'object' && (e as { name?: string }).name === 'LedgerError' && typeof (e as { code?: unknown }).code === 'string'

/** Status words. The Sui app is a Rust (ledger_device_sdk) app; the dashboard and other apps answer with BOLOS codes. */
const SW = {
  LOCKED: [0x5515, 0x6b0c, 0x6982],
  // 0x6985: CONDITIONS_OF_USE_NOT_SATISFIED (C apps), 0x6e04: StatusWords::UserCancelled (Rust SDK)
  REJECTED: [0x6985, 0x6e04],
  // SyscallError::NotSupported: the Sui app rejects a transaction it can't clear sign while blind signing is off
  BLIND: [0x6808],
  // CLA/INS not supported: the dashboard or another app is open. 0x6511/0x6a15/0x6807: app not open or missing
  APP_CLOSED: [0x6e00, 0x6e01, 0x6d00, 0x6d02, 0x6511, 0x6a15, 0x6807, 0x6a83],
}

/** Map anything thrown while talking to a Ledger to a LedgerError with a plain message. */
export function toLedgerError(e: unknown): LedgerError {
  if (isLedgerError(e)) return e
  const o = (e && typeof e === 'object' ? e : {}) as { name?: string; id?: string; message?: string; statusCode?: unknown }
  const name = o.name ?? ''
  const id = o.id ?? ''
  const msg = typeof o.message === 'string' ? o.message : typeof e === 'string' ? e : ''
  const sw = typeof o.statusCode === 'number' ? o.statusCode : undefined

  if (sw !== undefined) {
    if (SW.LOCKED.includes(sw)) return new LedgerError('locked', undefined, sw)
    if (SW.REJECTED.includes(sw)) return new LedgerError('rejected', undefined, sw)
    if (SW.BLIND.includes(sw)) return new LedgerError('blind-signing', undefined, sw)
    if (SW.APP_CLOSED.includes(sw)) return new LedgerError('app-closed', undefined, sw)
    return new LedgerError('unknown', `Your Ledger returned an error (0x${sw.toString(16)}). Check the Sui app is open and up to date, then try again.`, sw)
  }
  if (name === 'LockedDeviceError') return new LedgerError('locked')
  if (name === 'UpdateYourApp') return new LedgerError('update-app')
  if (name === 'LatestFirmwareVersionRequired') return new LedgerError('update-app', 'Update your Ledger in Ledger Live and try again.')
  if (name === 'TransportRaceCondition' || id === 'TransportLocked' || name === 'TransportLocked') return new LedgerError('busy')
  if (name === 'DisconnectedDevice' || name === 'DisconnectedDeviceDuringOperation') return new LedgerError('disconnected')
  if (name === 'TransportOpenUserCancelled') return new LedgerError('no-device')
  if (id === 'HIDNotSupported') return new LedgerError('unsupported-browser')
  // WebHID: another tab or app (Ledger Live) holds the device, or it vanished mid-open
  if (name === 'InvalidStateError' || (name === 'NotAllowedError' && /open/i.test(msg))) return new LedgerError('busy')
  if (name === 'NotFoundError') return new LedgerError('no-device')
  if (/disconnect|write|transferOut|transferIn/i.test(msg)) return new LedgerError('disconnected')
  return new LedgerError('unknown')
}

/** The message to show for any Ledger failure. */
export const ledgerErrorMessage = (e: unknown) => toLedgerError(e).message
