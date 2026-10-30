// Browser dev and screenshots only: ?devphrase makes a test recovery phrase account in the browser
// vault (PIN 123456) so the reveal screens can be shown. Never runs inside the app.

import { isTauri } from "../../native";
import { keystore } from "../../index";
import { useWallet } from "../store";

if (!isTauri && typeof location !== "undefined" && new URLSearchParams(location.search).has("devphrase")) {
  void (async () => {
    try {
      const accounts = await keystore.listAccounts();
      if (!accounts.some((a) => a.kind === "native")) {
        const a = await keystore.addPhrase(undefined, { label: "Test phrase", pin: { kind: "pin", secret: "123456" } });
        await keystore.select(a.address);
      }
      await useWallet.getState().refresh();
    } catch {
      /* the vault may already exist with another PIN */
    }
  })();
}
