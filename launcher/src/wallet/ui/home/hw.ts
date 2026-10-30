// Hardware wallet helpers for the UI. The Ledger module pulls in the device libraries, so it is
// loaded only when a button needs it.

export interface LedgerCheck {
  state: "ok" | "different" | "error";
  message?: string;
  /** one-line permission fix when Linux blocks the device */
  fix?: string;
}

/** Account index from a Sui BIP32 path like m/44'/784'/2'/0'/0'. */
export const ledgerIndexOf = (path: string | undefined): number => {
  const m = path?.match(/784'\/(\d+)'/);
  return m ? Number(m[1]) : 0;
};

/** Shows the account's address on the first Ledger found and compares it with the wallet's. */
export async function verifyOnLedger(address: string, path: string | undefined): Promise<LedgerCheck> {
  const L = await import("../../ledger");
  try {
    const devices = await L.listLedgers();
    if (!devices.length) return { state: "error", message: "No Ledger found. Plug it in, unlock it and open the Sui app." };
    await L.connectLedger(devices[0].id);
    const shown = await L.verifyLedgerAddress(ledgerIndexOf(path));
    return { state: shown.address === address ? "ok" : "different" };
  } catch (e) {
    const le = L.toLedgerError(e);
    return { state: "error", message: le.message, fix: le.fix };
  } finally {
    L.resetLedgerPrompt();
  }
}

/** Looks for a Ledger and reports what is in the way (Settings help). */
export async function checkLedger(): Promise<LedgerCheck & { found?: string }> {
  const L = await import("../../ledger");
  try {
    const devices = await L.listLedgers();
    if (!devices.length) return { state: "error", message: "No Ledger found. Plug it in and unlock it." };
    return { state: "ok", found: devices[0].name };
  } catch (e) {
    const le = L.toLedgerError(e);
    return { state: "error", message: le.message, fix: le.fix };
  }
}
