// The marketplace's own page stack inside the Marketplace tab. Every page in the stack stays
// mounted (the ones underneath are invisible and inert), so B returns instantly to the same
// scroll position, and focus goes back to the element that opened the page.

import { create } from "zustand";
import type { ItemCard } from "./api";
import type { Hint } from "../components/Chrome";
import { useApp } from "../lib/store";
import { setArea, useMarketArea } from "../market/area";

export type View =
  | { kind: "home" }
  | { kind: "collection"; slug: string }
  /** `card`/`thumb`: what the opener already showed (and the width its art was drawn at), painted at once while the item loads. */
  | { kind: "item"; id: string; card?: ItemCard; thumb?: number }
  | { kind: "search"; q: string }
  | { kind: "account"; address: string; label: string };

interface Frame {
  key: number;
  view: View;
  /** The element focused when the next page opened, restored on B. */
  focus: HTMLElement | null;
}

let seq = 0;
const MAX_DEPTH = 8;

interface NavState {
  stack: Frame[];
  push: (v: View) => void;
  pop: () => boolean;
  reset: (views?: View[]) => void;
}

export const useMarketNav = create<NavState>((set, get) => ({
  stack: [{ key: ++seq, view: { kind: "home" }, focus: null }],
  push: (view) => {
    const stack = get().stack.slice();
    stack[stack.length - 1] = { ...stack[stack.length - 1], focus: document.activeElement as HTMLElement | null };
    stack.push({ key: ++seq, view, focus: null });
    // Deep chains (item, collection, item, ...) drop the oldest pages above home.
    while (stack.length > MAX_DEPTH) stack.splice(1, 1);
    set({ stack });
  },
  pop: () => {
    const s = get().stack;
    if (s.length <= 1) return false;
    set({ stack: s.slice(0, -1) });
    return true;
  },
  reset: (views = []) => set({ stack: [{ key: ++seq, view: { kind: "home" }, focus: null }, ...views.map((view) => ({ key: ++seq, view, focus: null }))] }),
}));

/** Opens a marketplace page from anywhere in the launcher (search, wallet). */
export function openInMarket(view: View) {
  const app = useApp.getState();
  app.close();
  const nav = useMarketNav.getState();
  if (app.tab === "market" && useMarketArea.getState().area === "collections") nav.push(view);
  else {
    nav.reset([view]);
    setArea("collections");
    app.setTab("market");
  }
}

/** Context hints for the bottom bar while the Marketplace tab is showing. */
export const useMarketHints = create<{ hints: Hint[] | null }>(() => ({ hints: null }));
