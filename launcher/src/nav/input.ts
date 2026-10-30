// Input: controller events from the Rust side, the keyboard (desktop and the handheld's
// extra keys), and the browser Gamepad API when running outside Tauri. Everything turns
// into PadButton presses that go down a handler stack: the topmost handler (an open
// keyboard, a slider being dragged, a modal) sees each press first.

import { useEffect, useRef } from "react";
import { isTauri, onPad, onPadStick } from "../lib/bridge";
import { activity, noteInput } from "../lib/idle";
import type { PadButton } from "../lib/types";
import { activate, jump, move } from "./focus";

export type Handler = (b: PadButton, repeat: boolean) => boolean;

const stack: Handler[] = [];

// ---- deliberate activation ----
// Starting a game takes a deliberate A (controller or keyboard, never a repeat) or a mouse click.
// A touch (a palm on the screen while holding the handheld) or anything else never starts one.
let lastA = 0;
let lastMouse = 0;
export function deliberateActivation(): boolean {
  const now = performance.now();
  return now - lastA < 800 || now - lastMouse < 800;
}

// Observers of every dispatched press (tools/journeys probe). They never consume a press.
const watchers = new Set<(b: PadButton, repeat: boolean) => void>();
export function onDispatch(f: (b: PadButton, repeat: boolean) => void): () => void {
  watchers.add(f);
  return () => watchers.delete(f);
}

export function dispatch(b: PadButton, repeat = false) {
  noteInput();
  watchers.forEach((f) => f(b, repeat));
  if (b === "a" && !repeat) lastA = performance.now();
  for (let i = stack.length - 1; i >= 0; i--) {
    if (stack[i](b, repeat)) return;
  }
  if (b === "up" || b === "down" || b === "left" || b === "right") move(b);
  else if (b === "lt" || b === "rt") jump(b === "rt");
  else if (b === "a" && !repeat) activate();
}

// ---- right stick ----
// Analog right stick, (x, y) in -1..1 with down positive; (0, 0) when released. Chart mode reads it.
type StickListener = (x: number, y: number) => void;
const stickListeners = new Set<StickListener>();
export function emitStick(x: number, y: number) {
  noteInput();
  stickListeners.forEach((f) => f(x, y));
}
export function useStick(cb: StickListener, active = true) {
  const ref = useRef(cb);
  ref.current = cb;
  useEffect(() => {
    if (!active) return;
    const f: StickListener = (x, y) => ref.current(x, y);
    stickListeners.add(f);
    return () => void stickListeners.delete(f);
  }, [active]);
}

/** Registers a handler for the component's lifetime. Return true to consume a press. */
export function usePadHandler(handler: Handler, active = true) {
  const ref = useRef(handler);
  ref.current = handler;
  useEffect(() => {
    if (!active) return;
    const h: Handler = (b, r) => ref.current(b, r);
    stack.push(h);
    return () => {
      const i = stack.lastIndexOf(h);
      if (i >= 0) stack.splice(i, 1);
    };
  }, [active]);
}

const KEYS: Record<string, PadButton> = {
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  Enter: "a",
  " ": "a",
  Escape: "b",
  Backspace: "b",
  x: "x",
  y: "y",
  q: "lb",
  e: "rb",
  Tab: "start",
  PageUp: "lt",
  PageDown: "rt",
  F18: "qam", // SuiPlay0X1 quick access key (gameos hwdb maps it to F18)
  F17: "guide", // SuiPlay0X1 "Aya Space" key
  Home: "guide",
  s: "select",
};

let started = false;

// ---- input arbitration ----
//
// On a handheld the same button press can arrive twice: once from the controller (read
// natively) and once as a keyboard key from Steam's Desktop Configuration, which also turns
// the sticks and trackpads into mouse moves, clicks and scroll-wheel events. Stick drift then
// shows up as random scrolling. Rules:
//   - Controller mode starts with any controller press and lasts until a real touch, or a
//     mouse click while the controller has been quiet for a while.
//   - In controller mode wheel and mouse clicks are swallowed (pages only scroll from focus).
//   - Keyboard keys that map to pad buttons are dropped while the controller was used in the
//     last 1.5 s, and any press that repeats the same button from another source within
//     150 ms is a duplicate.
// RARE_DEBUG_INPUT=1 logs every source and decision.

type Source = "pad" | "key" | "browserpad";
let padMode = false;
let lastPad = 0;
let last: { b: PadButton; at: number; src: Source } | null = null;
let debugInput = false;
const ilog = (m: string) => {
  if (debugInput) void import("../lib/bridge").then(({ native }) => native.debugLog(`input ${m}`).catch(() => {}));
};

function accept(b: PadButton, repeat: boolean, src: Source): boolean {
  const now = performance.now();
  if (src === "pad" || src === "browserpad") {
    lastPad = now;
    if (!padMode) ilog("controller mode on");
    padMode = true;
  } else if (now - lastPad < 1500) {
    ilog(`drop key ${b}: controller active`);
    return false;
  }
  if (!repeat && last && last.b === b && last.src !== src && now - last.at < 150) {
    ilog(`drop ${src} ${b}: duplicate of ${last.src}`);
    return false;
  }
  if (!repeat) last = { b, at: now, src };
  ilog(`${src} ${b}${repeat ? " (repeat)" : ""}`);
  return true;
}

export function startInput() {
  if (started) return;
  started = true;
  void import("../lib/bridge").then(({ native }) =>
    native
      .debugEnv()
      .then((e) => {
        debugInput = !!e.input;
      })
      .catch(() => {}),
  );
  window.addEventListener("keydown", (e) => {
    const t = e.target as HTMLElement;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA") && !["ArrowUp", "ArrowDown", "Escape", "Enter", "Tab"].includes(e.key)) return;
    // Volume keys are the launcher's (volkeys.rs reads them whatever has the screen): one step
    // per press, never a second one from here.
    if (e.key.startsWith("AudioVolume")) {
      e.preventDefault();
      return;
    }
    const b = KEYS[e.key];
    if (!b) return;
    e.preventDefault();
    // The handheld's own keys (quick access, Aya Space) never come from Steam's emulation.
    const own = e.key === "F17" || e.key === "F18";
    if (own || accept(b, e.repeat, "key")) dispatch(b, e.repeat);
  });
  // Wheel and trackpad scrolling: never in controller mode.
  window.addEventListener(
    "wheel",
    (e) => {
      if (!padMode) return;
      e.preventDefault();
      e.stopPropagation();
      ilog(`drop wheel ${Math.round(e.deltaY)}`);
    },
    { capture: true, passive: false },
  );
  const pointer = (e: PointerEvent | MouseEvent) => {
    // A pressed on a focused element clicks it from script; only real events are filtered.
    if (!e.isTrusted) return;
    const touch = "pointerType" in e && (e as PointerEvent).pointerType === "touch";
    if (e.type === "pointerdown" && "pointerType" in e && (e as PointerEvent).pointerType === "mouse") lastMouse = performance.now();
    const quiet = performance.now() - lastPad > 3000;
    if (touch || (quiet && e.type === "pointerdown")) {
      if (padMode) ilog(`controller mode off (${touch ? "touch" : "mouse click"})`);
      padMode = false;
      return;
    }
    if (padMode) {
      // A click synthesised from a trigger or trackpad while the controller is in use.
      e.preventDefault();
      e.stopPropagation();
      ilog(`drop ${e.type}`);
    }
  };
  for (const t of ["pointerdown", "mousedown", "click", "auxclick", "contextmenu"]) window.addEventListener(t, pointer as EventListener, { capture: true });
  void onPad((b, repeat) => {
    if (!accept(b, repeat, "pad")) return;
    if (b === "select" || b === "start") return chordable(b);
    dispatch(b, repeat);
  });
  void onPadStick((x, y) => {
    lastPad = performance.now();
    emitStick(x, y);
  });
  if (!isTauri) pollBrowserGamepads();
  // UI development and the scripted pad tests: window.__pad("a"), window.__stick(0.5, 0).
  if (import.meta.env.DEV) {
    Object.assign(window, {
      __pad: (b: PadButton, repeat = false) => dispatch(b, repeat),
      __stick: (x: number, y: number) => emitStick(x, y),
    });
  }
}

// ---- View + Menu ----
// Holding View + Menu (Select + Start) is the way out of a game (the native side opens the
// Running list after 0.6 s). Here a View or Menu press waits a moment; when the other one comes
// in that time, neither acts (Steam would open, or quick settings).
const CHORD_WINDOW = 180;
let pendingChord: { b: PadButton; timer: ReturnType<typeof setTimeout> } | null = null;

function chordable(b: PadButton) {
  if (pendingChord) {
    clearTimeout(pendingChord.timer);
    const other = pendingChord.b !== b;
    const first = pendingChord.b;
    pendingChord = null;
    if (other) {
      ilog("View + Menu: chord, neither acts");
      return;
    }
    dispatch(first);
  }
  pendingChord = {
    b,
    timer: setTimeout(() => {
      pendingChord = null;
      dispatch(b);
    }, CHORD_WINDOW),
  };
}

/** Programmatic clicks (A on a focused element) must not be swallowed by the click guard. */
export function inControllerMode() {
  return padMode;
}

const REPEAT_DELAY = 320;
/** Interval between repeats after `ms` of repeating: 120 ms easing down to 40 ms over ~1.2 s. */
export function repeatEvery(ms: number) {
  return Math.max(40, 120 - ms / 15);
}

/**
 * Browser Gamepad API, only for UI development outside Tauri (in the app the native side pushes
 * pad events). The frame loop runs only while a pad is connected and the page has the screen.
 */
function pollBrowserGamepads() {
  const order: PadButton[] = ["a", "b", "x", "y", "lb", "rb", "lt", "rt", "select", "start", "guide", "guide", "up", "down", "left", "right", "guide"];
  const prev = new Map<number, boolean[]>();
  const heldAt = new Map<string, number>();
  const lastRep = new Map<string, number>();
  let rsLive = false;
  const tick = () => {
    for (const gp of navigator.getGamepads?.() ?? []) {
      if (!gp) continue;
      const p = prev.get(gp.index) ?? [];
      const now = gp.buttons.map((b) => b.pressed);
      const stickY = gp.axes[1] ?? 0;
      const stickX = gp.axes[0] ?? 0;
      now[12] = now[12] || stickY < -0.6;
      now[13] = now[13] || stickY > 0.6;
      now[14] = now[14] || stickX < -0.6;
      now[15] = now[15] || stickX > 0.6;
      now.forEach((down, i) => {
        const b = order[i];
        if (!b) return;
        const key = `${gp.index}:${i}`;
        if (down && !p[i]) {
          if (accept(b, false, "browserpad")) dispatch(b, false);
          heldAt.set(key, performance.now());
        } else if (down && i >= 12 && i <= 15) {
          // Same repeat curve as the native side: a pause, then faster and faster.
          const t0 = heldAt.get(key) ?? 0;
          const held = performance.now() - t0;
          const last = lastRep.get(key) ?? t0;
          if (held > REPEAT_DELAY && performance.now() - last > repeatEvery(held - REPEAT_DELAY)) {
            if (accept(b, true, "browserpad")) dispatch(b, true);
            lastRep.set(key, performance.now());
          }
        } else if (!down) lastRep.delete(key);
      });
      const sx = gp.axes[2] ?? 0;
      const sy = gp.axes[3] ?? 0;
      const live = Math.hypot(sx, sy) > 0.25;
      if (live || rsLive) emitStick(live ? sx : 0, live ? sy : 0);
      rsLive = live;
      prev.set(gp.index, now);
    }
    loop = 0;
    schedule();
  };
  let loop = 0;
  const schedule = () => {
    const pads = (navigator.getGamepads?.() ?? []).some((g) => g);
    if (!loop && pads && activity.get() !== "away") loop = requestAnimationFrame(tick);
  };
  window.addEventListener("gamepadconnected", schedule);
  // Back from away: pick the loop up again (a press can't wake it while it is stopped).
  activity.subscribe(schedule);
  schedule();
}
