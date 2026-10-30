// "<game> wants to see your address": the web-game connect prompt. Pick which account the game
// sees (the selected one by default); Allow remembers the game's origin.

import { Check } from "lucide-react";
import { useState } from "react";
import type { RareAccount } from "../../index";
import { Addr, Btn, ListButton, Sheet, WalletAvatar } from "../kit";
import { hostOf, siteRisks } from "./site";

export function ConnectSheet({
  game,
  origin,
  accounts,
  initial,
  onAllow,
  onDeny,
}: {
  game: string;
  origin: string;
  accounts: RareAccount[];
  initial?: string;
  onAllow: (address: string) => void;
  onDeny: () => void;
}) {
  const [pick, setPick] = useState(initial && accounts.some((a) => a.address === initial) ? initial : accounts[0]?.address);
  const warn = siteRisks(origin);
  return (
    <Sheet
      title={`${game} wants to see your address`}
      subtitle={hostOf(origin)}
      footer={
        <>
          <Btn variant="primary" def className="flex-1" onClick={() => pick && onAllow(pick)}>
            Allow
          </Btn>
          <Btn onClick={onDeny}>Don't allow</Btn>
        </>
      }
      onClose={onDeny}
      width="36rem"
    >
      <p className="text-sm text-secondary">It can also ask you to sign. Every request still shows up here for you to approve, and nothing is signed without it.</p>
      {warn.length > 0 && (
        <ul className="mt-3 flex flex-col gap-1 text-sm text-negative" role="alert">
          {warn.map((w) => (
            <li key={w.id}>{w.text}</li>
          ))}
        </ul>
      )}
      <h3 className="mb-2 mt-5 text-sm text-secondary">Account to share</h3>
      <div className="rounded border border-border-primary p-1">
        {accounts.map((a) => (
          <ListButton
            key={a.address}
            icon={<WalletAvatar address={a.address} size="2rem" />}
            title={a.label}
            body={<Addr address={a.address} />}
            right={a.address === pick ? <Check size="1.25rem" className="text-primary" /> : undefined}
            onClick={() => setPick(a.address)}
          />
        ))}
      </div>
    </Sheet>
  );
}
