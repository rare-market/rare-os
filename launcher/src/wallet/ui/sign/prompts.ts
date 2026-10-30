// Live device state for the review sheet: the Ledger and Keystone prompt stores and the passkey
// ceremony, behind one import. The dev override lets screenshots show a waiting state without a
// device (browser only).

import { create } from "zustand";
import { ceremonyStore, usePasskeyCeremony, type CeremonyState } from "../../passkey";
import { useKeystonePrompt } from "../../keystone/prompt";

export { useLedgerPrompt, cancelLedger } from "../../ledger/prompt";
export { useKeystonePrompt, cancelKeystone } from "../../keystone/prompt";

/** Current passkey phone/key ceremony state. */
export const usePasskeyState = (): CeremonyState => usePasskeyCeremony().state;

/** Dev only (browser): pretend the account is this kind so the matching waiting panel shows. */
export const useDevHw = create<{ force: null | "ledger" | "keystone" | "passkey" }>(() => ({ force: null }));

export const devSetKeystone = (s: Partial<ReturnType<typeof useKeystonePrompt.getState>>) => useKeystonePrompt.setState(s);
export const devSetPasskey = (s: CeremonyState) => ceremonyStore.setState({ state: s, qrUrl: s.kind === "qr" ? s.url : null });
export type { CeremonyState };

