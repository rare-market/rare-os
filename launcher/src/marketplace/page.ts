// Per-page controller hooks. The Marketplace root owns the only pad handler and asks the page on
// top first, so handler order never depends on React's effect order.

import { useEffect, useRef } from "react";
import type { Hint } from "../components/Chrome";
import type { PadButton } from "../lib/types";
import { focusFirst } from "../nav/focus";
import { useMarketHints } from "./nav";

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

/** Sets the bottom bar while this page is on top. */
export function usePageHints(active: boolean, hints: Hint[]) {
  const sig = hints.map((h) => h.b + h.label).join("|");
  useEffect(() => {
    if (active) useMarketHints.setState({ hints });
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
      // Late pass: the launcher's own tab-change focus may have put focus on the page's first element.
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
