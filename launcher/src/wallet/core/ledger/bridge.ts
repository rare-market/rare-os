import { verifyPersonalMessageSignature, verifyTransactionSignature } from '@mysten/sui/verify'
import { toBase64 } from '@mysten/sui/utils'
import type { AccountSigner } from '../keys/signer'
import { LEDGER_MESSAGES, LedgerError, type LedgerErrorCode } from './errors'
import { addressFromPublicKey } from './path'

/**
 * Background <-> page seam for Ledger signatures. The service worker holds no device: when a
 * Ledger account must sign it hands a job to an open extension page (toolbar popup, approval
 * window, side panel, ledger.html), which talks to the device over WebHID and answers with the
 * serialized signature. Each page keeps a port open while it lives and introduces itself with
 * its page id; popup requests carry that id, so a job goes back to the page that asked for it.
 */
export const LEDGER_PORT = 'rare.ledger.host'

export interface LedgerJob {
  id: string
  kind: 'transaction' | 'message'
  address: string
  /** account name, for the confirm screen */
  label?: string
  path: string
  /** base64 32-byte Ed25519 key */
  publicKey: string
  /** base64 tx bytes or message bytes */
  bytes: string
}

export type HostToBackground =
  | { type: 'hello'; host: string }
  | { type: 'ping' }
  | { type: 'done'; id: string; signature?: string; error?: { code: LedgerErrorCode; message: string } }
export type BackgroundToHost = { type: 'job'; job: LedgerJob } | { type: 'cancel'; id: string }

export interface PortLike {
  postMessage(m: unknown): void
  onMessage: { addListener(f: (m: unknown) => void): void }
  onDisconnect: { addListener(f: () => void): void }
}

/** Where a signature request came from: the page id of the popup window that asked, when known. */
export interface SignContext {
  host?: string
}

export type LedgerSignFn = (job: Omit<LedgerJob, 'id'>, ctx?: SignContext) => Promise<string>

/**
 * BACKGROUND: routes jobs to pages. `openHost` opens a window when no page is open (an
 * auto-approved dApp request with the popup closed); the window's page connects and gets the job.
 */
export function ledgerBroker(o: { openHost?: () => Promise<void>; hostWaitMs?: number; jobTimeoutMs?: number; newId?: () => string } = {}) {
  const hostWaitMs = o.hostWaitMs ?? 15_000
  const jobTimeoutMs = o.jobTimeoutMs ?? 10 * 60_000
  const newId = o.newId ?? (() => crypto.randomUUID())
  // most recent last
  const hosts: { id: string; port: PortLike }[] = []
  const waiters = new Set<() => void>()
  type Pending = { job: LedgerJob; port: PortLike; resolve: (s: string) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }
  const jobs = new Map<string, Pending>()

  function finish(id: string, r: { signature?: string; error?: Error }) {
    const p = jobs.get(id)
    if (!p) return
    jobs.delete(id)
    clearTimeout(p.timer)
    if (r.signature) p.resolve(r.signature)
    else p.reject(r.error ?? new LedgerError('unknown'))
  }

  function connect(port: PortLike) {
    port.onMessage.addListener((raw) => {
      const m = raw as HostToBackground
      if (!m || typeof m !== 'object') return
      if (m.type === 'hello' && typeof m.host === 'string') {
        const i = hosts.findIndex((h) => h.port === port)
        if (i >= 0) hosts.splice(i, 1)
        hosts.push({ id: m.host, port })
        for (const w of [...waiters]) w()
      } else if (m.type === 'done' && typeof m.id === 'string') {
        const p = jobs.get(m.id)
        if (!p || p.port !== port) return
        if (typeof m.signature === 'string' && m.signature) finish(m.id, { signature: m.signature })
        else finish(m.id, { error: new LedgerError(m.error?.code ?? 'unknown', m.error?.message || LEDGER_MESSAGES[m.error?.code ?? 'unknown']) })
      }
    })
    port.onDisconnect.addListener(() => {
      const i = hosts.findIndex((h) => h.port === port)
      if (i >= 0) hosts.splice(i, 1)
      for (const [id, p] of jobs) if (p.port === port) finish(id, { error: new LedgerError('no-window') })
    })
  }

  function pick(host?: string): PortLike | null {
    if (host) {
      const h = hosts.find((x) => x.id === host)
      if (h) return h.port
    }
    return hosts[hosts.length - 1]?.port ?? null
  }

  async function waitForHost(host?: string): Promise<PortLike> {
    const now = pick(host)
    if (now) return now
    if (!o.openHost) throw new LedgerError('no-window', 'Open Rare Wallet to sign with your Ledger.')
    const got = new Promise<PortLike | null>((resolve) => {
      const done = () => {
        const p = pick(host)
        if (!p) return
        waiters.delete(done)
        clearTimeout(t)
        resolve(p)
      }
      const t = setTimeout(() => (waiters.delete(done), resolve(null)), hostWaitMs)
      waiters.add(done)
    })
    await o.openHost().catch(() => {})
    const port = await got
    if (!port) throw new LedgerError('no-window', 'Open Rare Wallet to sign with your Ledger.')
    return port
  }

  const sign: LedgerSignFn = async (req, ctx) => {
    const port = await waitForHost(ctx?.host)
    const job: LedgerJob = { ...req, id: newId() }
    return new Promise<string>((resolve, reject) => {
      const timer = setTimeout(() => {
        port.postMessage({ type: 'cancel', id: job.id } satisfies BackgroundToHost)
        finish(job.id, { error: new LedgerError('timeout') })
      }, jobTimeoutMs)
      jobs.set(job.id, { job, port, resolve, reject, timer })
      try {
        port.postMessage({ type: 'job', job } satisfies BackgroundToHost)
      } catch {
        finish(job.id, { error: new LedgerError('no-window') })
      }
    })
  }

  return { connect, sign, pending: () => jobs.size, hosts: () => hosts.map((h) => h.id) }
}

/**
 * BACKGROUND: signs for a Ledger account through the broker. Holds only public data. Every
 * signature coming back from a page is verified against the stored key before it is used.
 */
export class LedgerSigner implements AccountSigner {
  readonly address: string
  constructor(
    private a: { publicKey: string; path: string; label?: string },
    private send: LedgerSignFn,
    private ctx?: SignContext,
  ) {
    this.address = addressFromPublicKey(a.publicKey)
  }
  async signTransaction(txBytes: Uint8Array) {
    const signature = await this.send(
      { kind: 'transaction', address: this.address, label: this.a.label, path: this.a.path, publicKey: this.a.publicKey, bytes: toBase64(txBytes) },
      this.ctx,
    )
    await verifyTransactionSignature(txBytes, signature, { address: this.address }).catch(() => {
      throw new LedgerError('mismatch', 'The signature from your Ledger does not match this account. Connect the Ledger you added it with.')
    })
    return signature
  }
  async signPersonalMessage(message: Uint8Array) {
    const signature = await this.send(
      { kind: 'message', address: this.address, label: this.a.label, path: this.a.path, publicKey: this.a.publicKey, bytes: toBase64(message) },
      this.ctx,
    )
    await verifyPersonalMessageSignature(message, signature, { address: this.address }).catch(() => {
      throw new LedgerError('mismatch', 'The signature from your Ledger does not match this account. Connect the Ledger you added it with.')
    })
    return { bytes: toBase64(message), signature }
  }
}
