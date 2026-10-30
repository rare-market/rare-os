// Rare Trade: the Trade tab's page stack. Every page stays mounted while it is in the stack (pages
// underneath are invisible and inert), so B goes back instantly with the scroll position kept and
// focus on the element that opened the page.

import { useEffect, useLayoutEffect, useRef } from "react";
import { useApp } from "../lib/store";
import { useMarketArea } from "../market/area";
import { focusEl, restoreLater } from "../nav/focus";
import { usePadHandler } from "../nav/input";
import { pageHandlers, useTradeHints, useTradeNav, type View } from "./nav";
import { TradeLoadBar } from "./parts";
import { PerpPage } from "./PerpPage";
import { RootPage } from "./RootPage";
import { SwapPage } from "./SwapPage";
import { TokenPage } from "./TokenPage";
import { TraderPage } from "./traders/TraderPage";

function Page({ view, frame, active }: { view: View; frame: number; active: boolean }) {
  switch (view.kind) {
    case "root":
      return <RootPage frame={frame} active={active} />;
    case "token":
      return <TokenPage coinType={view.coinType} initialSide={view.side} frame={frame} active={active} />;
    case "swap":
      return <SwapPage from={view.from} to={view.to} frame={frame} active={active} />;
    case "perp":
      return <PerpPage marketId={view.marketId} frame={frame} active={active} />;
    case "trader":
      return <TraderPage address={view.address} frame={frame} active={active} />;
  }
}

export function Trade() {
  const stack = useTradeNav((s) => s.stack);
  const top = stack[stack.length - 1];
  const prevTop = useRef(top.key);

  useLayoutEffect(() => {
    if (prevTop.current === top.key) return;
    const wentBack = !stack.some((f) => f.key === prevTop.current);
    prevTop.current = top.key;
    if (wentBack && top.focus?.isConnected) {
      const el = top.focus;
      requestAnimationFrame(() => focusEl(el, { scroll: false }));
      // Through the focus engine too, so its own landing never moves focus off the restored spot.
      if (top.spot) restoreLater(top.spot);
    }
  }, [top.key]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => () => useTradeHints.setState({ hints: null, sheet: null }), []);

  // Dev and screenshots: ?tr=perps | traders | positions | token/<type> | swap | perp/<id> | trader/<address>[,more pages].
  useEffect(() => {
    const tr = new URLSearchParams(location.search).get("tr");
    if (!tr || useTradeNav.getState().stack.length > 1) return;
    const nav = useTradeNav.getState();
    const views = tr.split(",").flatMap((p): View[] => {
      const [kind, ...rest] = p.split("/");
      const arg = rest.join("/");
      if (kind === "perps" || kind === "traders" || kind === "positions" || kind === "home") {
        nav.setSection(kind);
        return [];
      }
      if (kind === "token" && arg) return [{ kind: "token", coinType: decodeURIComponent(arg) }];
      if (kind === "sell" && arg) return [{ kind: "token", coinType: decodeURIComponent(arg), side: "sell" }];
      if (kind === "perp" && arg) return [{ kind: "perp", marketId: arg }];
      if (kind === "swap") return [{ kind: "swap" }];
      if (kind === "trader" && /^0x[0-9a-fA-F]{64}$/.test(arg)) return [{ kind: "trader", address: arg.toLowerCase() }];
      return [];
    });
    if (views.length) nav.reset(views);
  }, []);

  usePadHandler((b, repeat) => {
    const app = useApp.getState();
    if (app.overlay || app.tab !== "market" || useMarketArea.getState().area !== "trade") return false;
    const nav = useTradeNav.getState();
    const frame = nav.stack[nav.stack.length - 1];
    if (pageHandlers.get(frame.key)?.current(b, repeat)) return true;
    if (b === "b") return nav.pop();
    return false;
  });

  return (
    <div className="relative h-full">
      <TradeLoadBar />
      {/* Top page first in the DOM, so App's tab-change focusFirst finds its default before a hidden page's. */}
      {[...stack].reverse().map((f) => {
        const active = f.key === top.key;
        return (
          <div key={f.key} className={`absolute inset-0 ${active ? "fade-in" : "invisible"}`} inert={active ? undefined : true}>
            <Page view={f.view} frame={f.key} active={active} />
          </div>
        );
      })}
    </div>
  );
}
