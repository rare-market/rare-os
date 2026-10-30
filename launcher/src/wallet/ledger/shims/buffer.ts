/**
 * Ledger's and Keystone's JS (hw-transport, ledgerjs-hw-app-sui, bc-ur-registry-sui, cbor-sync)
 * use Node's Buffer as a global, which WebKitGTK does not have. Import this module first in any
 * module that loads them; ES modules run their imports in order, so the global is in place before
 * those libraries run.
 */
import { Buffer } from 'buffer'

const g = globalThis as { Buffer?: unknown }
g.Buffer ??= Buffer

export { Buffer }
