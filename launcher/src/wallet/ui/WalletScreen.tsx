// Rare Wallet tab root: a page stack like the Marketplace's. Every page in the stack stays
// mounted (pages underneath are invisible and inert), so B goes back instantly with scroll and
// focus kept. With no accounts the home page is the welcome screen (passkey sign-in first).

import { useEffect, useLayoutEffect, useRef } from "react";
import { focusEl } from "../../nav/focus";
import { usePadHandler } from "../../nav/input";
import { InventorySheetHost } from "../../inventory";
import { keystore } from "../index";
import { isTauri } from "../native";
import { devWatch } from "./dev";
import { useWallet, useWalletHints, useWalletNav, type WalletView } from "./store";
import { AccountsPage } from "./pages/Accounts";
import { ActivityDetailPage } from "./pages/ActivityDetail";
import { AddAccountPage } from "./pages/AddAccount";
import { CoinPage } from "./pages/Coin";
import { CreatePhrasePage } from "./pages/CreatePhrase";
import { HomePage } from "./pages/Home";
import { ImportPage } from "./pages/Import";
import { KeystonePage } from "./pages/Keystone";
import { LedgerPage } from "./pages/Ledger";
import { NftPage } from "./pages/Nft";
import { PasskeyPage } from "./pages/Passkey";
import { PlaytronPage } from "./pages/Playtron";
import { ReceivePage } from "./pages/Receive";
import { SendPage } from "./pages/Send";
import { SendNftsPage } from "./pages/SendNfts";
import { SettingsPage } from "./pages/Settings";
import { WatchPage } from "./pages/Watch";
import { WelcomePage } from "./pages/Welcome";

function Page({ view, active, empty }: { view: WalletView; active: boolean; empty: boolean }) {
  switch (view.kind) {
    case "home":
      return empty ? <WelcomePage active={active} /> : <HomePage view={view} active={active} />;
    case "coin":
      return <CoinPage view={view} active={active} />;
    case "nft":
      return <NftPage view={view} active={active} />;
    case "activity":
      return <ActivityDetailPage view={view} active={active} />;
    case "send":
      return <SendPage view={view} active={active} />;
    case "send-nfts":
      return <SendNftsPage view={view} active={active} />;
    case "receive":
      return <ReceivePage view={view} active={active} />;
    case "accounts":
      return <AccountsPage view={view} active={active} />;
    case "settings":
      return <SettingsPage view={view} active={active} />;
    case "add":
      return <AddAccountPage view={view} active={active} />;
    case "passkey":
      return <PasskeyPage view={view} active={active} />;
    case "create-phrase":
      return <CreatePhrasePage view={view} active={active} />;
    case "import":
      return <ImportPage view={view} active={active} />;
    case "watch":
      return <WatchPage view={view} active={active} />;
    case "ledger":
      return <LedgerPage view={view} active={active} />;
    case "keystone":
      return <KeystonePage view={view} active={active} />;
    case "playtron":
      return <PlaytronPage view={view} active={active} />;
  }
}

export function WalletScreen() {
  const stack = useWalletNav((s) => s.stack);
  const accounts = useWallet((s) => s.accounts);
  const top = stack[stack.length - 1];
  const prevTop = useRef(top.key);

  useEffect(() => {
    void (async () => {
      if (!isTauri && devWatch && !(await keystore.current())) await keystore.addWatch(devWatch, "Demo wallet").catch(() => {});
      await useWallet.getState().refresh();
    })();
    return () => useWalletHints.setState({ hints: null });
  }, []);

  // Dev and screenshots: ?wv=send,receive opens those pages over home.
  useEffect(() => {
    const wv = new URLSearchParams(location.search).get("wv");
    if (!wv || useWalletNav.getState().stack.length > 1) return;
    const views = wv.split(",").flatMap((p): WalletView[] => {
      const [kind, arg] = p.split("/");
      switch (kind) {
        case "send":
        case "receive":
        case "accounts":
        case "settings":
        case "add":
        case "create-phrase":
        case "watch":
        case "ledger":
        case "keystone":
        case "playtron":
          return [{ kind } as WalletView];
        case "passkey":
          return [{ kind, mode: arg === "signin" ? "signin" : "create" }];
        case "import":
          return [{ kind, what: arg === "key" ? "key" : "phrase" }];
        case "coin":
          return arg ? [{ kind, coinType: decodeURIComponent(arg) }] : [];
        case "nft":
          return arg ? [{ kind, id: arg }] : [];
        case "activity":
          return arg ? [{ kind, digest: arg }] : [];
        case "home":
          return [];
      }
      return [];
    });
    if (views.length) useWalletNav.getState().reset(views);
  }, []);

  // Back: focus returns to what opened the page we left.
  useLayoutEffect(() => {
    if (prevTop.current === top.key) return;
    const wentBack = !stack.some((f) => f.key === prevTop.current);
    prevTop.current = top.key;
    if (wentBack && top.focus?.isConnected) {
      const el = top.focus;
      requestAnimationFrame(() => focusEl(el, { scroll: false }));
    }
  }, [top.key]); // eslint-disable-line react-hooks/exhaustive-deps

  usePadHandler((b) => {
    if (b === "b") return useWalletNav.getState().pop();
    return false;
  });

  if (accounts === undefined) return null;
  const empty = accounts.length === 0;
  return (
    <div className="relative h-full">
      {stack.map((f, i) => {
        const active = i === stack.length - 1;
        return (
          <div key={f.key} className={active ? "absolute inset-0" : "invisible absolute inset-0"} inert={active ? undefined : true}>
            <Page view={f.view} active={active} empty={empty} />
          </div>
        );
      })}
      <InventorySheetHost />
    </div>
  );
}
