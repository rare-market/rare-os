/**
 * Live Keystone state for the review sheet, plus the signing hash it shows. Kept apart from the
 * UR libraries so screens can import it without pulling them into the main bundle.
 */
import { create } from 'zustand'
import { blake2b } from '@noble/hashes/blake2.js'
import { bytesToHex } from '@noble/hashes/utils.js'
import { bcs } from '@mysten/sui/bcs'
import { messageWithIntent } from '@mysten/sui/cryptography'

/**
 * The 32 bytes a Sui Ed25519 key signs: blake2b-256(intent || data). For a personal message the
 * data is the BCS vector<u8> of the message. The Keystone shows this hash; the review sheet shows
 * the same one so the person can compare.
 */
export function suiSigningDigest(kind: 'transaction' | 'message', bytes: Uint8Array): Uint8Array {
  const payload =
    kind === 'transaction' ? messageWithIntent('TransactionData', bytes) : messageWithIntent('PersonalMessage', bcs.byteVector().serialize(bytes).toBytes())
  return blake2b(payload, { dkLen: 32 })
}

export const suiSigningDigestHex = (kind: 'transaction' | 'message', bytes: Uint8Array) => bytesToHex(suiSigningDigest(kind, bytes))

export type KeystoneStage = 'idle' | 'connecting' | 'unlock' | 'home-screen' | 'approve' | 'confirm' | 'done' | 'error'

export const KEYSTONE_STAGE_TEXT: Record<KeystoneStage, string> = {
  idle: '',
  connecting: 'Connecting to your Keystone',
  unlock: 'Unlock your Keystone',
  'home-screen': 'Go to the home screen on your Keystone',
  approve: 'Approve on your Keystone and enter its password',
  confirm: 'Confirm on your Keystone',
  done: 'Done',
  error: '',
}

export interface KeystonePromptState {
  stage: KeystoneStage
  message: string
  /** lowercase hex of the hash the Keystone shows while signing, else null */
  digestHex: string | null
  error: { code: string; message: string; fix?: string } | null
}

const IDLE: KeystonePromptState = { stage: 'idle', message: '', digestHex: null, error: null }

export const useKeystonePrompt = create<KeystonePromptState>(() => IDLE)

export function setKeystoneStage(stage: Exclude<KeystoneStage, 'error'>, digestHex?: string | null) {
  useKeystonePrompt.setState((s) => ({ stage, message: KEYSTONE_STAGE_TEXT[stage], digestHex: digestHex === undefined ? s.digestHex : digestHex, error: null }))
}

export function setKeystoneError(e: { code: string; message: string; fix?: string }) {
  useKeystonePrompt.setState({ stage: 'error', message: e.message, error: { code: e.code, message: e.message, ...(e.fix ? { fix: e.fix } : {}) } })
}

export function resetKeystonePrompt() {
  useKeystonePrompt.setState(IDLE)
}

let current: AbortController | null = null

export function beginKeystoneTask(): AbortSignal {
  current?.abort()
  current = new AbortController()
  return current.signal
}

/** Stops waiting. The Keystone may still show the request; rejecting it there clears it. */
export function cancelKeystone() {
  current?.abort()
  current = null
}
