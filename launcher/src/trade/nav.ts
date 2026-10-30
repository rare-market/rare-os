// The Trade tab's page stack, its controller hooks and its bottom-bar hints, built the way the
// Marketplace's are: every page in the stack stays mounted (the ones underneath are invisible and
// inert), so B returns instantly to the same scroll position with focus on what opened the page.
// The Trade root owns the only pad handler and asks the page on top first.

import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import type { Hint } from "../nav/hints";
import { useApp } from "../lib/store";
import { setArea, useMarketArea } from "../market/area";
import type { PadButton } from "../lib/types";
import { focusFirst, spot, type Spot } from "../nav/focus";
import { usePadHandler } from "../nav/input";

export type Section = "home" | "perps" | "traders" | "positions";
export const SECTIONS: Array<{ value: Section; label: string }> = [
  { value: "home", label: "Home" },
  { value: "perps", label: "Perps" },
  { value: "traders", label: "Traders" },
  { value: "positions", label: "Positions" },
];

export type View =
  | { kind: "root" }
  | { kind: "token"; coinType: string; side?: "buy" | "sell" }
  | { kind: "swap"; from?: string; to?: string }
  | { kind: "perp"; marketId: string }
  | { kind: "trader"; address: string };

interface Frame {
  key: number;
  view: View;
  /** The element focused when the next page opened, restored on B. */
  focus: HTMLElement | null;
  /** The same place as the focus engine knows it (group and index, scroll). */
  spot?: Spot | null;
}

let seq = 0;
const MAX_DEPTH = 8;

interface NavState {
  stack: Frame[];
  section: Section;
  setSection: (s: Section) => void;
  push: (v: View) => void;
  pop: () => boolean;
  reset: (views?: View[]) => void;
}

export const useTradeNav = create<NavState>((set, get) => ({
  stack: [{ key: ++seq, view: { kind: "root" }, focus: null }],
  section: "home",
  setSection: (section) => set({ section }),
  push: (view) => {
    const stack = get().stack.slice();
    stack[stack.length - 1] = { ...stack[stack.length - 1], focus: document.activeElement as HTMLElement | null, spot: spot() };
    stack.push({ key: ++seq, view, focus: null });
    while (stack.length > MAX_DEPTH) stack.splice(1, 1);
    set({ stack });
  },
  pop: () => {
    const s = get().stack;
    if (s.length <= 1) return false;
    set({ stack: s.slice(0, -1) });
    return true;
  },
  reset: (views = []) => set({ stack: [{ key: ++seq, view: { kind: "root" }, focus: null }, ...views.map((view) => ({ key: ++seq, view, focus: null }))] }),
}));

/** Opens a Trade page from anywhere in the launcher (wallet, search). */
export function openInTrade(view: View) {
  const app = useApp.getState();
  app.close();
  const nav = useTradeNav.getState();
  if (app.tab === "market" && useMarketArea.getState().area === "trade") nav.push(view);
  else {
    nav.reset([view]);
    setArea("trade");
    app.setTab("market");
  }
}

/** Context hints for the bottom bar while the Trade tab is showing; an open sheet's win. */
export const useTradeHints = create<{ hints: Hint[] | null; sheet: Hint[] | null }>(() => ({ hints: null, sheet: null }));

/**
 * A sheet's controller: B closes it, the page's own shortcuts are held while it is up, and the
 * bottom bar shows the sheet's hints.
 */
export function useSheetPad(onClose: () => void, hints: Hint[], active = true) {
  usePadHandler((b) => {
    if (b === "b") {
      onClose();
      return true;
    }
    return b === "lb" || b === "rb" || b === "lt" || b === "rt" || b === "x" || b === "y";
  }, active);
  const sig = hints.map((h) => h.b + h.label).join("|");
  useEffect(() => {
    useTradeHints.setState({ sheet: active ? hints : [{ b: "a", label: "Select" }, { b: "b", label: "Back" }] });
  }, [sig, active]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => useTradeHints.setState({ sheet: null }), []);
}

export type PageHandler = (b: PadButton, repeat: boolean) => boolean;
export const pageHandlers = new Map<number, { current: PageHandler }>();

/** Buttons for one page; return true to consume. Only the top page is asked. */
export function usePageButtons(frame: number, handler: PageHandler) {
  const ref = useRef(handler);
  ref.current = handler;
  useEffect(() => {
    pageHandlers.set(frame, ref);
    return () => {
      if (pageHandlers.get(frame) === ref) pageHandlers.delete(frame);
    };
  }, [frame]);
}

/**
 * Buttons for the section showing on the Trade root (Traders' X), asked before the root's own.
 * Only one section is mounted at a time.
 */
export const sectionButtons: { current: PageHandler | null } = { current: null };
export function useSectionButtons(handler: PageHandler) {
  const ref = useRef(handler);
  ref.current = handler;
  useEffect(() => {
    const fn: PageHandler = (b, r) => ref.current(b, r);
    sectionButtons.current = fn;
    return () => {
      if (sectionButtons.current === fn) sectionButtons.current = null;
    };
  }, []);
}

/** Sets the bottom bar while this page is on top. */
export function usePageHints(active: boolean, hints: Hint[]) {
  const sig = hints.map((h) => h.b + h.label).join("|");
  useEffect(() => {
    if (active) useTradeHints.setState({ hints });
  }, [active, sig]); // eslint-disable-line react-hooks/exhaustive-deps
}

/**
 * Lands focus on the page's default element when it opens, and again when its data arrives, as
 * long as the person has not moved away from where it first landed.
 */
export function useLandingFocus(root: React.RefObject<HTMLElement | null>, active: boolean, ready: unknown) {
  const auto = useRef<Element | null>(null);
  useEffect(() => {
    if (!active) return;
    const land = (late: boolean) => {
      const el = root.current;
      if (!el) return;
      const a = document.activeElement;
      const first = late && a === el.querySelector("[data-nav]") && a !== el.querySelector("[data-nav-default]");
      if (!a || a === document.body || a === auto.current || !el.contains(a) || first) {
        focusFirst(el);
        auto.current = document.activeElement;
      }
    };
    const t1 = setTimeout(() => land(false), 40);
    const t2 = setTimeout(() => land(true), 250);
    return () => {
      clearTimeout(t1);
      clearTimeout(t2);
    };
  }, [ready]); // eslint-disable-line react-hooks/exhaustive-deps
}

export const H = {
  open: { b: "a", label: "Open" } as Hint,
  select: { b: "a", label: "Select" } as Hint,
  back: { b: "b", label: "Back" } as Hint,
  close: { b: "b", label: "Close" } as Hint,
  search: { b: "y", label: "Search" } as Hint,
  qs: { b: "qam", label: "Quick settings" } as Hint,
};

export type Area = "chart" | "order" | "other";

/** Which part of a page holds focus: the chart, the order panel, or anything else. */
export function useFocusArea(root: React.RefObject<HTMLElement | null>): Area {
  const [area, setArea] = useState<Area>("other");
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    const read = () => {
      const a = document.activeElement?.closest("[data-area]")?.getAttribute("data-area");
      setArea(a === "chart" || a === "order" ? a : "other");
    };
    el.addEventListener("focusin", read);
    el.addEventListener("focusout", read);
    read();
    return () => {
      el.removeEventListener("focusin", read);
      el.removeEventListener("focusout", read);
    };
  }, [root]);
  return area;
}

/** True when focus sits inside the page's order panel (LT/RT step the amount only there). */
export const inOrderPanel = () => !!document.activeElement?.closest("[data-area=order]");
