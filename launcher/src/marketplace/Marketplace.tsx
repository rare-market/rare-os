// Rare Marketplace, native: the Marketplace tab's page stack. Every page stays mounted while it is
// in the stack (pages underneath are invisible and inert), so B goes back instantly with the
// scroll position kept and focus on the element that opened the page.

import { useEffect, useLayoutEffect, useRef } from "react";
import { useApp } from "../lib/store";
import { useMarketArea } from "../market/area";
import { focusEl } from "../nav/focus";
import { usePadHandler } from "../nav/input";
import { AccountPage } from "./AccountPage";
import { CollectionPage } from "./CollectionPage";
import { HomePage } from "./HomePage";
import { ItemPage } from "./ItemPage";
import { useMarketHints, useMarketNav, type View } from "./nav";
import { pageHandlers } from "./page";
import { MarketLoadBar } from "./parts";
import { SearchPage } from "./SearchPage";

function Page({ view, frame, active }: { view: View; frame: number; active: boolean }) {
  switch (view.kind) {
    case "home":
      return <HomePage frame={frame} active={active} />;
    case "collection":
      return <CollectionPage slug={view.slug} frame={frame} active={active} />;
    case "item":
      return <ItemPage id={view.id} card={view.card} thumb={view.thumb} frame={frame} active={active} />;
    case "search":
      return <SearchPage initial={view.q} frame={frame} active={active} />;
    case "account":
      return <AccountPage address={view.address} label={view.label} frame={frame} active={active} />;
  }
}

export function Marketplace() {
  const stack = useMarketNav((s) => s.stack);
  const top = stack[stack.length - 1];
  const prevTop = useRef(top.key);

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

  useEffect(() => () => useMarketHints.setState({ hints: null }), []);

  // Dev and screenshots: ?mk=collection/popkins,item/0x... opens those pages over the front page.
  useEffect(() => {
    const mk = new URLSearchParams(location.search).get("mk");
    if (!mk || useMarketNav.getState().stack.length > 1) return;
    const views = mk.split(",").flatMap((p): View[] => {
      const [kind, arg] = p.split("/");
      if (kind === "collection" && arg) return [{ kind, slug: arg }];
      if (kind === "item" && arg) return [{ kind, id: arg }];
      if (kind === "account" && arg) return [{ kind, address: arg, label: "Wallet" }];
      if (kind === "search") return [{ kind, q: arg ?? "" }];
      return [];
    });
    if (views.length) useMarketNav.getState().reset(views);
  }, []);

  usePadHandler((b, repeat) => {
    const app = useApp.getState();
    if (app.overlay || app.tab !== "market" || useMarketArea.getState().area !== "collections") return false;
    const nav = useMarketNav.getState();
    const frame = nav.stack[nav.stack.length - 1];
    if (pageHandlers.get(frame.key)?.current(b, repeat)) return true;
    switch (b) {
      case "b":
        return nav.pop();
      case "y":
        if (frame.view.kind === "search") return false;
        nav.push({ kind: "search", q: "" });
        return true;
      default:
        return false;
    }
  });

  return (
    <div className="relative h-full">
      <MarketLoadBar />
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
