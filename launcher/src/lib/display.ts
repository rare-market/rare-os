// Docked display logic that needs no native side: how big the UI is for a window, what to say when
// a TV is plugged in or out. The root font size itself is set in styles/app.css (rem layout against
// a 1280x800 design: min(width / 80, height / 50)); this mirrors it so it can be tested.

import type { Config, DisplayState } from "./types";

export type UiScale = "auto" | "large" | "larger";
export const UI_SCALES: UiScale[] = ["auto", "large", "larger"];

/** Multiplier on top of the automatic size, written to --ui-scale. */
export function scaleFactor(s: UiScale): number {
  return s === "large" ? 1.15 : s === "larger" ? 1.3 : 1;
}

export function scaleOf(c: Pick<Config, "uiScale"> | null | undefined): UiScale {
  return c?.uiScale === "large" || c?.uiScale === "larger" ? c.uiScale : "auto";
}

export function nextScale(s: UiScale): UiScale {
  return UI_SCALES[(UI_SCALES.indexOf(s) + 1) % UI_SCALES.length];
}

export function scaleLabel(s: UiScale): string {
  return s === "auto" ? "Automatic" : s === "large" ? "Large" : "Larger";
}

/** The root font size in px for a window, as the stylesheet computes it. 16 at 1280x800, 24 at
 *  1920x1200 (the internal panel, rotated), 21.6 at 1920x1080, 43.2 at 3840x2160. */
export function rootPx(width: number, height: number, s: UiScale = "auto"): number {
  return Math.min(width / 80, height / 50) * scaleFactor(s);
}

/** "3840x2160" for a window, rounded to what the output really is. */
export function sizeLabel(w: number, h: number): string {
  return `${Math.round(w)}x${Math.round(h)}`;
}

/** The Docked line for Settings > Display. */
export function dockedLine(d: DisplayState | null | undefined): string {
  if (!d) return "";
  if (!d.docked) return "Built-in screen";
  const name = d.dockName ?? d.active ?? "external display";
  return d.dockMode ? `Docked to ${name} ${d.dockMode}` : `Docked to ${name}`;
}

/** The toast for a change between two readings, or null when nothing worth saying changed. */
export function dockChange(prev: DisplayState | null | undefined, next: DisplayState): string | null {
  if (!prev) return null; // first reading: starting up already docked is not news
  if (!prev.docked && next.docked) return dockedLine(next);
  if (prev.docked && !next.docked) return "Back on the built-in screen";
  if (prev.docked && next.docked && (prev.dockName !== next.dockName || prev.dockMode !== next.dockMode)) return dockedLine(next);
  return null;
}
