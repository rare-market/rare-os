// The one answer to a wallet that cannot sign: a watch-only account or no account at all. A plain
// line and one button; the button opens Rare Wallet's passkey flow in a sheet over whatever is on
// screen (a game's Rare menu, an item page), and when an account lands the sheet closes and the
// page underneath carries on with it.

import { useState } from "react";
import { Button } from "../../ui";
import type { RareAccount } from "../index";
import { PasskeyFlow } from "./onboard/PasskeyFlow";
import { Btn, Sheet } from "./kit";
import { useWallet } from "./store";

export type Need = "sell" | "buy" | "trade" | "items";

const LINE: Record<Need, string> = {
  sell: "Add a passkey to sell.",
  buy: "Add a passkey to buy.",
  trade: "Add a passkey to trade.",
  items: "Sign in with a passkey to see your items.",
};

function AddPasskeySheet({ onClose }: { onClose: () => void }) {
  const [create, setCreate] = useState(false);
  return (
    <Sheet
      title={create ? "Create account" : "Add a passkey"}
      subtitle={create ? "Make a new wallet protected by a passkey." : "Use the passkey you made before."}
      onClose={onClose}
      width="40rem"
      footer={
        <Btn variant="ghost" onClick={() => setCreate((c) => !c)}>
          {create ? "Use a passkey I already have" : "Create account"}
        </Btn>
      }
    >
      <PasskeyFlow
        key={create ? "create" : "signin"}
        create={create}
        active
        onExit={onClose}
        onFinish={(address) => {
          void useWallet.getState().select(address).then(onClose);
        }}
      />
    </Sheet>
  );
}

/** Watch-only or no account: the line and its button. Nothing here for an account that can sign. */
export function AddPasskey({ need, account, className = "" }: { need: Need; account: Pick<RareAccount, "kind"> | null | undefined; className?: string }) {
  const [open, setOpen] = useState(false);
  const signedOut = !account;
  return (
    <div className={`flex flex-col items-start gap-[0.75rem] ${className}`}>
      <p className="text-small text-secondary">{signedOut && need !== "items" ? LINE[need].replace("Add a passkey to", "Sign in with a passkey to") : LINE[need]}</p>
      <Button variant="primary" size="md" hint={signedOut ? "Sign in" : "Add a passkey"} onClick={() => setOpen(true)}>
        {signedOut ? "Sign in" : "Add a passkey"}
      </Button>
      {open && <AddPasskeySheet onClose={() => setOpen(false)} />}
    </div>
  );
}
