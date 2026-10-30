// Rare Wallet first run: no accounts yet. A passkey is the main way in; everything else sits under
// "Other ways to sign in".

import { useWalletNav } from "../store";
import { Btn, Page } from "../kit";
import { H, useHints } from "../onboard/flow";
import { OtherWays } from "../onboard/OtherWays";

export function WelcomePage({ active }: { active: boolean }) {
  const push = useWalletNav.getState().push;
  useHints(active, [H.select]);
  return (
    <Page active={active} narrow title="Rare Wallet" subtitle="Hold your coins and collectibles, and play games that use them.">
      <div className="flex flex-col gap-3">
        <Btn variant="primary" def className="w-full" onClick={() => push({ kind: "passkey", mode: "signin" })}>
          Sign in with a passkey
        </Btn>
        <Btn className="w-full" onClick={() => push({ kind: "passkey", mode: "create" })}>
          Create account
        </Btn>
        <p className="text-center text-sm text-secondary">A passkey uses your phone or a security key. There are no words to write down.</p>
      </div>
      <h2 className="mb-2 mt-9 text-base font-semibold">Other ways to sign in</h2>
      <OtherWays onPick={push} />
      <div className="mt-6 flex justify-center">
        <Btn variant="ghost" size="md" onClick={() => push({ kind: "playtron" })}>
          Move your assets from your old SuiPlay wallet
        </Btn>
      </div>
    </Page>
  );
}
