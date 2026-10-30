// Send collectibles: the picked items (art and names from rare.market), the recipient, Review.
// Items the chain won't let move (soulbound, locked by a policy) are listed with the reason and
// left out. Large picks go as several transactions; each one gets its own review.

import { useEffect, useMemo, useState } from "react";
import { MImg } from "../../../marketplace/MImg";
import { load, paths } from "../../../marketplace/api";
import type { ItemCard } from "../../../lib/rare";
import { SignRejected, signAndExecute } from "../../sign";
import { buildNftSend, nftSendability, planNftSend, type NftRef, type Sendability } from "../../tx";
import { canSignAccount } from "../dev";
import { useHints } from "../home/lib";
import { Btn, Field, Page, Panel, cn } from "../kit";
import { RecipientLine, RecipientPicks, useRecipient } from "../sign/Recipient";
import { noteSentItems } from "../../../inventory";
import { noteSent } from "../sign/recents";
import { useWallet, useWalletNav, type WalletView } from "../store";

export function SendNftsPage({ view, active }: { view: Extract<WalletView, { kind: "send-nfts" }>; active: boolean }) {
  const account = useWallet((s) => s.current);
  const accounts = useWallet((s) => s.accounts) ?? [];
  const [cards, setCards] = useState<ItemCard[] | null>(null);
  const [can, setCan] = useState<Map<string, Sendability> | null>(null);
  const [to, setTo] = useState(view.to ?? "");
  const rs = useRecipient(to);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const key = view.ids.join(",");
  useEffect(() => {
    let gone = false;
    setCards(null);
    void Promise.all(view.ids.map((id) => load<ItemCard>(paths.item(id)).catch(() => null))).then((r) => !gone && setCards(r.filter((c): c is ItemCard => !!c)));
    return () => {
      gone = true;
    };
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!cards || !account) return;
    let gone = false;
    const refs: NftRef[] = cards.map((c) => ({ objectId: c.id, type: c.type, kiosk: !!c.kioskId }));
    void nftSendability(account.address, refs)
      .then((m) => !gone && setCan(m))
      .catch(() => !gone && setCan(new Map()));
    return () => {
      gone = true;
    };
  }, [cards, account?.address]); // eslint-disable-line react-hooks/exhaustive-deps

  const status = (c: ItemCard): Sendability | undefined => can?.get(c.id);
  const sendable = useMemo(() => (cards ?? []).flatMap((c) => {
    const s = can?.get(c.id);
    return s?.ok ? [{ card: c, send: s.send }] : [];
  }), [cards, can]);

  const ready = rs.s === "ready" ? rs : null;
  const self = !!ready && !!account && ready.address === account.address.toLowerCase();
  const canSign = canSignAccount(account);
  const checking = !can;
  let cta = "Review";
  if (checking) cta = "Checking items";
  else if (!sendable.length) cta = "Nothing here can be sent";
  else if (!ready) cta = rs.s === "empty" ? "Enter a recipient" : rs.s === "loading" ? "Looking up name" : "Check the recipient";
  else if (self) cta = "Pick another address";
  const canReview = canSign && !checking && sendable.length > 0 && !!ready && !self && !busy;

  const submit = async () => {
    if (!canReview || !account || !ready) return;
    setBusy(true);
    setError(null);
    try {
      const batches = planNftSend({ sender: account.address, to: ready.address, items: sendable.map((s) => s.send) });
      for (let i = 0; i < batches.length; i++) {
        const tx = buildNftSend({ sender: account.address, to: ready.address, items: batches[i] });
        const names = batches[i].length === 1 ? sendable.find((s) => s.send === batches[i][0])?.card.name : null;
        const title = names ? `Send ${names}` : `Send ${batches[i].length} collectibles${batches.length > 1 ? ` (${i + 1} of ${batches.length})` : ""}`;
        await signAndExecute(tx, { account: account.address, title });
      }
      noteSent(ready.address, ready.suins);
      noteSentItems(sendable.map((s) => s.card.id));
      useWallet.getState().bump();
      useWalletNav.getState().reset();
    } catch (e) {
      if (!(e instanceof SignRejected)) setError(e instanceof Error ? e.message : "Could not send");
    } finally {
      setBusy(false);
    }
  };

  useHints(active, [{ b: "a", label: "Select" }, { b: "b", label: "Back" }]);

  if (!account) return null;
  const n = cards?.length ?? view.ids.length;
  return (
    <Page active={active} ready={`${account.address}${cards ? 1 : 0}`} title="Send collectibles" subtitle={`${n} ${n === 1 ? "item" : "items"} from ${account.label}`}>
      {!canSign && (
        <Panel className="mx-auto mb-6 max-w-[64rem] px-5 py-4">
          <p className="text-base font-semibold">This account is watch only</p>
          <p className="mt-1 text-sm text-secondary">Add an account that can sign to send from this wallet.</p>
        </Panel>
      )}
      <div className="mx-auto grid max-w-[64rem] grid-cols-[minmax(0,1fr)_minmax(0,22rem)] gap-10">
        <div>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-3">
            {(cards ?? []).map((c) => {
              const s = status(c);
              const blocked = s && !s.ok;
              return (
                <div key={c.id} className="min-w-0">
                  <div className="relative aspect-square overflow-hidden rounded-md bg-layer-03">
                    <MImg src={c.imageUrl} label={c.name} width={300} className={cn("absolute inset-0 size-full", blocked && "opacity-40")} />
                  </div>
                  <p className="mt-1.5 truncate text-sm font-medium">{c.name}</p>
                  <p className={cn("truncate text-xs", blocked ? "text-negative" : "text-secondary")}>{blocked ? (s as { reason: string }).reason : c.collection?.name}</p>
                </div>
              );
            })}
          </div>
          {cards && cards.length === 0 && <p className="py-10 text-center text-sm text-secondary">These items could not be loaded.</p>}
        </div>

        <div className="flex min-w-0 flex-col gap-4">
          <div>
            <Field label="To" value={to} onChange={(v) => setTo(v.replace(/\s+/g, ""))} placeholder="0x address or name.sui" prompt="Send to" def />
            <RecipientLine rs={rs} self={self} />
          </div>
          {!ready && <RecipientPicks accounts={accounts} current={account.address} filter={to} onPick={setTo} />}
          <div className="mt-auto">
            {error && (
              <p className="mb-2 text-sm text-negative" role="alert">
                {error}
              </p>
            )}
            <Btn variant={canReview ? "primary" : "dimmed"} className="w-full" onClick={canReview ? () => void submit() : undefined}>
              {busy ? "Opening review" : cta}
            </Btn>
          </div>
        </div>
      </div>
    </Page>
  );
}
