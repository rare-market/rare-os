/**
 * Stand-in for @ledgerhq/ledger-cal-service (aliased in vite.config.ts). The Sui Ledger app client
 * only calls it for token "resolution" (Ledger's signed token metadata), which Rare Wallet never
 * requests. The real package pulls in axios with a top-level require('https') that throws in a webview.
 */
const unavailable = async (): Promise<never> => {
  throw new Error('Ledger token metadata is not used by Rare Wallet')
}
export default { getCertificate: unavailable, findToken: unavailable }
export const getCertificate = unavailable
export const findToken = unavailable
