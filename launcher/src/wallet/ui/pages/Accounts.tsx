// Accounts: the switcher as a page, grouped by how each account signs. A selects and goes home,
// X renames, Y opens the account's actions.

import { Check, Copy, Pencil, Plus, Trash2, Users } from "lucide-react";
import { useEffect, useState } from "react";
import { askText } from "../../../components/Osk";
import { usePadHandler } from "../../../nav/input";
import { useApp } from "../../../lib/store";
import { fmtUsd } from "../../../marketplace/format";
import { keystore, type RareAccount } from "../../index";
import { trade } from "../../data/prices";
import { Btn, ListButton, Page, Sheet, WalletAvatar, cn } from "../kit";
import { useWallet, useWalletNav, type WalletView } from "../store";
import { ensureUnlockedFor } from "../unlock";
import { RemoveAccountSheet } from "../home/Sheets";
import { kindGroup, kindLabel, useHints } from "../home/lib";
import { failed } from "../../../lib/fail";

const GROUPS: Array<{ id: "rare" | "hardware" | "watch"; title: string }> = [
  { id: "rare", title: "Rare accounts" },
  { id: "hardware", title: "Hardware" },
  { id: "watch", title: "Watch" },
];

function Row({ a, current, value, onSelect }: { a: RareAccount; current: boolean; value: number | null | undefined; onSelect: () => void }) {
  return (
    <button data-nav data-nav-default={current ? "" : undefined} data-addr={a.address} onClick={onSelect} className="flex h-[4.5rem] w-full items-center gap-4 rounded-lg px-4 text-left transition-colors focus-visible:bg-layer-03">
      <WalletAvatar address={a.address} size="2.75rem" />
      <span className="min-w-0 flex-1">
        <span className="flex items-baseline gap-3">
          <span className="truncate text-base font-semibold">{a.label}</span>
          <span className="shrink-0 text-sm text-secondary">{kindLabel(a)}</span>
        </span>
        <span className="block font-mono text-sm text-secondary">{`${a.address.slice(0, 8)}...${a.address.slice(-6)}`}</span>
      </span>
      <span className="shrink-0 text-right tabular-nums">
        <span className="block text-base font-semibold">{value != null ? fmtUsd(value) : ""}</span>
      </span>
      {current ? <Check size="1.2rem" className="shrink-0 text-icon-primary" aria-label="Current account" /> : <span className="w-[1.2rem] shrink-0" />}
    </button>
  );
}

export function AccountsPage({ view, active }: { view: Extract<WalletView, { kind: "accounts" }>; active: boolean }) {
  void view;
  const accounts = useWallet((s) => s.accounts) ?? [];
  const current = useWallet((s) => s.current);
  const toast = useApp((s) => s.toast);
  const push = useWalletNav((s) => s.push);
  const [values, setValues] = useState<Record<string, number | null>>({});
  const [menu, setMenu] = useState<RareAccount | null>(null);
  const [removing, setRemoving] = useState<RareAccount | null>(null);
  const [busy, setBusy] = useState(false);
  const [hasPhrase, setHasPhrase] = useState(false);

  // value per account is one cheap the Trade API call each; it fills in as answers arrive
  const sig = accounts.map((a) => a.address).join(",");
  useEffect(() => {
    let live = true;
    for (const a of accounts.slice(0, 16)) {
      void trade
        .portfolio(a.address)
        .then((p) => live && setValues((v) => ({ ...v, [a.address]: p.totalUsd })))
        .catch(() => {});
    }
    return () => {
      live = false;
    };
  }, [sig]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    setHasPhrase(false);
    if (current) void keystore.hasPhrase(current.address).then(setHasPhrase);
  }, [current?.address]); // eslint-disable-line react-hooks/exhaustive-deps

  const select = async (a: RareAccount) => {
    await useWallet.getState().select(a.address);
    useWalletNav.getState().pop();
  };
  const rename = async (a: RareAccount) => {
    const v = await askText("Account name", a.label, { placeholder: "Account name" });
    if (v == null) return;
    try {
      await keystore.rename(a.address, v);
      await useWallet.getState().refresh();
    } catch (e) {
      toast(failed("Couldn't rename the account.", e), "error");
    }
  };
  const focusedAccount = () => {
    const el = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>("[data-addr]");
    return el ? accounts.find((a) => a.address === el.dataset.addr) : undefined;
  };

  usePadHandler((b) => {
    if (menu || removing) return false;
    const a = focusedAccount();
    if (!a) return false;
    if (b === "x") {
      void rename(a);
      return true;
    }
    if (b === "y") {
      setMenu(a);
      return true;
    }
    return false;
  }, active);

  useHints(active, [{ b: "a", label: "Use" }, { b: "x", label: "Rename" }, { b: "y", label: "Actions" }, { b: "b", label: "Back" }]);

  const addFromPhrase = async () => {
    if (!current?.hd || busy) return;
    setBusy(true);
    try {
      if (!(await ensureUnlockedFor(current.address, "Enter your PIN to add an account"))) return;
      const a = await keystore.deriveNext({ phraseId: current.hd.phraseId });
      await useWallet.getState().select(a.address);
      toast("Account added");
    } catch (e) {
      toast(failed("Couldn't add the account.", e), "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Page
      active={active}
      ready={accounts.length}
      title="Accounts"
      subtitle={`${accounts.length} ${accounts.length === 1 ? "account" : "accounts"} on this device`}
      right={
        <>
          {hasPhrase && (
            <Btn onClick={() => void addFromPhrase()}>
              <Plus size="1.1rem" /> {busy ? "Adding" : "Add another from this phrase"}
            </Btn>
          )}
          <Btn variant="primary" onClick={() => push({ kind: "add" })}>
            <Plus size="1.1rem" /> Add account
          </Btn>
        </>
      }
    >
      <div className="flex flex-col gap-6">
        {GROUPS.map((g) => {
          const list = accounts.filter((a) => kindGroup(a) === g.id);
          if (!list.length) return null;
          return (
            <section key={g.id}>
              <h2 className="mb-1 px-4 text-sm font-semibold text-secondary">{g.title}</h2>
              <div className={cn("rounded-lg border border-border-primary bg-layer-01 p-1")}>
                {list.map((a) => (
                  <Row key={a.address} a={a} current={a.address === current?.address} value={values[a.address]} onSelect={() => void select(a)} />
                ))}
              </div>
            </section>
          );
        })}
        {accounts.length === 0 && (
          <div className="flex flex-col items-center gap-3 py-16 text-secondary">
            <Users size="1.5rem" />
            <p className="text-sm">No accounts yet</p>
          </div>
        )}
      </div>

      {menu && (
        <Sheet title={menu.label} subtitle={kindLabel(menu)} onClose={() => setMenu(null)} width="34rem">
          <div className="-mx-2 flex flex-col">
            <ListButton def icon={<Check size="1.2rem" />} title="Use this account" onClick={() => { const a = menu; setMenu(null); void select(a); }} />
            <ListButton icon={<Pencil size="1.2rem" />} title="Rename" onClick={() => { const a = menu; setMenu(null); void rename(a); }} />
            <ListButton icon={<Copy size="1.2rem" />} title="Copy address" onClick={() => { void navigator.clipboard?.writeText(menu.address).catch(() => {}); toast("Address copied"); setMenu(null); }} />
            <ListButton icon={<Trash2 size="1.2rem" />} title="Remove from this device" onClick={() => { const a = menu; setMenu(null); setRemoving(a); }} />
          </div>
        </Sheet>
      )}
      {removing && <RemoveAccountSheet account={removing} onClose={() => setRemoving(null)} />}
    </Page>
  );
}
