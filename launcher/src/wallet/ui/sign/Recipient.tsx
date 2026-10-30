// Recipient pieces shared by Send and SendNfts: the resolved-name line under the field, the
// lookup hook, and the quick picks (own accounts, recent recipients).

import { useEffect, useState } from "react";
import { resolveRecipient, shortAddress, type RecipientState } from "../../tx";
import type { RareAccount } from "../../index";
import { Addr, ListButton, WalletAvatar, cn } from "../kit";
import { recents, since } from "./recents";

/** Resolves the typed text (address or .sui name) into a field state as it changes. */
export function useRecipient(to: string): RecipientState {
  const [rs, setRs] = useState<RecipientState>({ s: "empty" });
  useEffect(() => {
    if (!to.trim()) return setRs({ s: "empty" });
    let gone = false;
    setRs({ s: "typing" });
    void resolveRecipient(to).then((r) => !gone && setRs(r));
    return () => {
      gone = true;
    };
  }, [to]);
  return rs;
}

export function RecipientPicks({ accounts, current, filter, onPick }: { accounts: RareAccount[]; current?: string; filter: string; onPick: (address: string) => void }) {
  const picks = accounts.filter((a) => a.address !== current);
  const q = filter.trim().toLowerCase();
  const list = recents().filter((r) => !q || r.address.includes(q) || r.suins?.includes(q));
  if (!picks.length && !list.length) return null;
  return (
    <div className="min-h-0">
      {picks.length > 0 && (
        <>
          <h3 className="mb-1 text-sm text-secondary">Your accounts</h3>
          {picks.map((a) => (
            <ListButton key={a.address} icon={<WalletAvatar address={a.address} size="2rem" />} title={a.label} body={<Addr address={a.address} />} onClick={() => onPick(a.address)} />
          ))}
        </>
      )}
      {list.length > 0 && (
        <>
          <h3 className="mb-1 mt-3 text-sm text-secondary">Recent</h3>
          {list.slice(0, 4).map((r) => (
            <ListButton
              key={r.address}
              icon={<WalletAvatar address={r.address} size="2rem" />}
              title={r.suins ?? shortAddress(r.address)}
              body={r.suins ? <Addr address={r.address} /> : undefined}
              right={since(r.at)}
              onClick={() => onPick(r.address)}
            />
          ))}
        </>
      )}
    </div>
  );
}

export function RecipientLine({ rs, self }: { rs: RecipientState; self: boolean }) {
  if (rs.s === "ready")
    return (
      <p className={cn("mt-1.5 flex items-center gap-2 text-sm", self ? "text-negative" : "text-secondary")}>
        {self ? (
          "Pick an address other than this account"
        ) : (
          <>
            <WalletAvatar address={rs.address} size="1.25rem" />
            {rs.suins && <span className="text-primary">{rs.suins}</span>}
            <Addr address={rs.address} />
          </>
        )}
      </p>
    );
  if (rs.s === "loading") return <p className="mt-1.5 text-sm text-secondary">Looking up {rs.name}</p>;
  if (rs.s === "invalid" || rs.s === "missing" || rs.s === "error")
    return (
      <p className="mt-1.5 text-sm text-negative" role="alert">
        {rs.msg}
      </p>
    );
  return null;
}
