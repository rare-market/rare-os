// The passkey page's door to the passkey module: the live ceremony, the phone or key choice, and
// the two account calls.

import { createPasskeyAccount, signInWithPasskey, usePasskeyCeremony, usePasskeyPrefs, type CeremonyState } from "../../passkey";

export type Ceremony = CeremonyState;
export { createPasskeyAccount, signInWithPasskey, usePasskeyPrefs };

/** Live state of the running ceremony (QR, status, PIN prompt, error). */
export function useCeremony(): { state: Ceremony; qrUrl: string | null } {
  const { state, qrUrl } = usePasskeyCeremony();
  return { state, qrUrl };
}
