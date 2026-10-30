// Add an account: the same ways as the welcome screen, plus a new account from a recovery phrase
// this device already holds.

import { FileText, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { keystore } from "../../index";
import { Btn, ListButton, Page, Panel } from "../kit";
import { requestUnlock } from "../unlock";
import { useWallet, useWalletNav, type WalletView } from "../store";
import { H, finishAdd, plainError, useHints } from "../onboard/flow";
import { OtherWays } from "../onboard/OtherWays";

export function AddAccountPage({ active }: { view: Extract<WalletView, { kind: "add" }>; active: boolean }) {
  const push = useWalletNav.getState().push;
  const accounts = useWallet((s) => s.accounts);
  const [hasPhrase, setHasPhrase] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useHints(active, [H.select, H.back]);

  useEffect(() => {
    void (async () => {
      const list = await keystore.listAccounts();
      const flags = await Promise.all(list.map((a) => keystore.hasPhrase(a.address).catch(() => false)));
      setHasPhrase(flags.some(Boolean));
    })();
  }, [accounts]);

  const derive = async () => {
    setBusy(true);
    setError("");
    try {
      if (!(await requestUnlock("Enter your PIN to add an account"))) return;
      const a = await keystore.deriveNext();
      await finishAdd(a.address);
    } catch (e) {
      setError(plainError(e, "Could not add the account"));
    } finally {
      setBusy(false);
    }
  };

  const none = !accounts?.length;
  return (
    <Page active={active} narrow title={none ? "Sign in" : "Add an account"} subtitle={none ? undefined : "Pick how you want to add it."}>
      <div className="flex flex-col gap-3">
        <Btn variant="primary" def className="w-full" onClick={() => push({ kind: "passkey", mode: "signin" })}>
          Sign in with a passkey
        </Btn>
        <Btn className="w-full" onClick={() => push({ kind: "passkey", mode: "create" })}>
          Create account
        </Btn>
      </div>
      <h2 className="mb-2 mt-8 text-base font-semibold">{none ? "Other ways to sign in" : "Other ways to add"}</h2>
      <div className="flex flex-col gap-3">
        {hasPhrase && (
          <Panel className="p-1">
            <ListButton
              icon={<FileText size="1.4rem" />}
              title="Add an account from your recovery phrase"
              body="The next account of the phrase already on this device"
              right={busy ? "Adding" : undefined}
              onClick={() => void derive()}
            />
          </Panel>
        )}
        <Panel className="p-1">
          <ListButton icon={<Plus size="1.4rem" />} title="Create a new recovery phrase" body="12 or 24 words you keep safe yourself" onClick={() => push({ kind: "create-phrase" })} />
        </Panel>
        <OtherWays onPick={push} />
      </div>
      {error && <p className="mt-4 text-sm text-negative">{error}</p>}
      {none && (
        <div className="mt-6 flex justify-center">
          <Btn variant="ghost" size="md" onClick={() => push({ kind: "playtron" })}>
            Move your assets from your old SuiPlay wallet
          </Btn>
        </div>
      )}
    </Page>
  );
}
