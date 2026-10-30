// Scripted controller input for checking focus on a device without pressing buttons.
// RARE_DEBUG_NAV (native env) or ?nav= (browser) holds a comma separated script:
//   right,right,down        pad buttons (up down left right a b x y lb rb lt rt start select)
//   wait:800                pause
//   tab:library             switch tab
//   game:steam:1145350      open a game page
//   app:chrome              open an app's page
//   down*20                 a held direction: 20 presses 60 ms apart (no per-press log)
//   frames:label            log frame times (p50/p95/max) since the last frames: step
// Each press is timed from dispatch to the second frame after it and logged to stderr
// through debug_log, with what ended up focused.

import { native } from "../lib/bridge";
import { useApp, type Tab } from "../lib/store";
import type { PadButton } from "../lib/types";
import { dispatch } from "./input";

const BUTTONS = new Set(["up", "down", "left", "right", "a", "b", "x", "y", "lb", "rb", "lt", "rt", "start", "select", "guide", "qam"]);
const frames = () => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export function describe(el: Element | null): string {
  if (!el || el === document.body) return "(nothing)";
  const h = el as HTMLElement;
  const text = (h.innerText || h.getAttribute("aria-label") || "").replace(/\s+/g, " ").trim().slice(0, 40);
  const group = h.closest<HTMLElement>("[data-nav-group]")?.dataset.navGroup;
  const r = h.getBoundingClientRect();
  return `${h.tagName.toLowerCase()}${group ? `@${group}` : ""} "${text}" at ${Math.round(r.left)},${Math.round(r.top)} ${Math.round(r.width)}x${Math.round(r.height)}`;
}

export async function runDebugNav() {
  const { nav } = await native.debugEnv().catch(() => ({ nav: null }));
  if (!nav) return;
  const log = (m: string) => void native.debugLog(m).catch(() => {});
  // Wait for the first library scan and a moment for art and catalogs.
  for (let i = 0; i < 100 && !useApp.getState().library; i++) await sleep(100);
  await sleep(2500);
  log(`nav script: ${nav}`);
  log(`audio: ${typeof AudioContext !== "undefined" ? "AudioContext available" : "no AudioContext"}`);
  const times: number[] = [];
  // rAF frame-time logger: every frame's duration while the script runs.
  let frameLog: number[] = [];
  let last = performance.now();
  let running = true;
  const tick = (t: number) => {
    frameLog.push(t - last);
    last = t;
    if (running) requestAnimationFrame(tick);
  };
  requestAnimationFrame((t) => {
    last = t;
    requestAnimationFrame(tick);
  });
  let segment = "start";
  const flush = (next: string) => {
    const f = frameLog.slice().sort((a, b) => a - b);
    if (f.length > 5) {
      const q = (p: number) => f[Math.min(f.length - 1, Math.floor(f.length * p))].toFixed(1);
      const janky = f.filter((x) => x > 25).length;
      log(`frames ${segment}: n=${f.length} p50 ${q(0.5)} ms, p95 ${q(0.95)} ms, max ${f[f.length - 1].toFixed(1)} ms, ${janky} frames over 25 ms`);
    }
    frameLog = [];
    segment = next;
  };
  for (const raw of nav.split(",").map((s) => s.trim()).filter(Boolean)) {
    if (raw.startsWith("frames:")) {
      flush(raw.slice(7));
      continue;
    }
    const held = raw.match(/^([a-z]+)\*(\d+)$/);
    if (held && BUTTONS.has(held[1])) {
      for (let i = 0; i < Number(held[2]); i++) {
        dispatch(held[1] as PadButton, i > 0);
        await sleep(60);
      }
      log(`${raw} -> ${describe(document.activeElement)}`);
      continue;
    }
    if (raw.startsWith("wait:")) {
      await sleep(Number(raw.slice(5)) || 300);
      continue;
    }
    if (raw.startsWith("tab:")) {
      useApp.getState().setTab(raw.slice(4) as Tab);
      await sleep(600);
      continue;
    }
    if (raw.startsWith("game:")) {
      useApp.getState().open({ kind: "game", id: raw.slice(5) });
      await sleep(600);
      continue;
    }
    if (raw.startsWith("app:")) {
      useApp.getState().open({ kind: "app", id: raw.slice(4) });
      await sleep(600);
      continue;
    }
    if (!BUTTONS.has(raw)) {
      log(`unknown step ${raw}`);
      continue;
    }
    const t0 = performance.now();
    dispatch(raw as PadButton);
    await frames();
    const ms = performance.now() - t0;
    times.push(ms);
    log(`${raw.padEnd(6)} ${ms.toFixed(1).padStart(6)} ms  -> ${describe(document.activeElement)}`);
    await sleep(220);
  }
  if (times.length) {
    const avg = times.reduce((a, b) => a + b, 0) / times.length;
    log(`nav timing: ${times.length} presses, avg ${avg.toFixed(1)} ms, max ${Math.max(...times).toFixed(1)} ms (dispatch to second frame)`);
  }
  flush("end");
  running = false;
  log(`focused at end: ${describe(document.activeElement)}`);
}
