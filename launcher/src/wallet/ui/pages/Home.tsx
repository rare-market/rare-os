// Rare Wallet home: account header with value, actions, then Coins / Collectibles / Activity
// tabs (LB and RB switch them). Modelled on rare.market's profile page.

import { ArrowDownLeft, ArrowUpRight, Settings as Cog, Users, UserPlus, ArrowRightLeft } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { usePadHandler } from "../../../nav/input";
import { Btn, Empty, Page, Tabs } from "../kit";
import { canSignAccount } from "../dev";
import { useWallet, useWalletNav, type WalletView } from "../store";
import { AccountHeader } from "../home/AccountHeader";
import { ActivityGroups } from "../home/ActivityList";
import { CoinHead, CoinRow } from "../home/CoinRows";
import { LoadBar, useActivity, useHints, usePortfolio } from "../home/lib";
import { InventoryView } from "../../../inventory";
import { startAutoLock } from "../home/autoLock";
import "../home/devSeed";

type Tab = "coins" | "collectibles" | "activity";
const TABS: Tab[] = ["coins", "collectibles", "activity"];

export function HomePage({ view, active }: { view: Extract<WalletView, { kind: "home" }>; active: boolean }) {
  const account = useWallet((s) => s.current);
  const epoch = useWallet((s) => s.epoch);
  const push = useWalletNav((s) => s.push);
  const [tab, setTab] = useState<Tab>(view.tab ?? "coins");
  const [selecting, setSelecting] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const address = account?.address;

  useEffect(() => startAutoLock(), []);

  const pf = usePortfolio(address, epoch);
  const act = useActivity(tab === "activity" ? address : undefined, epoch);

  // a different account starts clean
  const last = useRef(address);
  useEffect(() => {
    if (last.current !== address) {
      last.current = address;
      setSelecting(false);
      setSelected(new Set());
    }
  }, [address]);

  // the Trade API can answer a page with only a few rows; keep going until the list fills the screen
  useEffect(() => {
    if (tab === "activity" && act.ready && !act.done && !act.loading && !act.error && act.items.length < 12) act.more();
  }, [tab, act.ready, act.done, act.loading, act.error, act.items.length]); // eslint-disable-line react-hooks/exhaustive-deps

  const canSign = canSignAccount(account);
  const sendNfts = () => selected.size && push({ kind: "send-nfts", ids: [...selected] });
  const toggle = (id: string) =>
    setSelected((s) => {
      const n = new Set(s);
      if (!n.delete(id)) n.add(id);
      return n;
    });
  const stopSelecting = () => {
    setSelecting(false);
    setSelected(new Set());
  };
  const setTabSafe = (t: Tab) => {
    if (t !== "collectibles") stopSelecting();
    setTab(t);
  };

  usePadHandler((b) => {
    // LT/RT switch Coins, Collectibles and Activity (LB/RB always stay the launcher's tabs).
    if (b === "lt" || b === "rt") {
      const i = TABS.indexOf(tab) + (b === "rt" ? 1 : -1);
      setTabSafe(TABS[(i + TABS.length) % TABS.length]);
      return true;
    }
    if (b === "x" && tab === "collectibles" && canSign) {
      if (selecting) stopSelecting();
      else setSelecting(true);
      return true;
    }
    if (b === "y") {
      if (selecting) sendNfts();
      else push({ kind: "accounts" });
      return true;
    }
    if (b === "b" && selecting) {
      stopSelecting();
      return true;
    }
    return false;
  }, active);

  useHints(
    active,
    selecting
      ? [
          { b: "a", label: "Toggle" },
          { b: "y", label: selected.size ? `Send ${selected.size}` : "Send" },
          { b: "x", label: "Done" },
        ]
      : [{ b: "a", label: "Open" }, ...(tab === "collectibles" && canSign ? [{ b: "x" as const, label: "Select" }] : []), { b: "y", label: "Accounts" }, { b: "qam", label: "Quick settings" }],
  );

  if (!account) return null;
  const loading = tab === "coins" ? pf.loading : tab === "collectibles" ? false : act.loading;
  const ready = tab === "coins" ? pf.data : tab === "collectibles" ? true : act.ready;

  return (
    <Page active={active} ready={`${tab}:${!!ready}`}>
      <AccountHeader account={account} pf={pf.data} />

      <div className="mb-6 flex flex-wrap items-center gap-2">
        {canSign ? (
          <Btn variant="primary" def onClick={() => push({ kind: "send" })}>
            <ArrowUpRight size="1.1rem" /> Send
          </Btn>
        ) : (
          <Btn variant="primary" def onClick={() => push({ kind: "add" })}>
            <UserPlus size="1.1rem" /> Add a signing account
          </Btn>
        )}
        <Btn onClick={() => push({ kind: "receive" })}>
          <ArrowDownLeft size="1.1rem" /> Receive
        </Btn>
        <Btn onClick={() => push({ kind: "accounts" })}>
          <Users size="1.1rem" /> Accounts
        </Btn>
        <Btn onClick={() => push({ kind: "settings" })}>
          <Cog size="1.1rem" /> Settings
        </Btn>
        <Btn variant="ghost" className="ml-auto" onClick={() => push({ kind: "playtron" })}>
          <ArrowRightLeft size="1.1rem" /> Move from your old SuiPlay wallet
        </Btn>
      </div>

      <div className="mb-2 flex items-end justify-between border-b border-border-primary">
        <Tabs
          items={[
            { value: "coins" as Tab, label: "Coins" },
            { value: "collectibles" as Tab, label: "Collectibles" },
            { value: "activity" as Tab, label: "Activity" },
          ]}
          value={tab}
          onChange={setTabSafe}
        />
        {tab === "collectibles" && canSign && (
          <div className="mb-2 flex items-center gap-2">
            {selecting && (
              <Btn size="md" variant="primary" onClick={sendNfts}>
                {selected.size ? `Send ${selected.size}` : "Pick items to send"}
              </Btn>
            )}
            <Btn size="md" onClick={() => (selecting ? stopSelecting() : setSelecting(true))}>
              {selecting ? "Done" : "Select"}
            </Btn>
          </div>
        )}
      </div>
      <LoadBar on={loading} />

      {tab === "coins" && (
        <div className="pt-2">
          {pf.data && pf.data.tokens.length > 0 && (
            <>
              <CoinHead />
              <div data-nav-group="wallet-coins">
                {pf.data.tokens.map((t, i) => (
                  <CoinRow key={t.coinType} t={t} idx={i} onOpen={() => push({ kind: "coin", coinType: t.coinType })} />
                ))}
              </div>
            </>
          )}
          {pf.data && pf.data.tokens.length === 0 && (
            <Empty title="No coins yet" action={<Btn onClick={() => push({ kind: "receive" })}>Receive</Btn>}>
              Send Sui assets to this address and they show up here.
            </Empty>
          )}
          {pf.error && !pf.data && <Empty title="Could not load balances">Check the connection and try again.</Empty>}
        </div>
      )}

      {tab === "collectibles" && <InventoryView address={account.address} epoch={epoch} selecting={selecting} selected={selected} onToggle={toggle} />}

      {tab === "activity" && (
        <div className="pt-2">
          <div data-nav-group="wallet-activity">
            <ActivityGroups items={act.items} onOpen={(a) => push({ kind: "activity", digest: a.digest })} />
          </div>
          {act.items.length > 0 && !act.done && !act.error && (
            <div className="flex justify-center py-3">
              <Btn size="md" onClick={act.more}>
                Show more
              </Btn>
            </div>
          )}
          {act.ready && act.items.length === 0 && <Empty title="No activity yet">Your transactions show up here.</Empty>}
          {act.error && (
            <Empty compact title={act.ready ? "Couldn't load older activity" : "Couldn't load activity"} action={<Btn size="md" onClick={act.retry}>Retry</Btn>} />
          )}
        </div>
      )}
    </Page>
  );
}
