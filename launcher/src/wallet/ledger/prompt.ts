/**
 * Live Ledger state for the review sheet while a request waits on the device. Kept apart from
 * the Ledger libraries so screens can import it without pulling them into the main bundle.
 */
import { create } from 'zustand'

export type LedgerStage = 'idle' | 'connecting' | 'unlock' | 'open-app' | 'confirm' | 'verify' | 'done' | 'error'

export const LEDGER_STAGE_TEXT: Record<LedgerStage, string> = {
  idle: '',
  connecting: 'Connecting to your Ledger',
  unlock: 'Unlock your Ledger',
  'open-app': 'Open the Sui app on your Ledger',
  confirm: 'Confirm on your Ledger',
  verify: 'Check the address on your Ledger',
  done: 'Done',
  error: '',
}

export interface LedgerPromptError {
  code: string
  message: string
  /** "udev": the one-line command that gives Rare OS access to the Ledger */
  fix?: string
}

export interface LedgerPromptState {
  stage: LedgerStage
  /** what to show: the stage text, or the error message */
  message: string
  /** name of the Ledger in use ("Nano X 4F2A") */
  device: string | null
  error: LedgerPromptError | null
}

const IDLE: LedgerPromptState = { stage: 'idle', message: '', device: null, error: null }

export const useLedgerPrompt = create<LedgerPromptState>(() => IDLE)

export function setLedgerStage(stage: Exclude<LedgerStage, 'error'>, device?: string | null) {
  useLedgerPrompt.setState((s) => ({ stage, message: LEDGER_STAGE_TEXT[stage], device: device === undefined ? s.device : device, error: null }))
}

export function setLedgerError(e: { code: string; message: string; fix?: string }) {
  useLedgerPrompt.setState({ stage: 'error', message: e.message, error: { code: e.code, message: e.message, ...(e.fix ? { fix: e.fix } : {}) } })
}

export function resetLedgerPrompt() {
  useLedgerPrompt.setState(IDLE)
}

let current: AbortController | null = null

/** Starts a new device task (cancelling any earlier one) and returns its signal. */
export function beginLedgerTask(): AbortSignal {
  current?.abort()
  current = new AbortController()
  return current.signal
}

/** Cancel button on the review sheet: stops waiting. The device may still show the request. */
export function cancelLedger() {
  current?.abort()
  current = null
}
