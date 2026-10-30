// The art behind the top of Home: the focused game's hero, full bleed under the top bar, like a
// console home. Changing it cross-fades two stacked layers (opacity only); scrolling Home fades
// and lifts it (transform and opacity, written straight to the element, no re-render).

import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { Art } from "./Art";

interface BackdropState {
  key: string | null;
  sources: string[];
  title: string;
}

export const useBackdrop = create<BackdropState>(() => ({ key: null, sources: [], title: "" }));

/** Show a game's art behind the page (null clears it). */
export function setBackdrop(b: { key: string; sources: string[]; title: string } | null) {
  const cur = useBackdrop.getState();
  if (!b) {
    if (cur.key !== null) useBackdrop.setState({ key: null, sources: [], title: "" });
    return;
  }
  if (cur.key !== b.key) useBackdrop.setState(b);
}

let el: HTMLDivElement | null = null;
/** Home's scroll offset in px: the art fades out over the first screen and drifts up a little. */
export function scrollBackdrop(px: number) {
  if (!el) return;
  const k = Math.min(1, px / (window.innerHeight * 0.55));
  el.style.opacity = String(1 - k);
  el.style.transform = `translate3d(0, ${-px * 0.25}px, 0)`;
}

export function Backdrop() {
  const b = useBackdrop();
  const ref = useRef<HTMLDivElement>(null);
  // The last two layers: the new one fades in over the old, then the old is dropped.
  const [layers, setLayers] = useState<BackdropState[]>([]);
  useEffect(() => {
    if (!b.key) {
      setLayers([]);
      return;
    }
    setLayers((l) => (l[l.length - 1]?.key === b.key ? l : [...l.slice(-1), b]));
  }, [b]);
  useEffect(() => {
    el = ref.current;
    return () => {
      el = null;
    };
  }, []);
  return (
    <div ref={ref} aria-hidden className="pointer-events-none absolute inset-x-0 top-0 z-0 h-[40rem] overflow-hidden bg-bg" style={{ willChange: "opacity, transform" }}>
      {layers.map((l) => (
        <div key={l.key} className="fade absolute inset-0">
          <Art sources={l.sources} title={l.title} variant="hero" className="absolute inset-0 h-full w-full object-cover object-[center_25%] mix-blend-lighten" />
        </div>
      ))}
      {layers.length > 0 && (
        <>
          {/* Fades into the page: from the left behind the text, and down into the shelves. */}
          <div className="absolute inset-x-0 top-0 h-[7rem] bg-gradient-to-b from-bg/80 to-transparent" />
          <div className="absolute inset-y-0 left-0 w-[62%] bg-gradient-to-r from-bg via-bg/55 to-transparent" />
          <div className="absolute inset-x-0 bottom-0 h-[45%] bg-gradient-to-t from-bg via-bg/60 to-transparent" />
        </>
      )}
    </div>
  );
}
