// What the bottom hint bar shows. It always matches what the buttons do right now:
//   A  the focused element's action (data-hint-a, else "Select")
//   B  "Back" whenever B has somewhere to go
//   X  the screen's or focused element's extra action (useHints / data-hint-x)
//   Y  "Search" unless a screen claims it
// Screens and overlays register their extra hints with useHints; the topmost wins.

import { useEffect, useState } from "react";
import { create } from "zustand";
import type { PadButton } from "../lib/types";
import { isGuest } from "../lib/guest";

export interface Hint {
  b: PadButton;
  label: string;
}

/**
 * On game and app pages: how to reach Rare from inside the game. On Rare OS that is the Rare
 * button ("Menu"); in guest mode Steam keeps it, so the hint is the View hold.
 */
export function exitHint(): Hint {
  return isGuest() ? { b: "select", label: "Hold View for the Rare menu" } : { b: "rare", label: "Menu" };
}

let seq = 0;
export const useHintStack = create<{ stack: Array<{ id: number; hints: Hint[] }> }>(() => ({ stack: [] }));

/** Extra hints (X, Y, LT/RT...) while this component is mounted. */
export function useHints(hints: Hint[], active = true) {
  const key = hints.map((h) => h.b + h.label).join("|");
  useEffect(() => {
    if (!active) return;
    const id = ++seq;
    useHintStack.setState((s) => ({ stack: [...s.stack, { id, hints }] }));
    return () => useHintStack.setState((s) => ({ stack: s.stack.filter((e) => e.id !== id) }));
  }, [key, active]); // eslint-disable-line react-hooks/exhaustive-deps
}

/** The focused element's own hints, updated as focus moves. */
export function useFocusedHints(): { a: string | null; x: string | null; y: string | null; menu: string | null; focused: boolean } {
  const read = () => {
    const el = document.activeElement as HTMLElement | null;
    const nav = el?.closest?.<HTMLElement>("[data-nav]") ?? null;
    return { a: nav?.dataset.hintA ?? null, x: nav?.dataset.hintX ?? null, y: nav?.dataset.hintY ?? null, menu: nav?.closest<HTMLElement>("[data-game]") ? "Options" : null, focused: !!nav };
  };
  const [v, setV] = useState(read);
  useEffect(() => {
    let raf = 0;
    const update = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() =>
        setV((old) => {
          const n = read();
          return old.a === n.a && old.x === n.x && old.y === n.y && old.menu === n.menu && old.focused === n.focused ? old : n;
        }),
      );
    };
    document.addEventListener("focusin", update);
    document.addEventListener("focusout", update);
    return () => {
      document.removeEventListener("focusin", update);
      document.removeEventListener("focusout", update);
      cancelAnimationFrame(raf);
    };
  }, []);
  return v;
}
