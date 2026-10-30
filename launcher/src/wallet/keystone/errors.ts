/**
 * Keystone failures as a small set of codes with plain messages. Errors come from the Rust USB
 * transport (NativeError codes: udev, notfound, locked, rejected, timeout, usb-off) and from the
 * device's own answers, which only carry text (e.g. "Export address is just allowed on specific
 * pages"), so those are matched by message.
 */
export type KeystoneErrorCode =
  | 'no-device'
  | 'locked'
  | 'rejected'
  | 'timeout'
  | 'usb-off'
  | 'udev'
  | 'home-screen'
  | 'busy'
  | 'disconnected'
  | 'mismatch'
  | 'unsupported'
  | 'cancelled'
  | 'bad-response'
  | 'unknown'

export const KEYSTONE_MESSAGES: Record<KeystoneErrorCode, string> = {
  'no-device': 'No Keystone found. Connect it with USB-C and unlock it.',
  locked: 'Your Keystone is locked. Unlock it and try again.',
  rejected: 'Rejected on your Keystone.',
  timeout: 'Your Keystone did not answer in time. Try again.',
  // the firmware refuses requests away from its home screen; Rust reports that as "usb-off"
  'usb-off': 'Go to the home screen on your Keystone. If USB is off, turn it on in Settings > Connection.',
  udev: 'Rare OS needs permission to use your Keystone. Run this command once in a terminal, then reconnect your Keystone.',
  'home-screen': 'Go to the home screen on your Keystone and try again.',
  busy: 'Your Keystone is busy with another request. Finish or reject it on the device, then try again.',
  disconnected: 'Your Keystone disconnected. Reconnect it and try again.',
  mismatch: 'This Keystone does not hold this account. Connect the Keystone you added it with.',
  unsupported: 'Your Keystone cannot do this yet. Update it to the latest multi-coin firmware and try again.',
  cancelled: 'Cancelled. If your Keystone still shows the request, reject it there.',
  'bad-response': 'Your Keystone sent an answer Rare Wallet could not read. Update its firmware and try again.',
  unknown: 'Your Keystone did not respond. Reconnect it and try again.',
}

export class KeystoneError extends Error {
  readonly code: KeystoneErrorCode
  /** extra data from the Rust side; for "udev", `fix` is the one-line command to show */
  readonly data?: Record<string, unknown>
  constructor(code: KeystoneErrorCode, message?: string, data?: Record<string, unknown>) {
    super(message ?? KEYSTONE_MESSAGES[code])
    this.name = 'KeystoneError'
    this.code = code
    this.data = data
  }
  get fix(): string | undefined {
    return typeof this.data?.fix === 'string' ? this.data.fix : undefined
  }
}

export const isKeystoneError = (e: unknown): e is KeystoneError =>
  !!e && typeof e === 'object' && (e as { name?: string }).name === 'KeystoneError' && typeof (e as { code?: unknown }).code === 'string'

const NATIVE: Record<string, KeystoneErrorCode> = {
  udev: 'udev',
  notfound: 'no-device',
  locked: 'locked',
  rejected: 'rejected',
  timeout: 'timeout',
  'usb-off': 'usb-off',
  busy: 'busy',
  disconnected: 'disconnected',
  mismatch: 'mismatch',
  unsupported: 'unsupported',
}

/** Device answers (firmware src/webusb_protocol/general/eapdu_services/service_resolve_ur.c and the Rust errors). */
function specificText(msg: string): KeystoneErrorCode | null {
  if (/specific pages/i.test(msg)) return 'home-screen'
  if (/waiting for user approval/i.test(msg)) return 'busy'
  if (/view type is not supported|not ?support/i.test(msg)) return 'unsupported'
  if (/fingerprint/i.test(msg)) return 'mismatch'
  return null
}
function fromDeviceText(msg: string): KeystoneErrorCode | null {
  return specificText(msg) ?? (/reject/i.test(msg) ? 'rejected' : /locked/i.test(msg) ? 'locked' : null)
}

export function toKeystoneError(e: unknown): KeystoneError {
  if (isKeystoneError(e)) return e
  const o = (e && typeof e === 'object' ? e : {}) as { name?: string; message?: string; code?: unknown; data?: unknown }
  const msg = typeof o.message === 'string' ? o.message : typeof e === 'string' ? e : ''
  if (o.name === 'NativeError' && typeof o.code === 'string') {
    const data = o.data && typeof o.data === 'object' ? (o.data as Record<string, unknown>) : undefined
    // the device's text is more precise than the transport's code (e.g. a refusal that means "go home")
    const code = o.code === 'udev' ? 'udev' : (specificText(msg) ?? NATIVE[o.code] ?? fromDeviceText(msg) ?? 'unknown')
    if (code === 'rejected' && /password/i.test(msg)) return new KeystoneError(code, 'Wrong password on your Keystone. Try again.', data)
    return new KeystoneError(code, undefined, data)
  }
  return new KeystoneError(fromDeviceText(msg) ?? 'unknown')
}

export const keystoneErrorMessage = (e: unknown) => toKeystoneError(e).message
