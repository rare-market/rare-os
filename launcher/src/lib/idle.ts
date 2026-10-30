// How busy the launcher should be. Three states:
//   active: the launcher has the screen and was used in the last minute;
//   idle:   the launcher has the screen, nobody touched it for IDLE_MS;
//   away:   a game or another window has the screen, or the page is hidden.
// Looping animations stop outside active (html[data-activity], app.css) and data polls slow
// down when idle and stop when away (`poll`). Any input makes it active again at once, and a
// poll that is overdue for the active cadence runs right then, so nothing looks stale in use.

export type Activity = "active" | "idle" | "away";

/** No input for this long: idle. */
export const IDLE_MS = 60_000;

/** The state for these signals at `now`. Pure (tests). */
export function activityAt(s: { lastInput: number; visible: boolean; focused: boolean }, now: number, idleMs = IDLE_MS): Activity {
  if (!s.visible || !s.focused) return "away";
  return now - s.lastInput >= idleMs ? "idle" : "active";
}

type Listener = (a: Activity, prev: Activity) => void;

/** The state machine, with the clock and timers passed in (tests use fake timers). */
export function createActivity(opts: { now?: () => number; idleMs?: number } = {}) {
  const now = opts.now ?? (() => Date.now());
  let idleMs = opts.idleMs ?? IDLE_MS;
  const sig = { lastInput: now(), visible: true, focused: true };
  let state: Activity = "active";
  let timer: ReturnType<typeof setTimeout> | null = null;
  const listeners = new Set<Listener>();

  function update() {
    const next = activityAt(sig, now(), idleMs);
    if (timer) clearTimeout(timer);
    timer = null;
    // Active: wake once when it turns idle (one timer, re-armed on input, never a poll).
    if (next === "active") timer = setTimeout(update, Math.max(0, sig.lastInput + idleMs - now()));
    if (next === state) return;
    const prev = state;
    state = next;
    for (const l of [...listeners]) l(next, prev);
  }

  update();
  return {
    get: () => state,
    /** A press, key, click or stick move. Input only reaches the page while it has the screen. */
    input() {
      sig.lastInput = now();
      sig.focused = true;
      sig.visible = true;
      // Already active: the idle timer is only re-armed when it fires (cheap on a held stick).
      if (state !== "active") update();
    },
    setVisible(v: boolean) {
      sig.visible = v;
      update();
    },
    setFocused(f: boolean) {
      sig.focused = f;
      update();
    },
    setIdleMs(ms: number) {
      idleMs = ms;
      update();
    },
    subscribe(l: Listener) {
      listeners.add(l);
      return () => void listeners.delete(l);
    },
    /** Re-evaluates now (the idle timer calls this too). */
    update,
    stop() {
      if (timer) clearTimeout(timer);
      timer = null;
    },
  };
}

export type ActivityMachine = ReturnType<typeof createActivity>;

/** Per-state interval in ms; null stops the poll in that state. */
export interface Cadence {
  active: number;
  idle: number | null;
  away: number | null;
}

/** Milliseconds until the next run, or null when the state stops the poll. Pure (tests). */
export function dueIn(lastRun: number, now: number, cadence: Cadence, state: Activity): number | null {
  const every = cadence[state];
  if (every == null) return null;
  return Math.max(0, lastRun + every - now);
}

/**
 * Runs `fn` on the cadence of the current state. One timer at a time; a state change re-plans
 * it, so coming back (input, focus) runs an overdue poll at once. Returns a stop function.
 */
export function pollWith(machine: ActivityMachine, fn: () => unknown, cadence: Cadence, opts: { now?: () => number; runNow?: boolean } = {}): () => void {
  const now = opts.now ?? (() => Date.now());
  let last = now();
  let timer: ReturnType<typeof setTimeout> | null = null;
  let stopped = false;
  const run = () => {
    last = now();
    try {
      const r = fn();
      if (r && typeof (r as Promise<unknown>).catch === "function") (r as Promise<unknown>).catch(() => {});
    } catch {
      /* a failed poll waits for the next one */
    }
  };
  const plan = () => {
    if (timer) clearTimeout(timer);
    timer = null;
    if (stopped) return;
    const ms = dueIn(last, now(), cadence, machine.get());
    if (ms == null) return;
    timer = setTimeout(() => {
      run();
      plan();
    }, ms);
  };
  if (opts.runNow) run();
  plan();
  const unsub = machine.subscribe(plan);
  return () => {
    stopped = true;
    unsub();
    if (timer) clearTimeout(timer);
  };
}

/** Milliseconds to the next whole minute (plus a little, so the clock reads the new minute). */
export function msToNextMinute(now: number): number {
  return 60_000 - (now % 60_000) + 50;
}

// ---- the launcher's own machine, bound to the page ----

export const activity = createActivity();

/** Runs `fn` on the launcher's activity cadence (see pollWith). */
export function poll(fn: () => unknown, cadence: Cadence, opts: { runNow?: boolean } = {}) {
  return pollWith(activity, fn, cadence, opts);
}

/** Marks input (called by nav/input for pad, stick and keys, and by the page listeners). */
export function noteInput() {
  activity.input();
}

/** A game Rare started takes the screen: away until focus or input says the launcher is back. */
export function gameTookScreen() {
  activity.setFocused(false);
}

// Catch-all for loops the CSS list in app.css doesn't name: every running animation that never
// ends is paused outside active and played again on the way back.
const pausedLoops = new Set<Animation>();
function sweep(a: Activity) {
  if (typeof document.getAnimations !== "function") return;
  if (a === "active") {
    for (const x of pausedLoops) if (x.playState === "paused") x.play();
    pausedLoops.clear();
    return;
  }
  for (const x of document.getAnimations()) {
    if (x.playState === "running" && x.effect?.getTiming().iterations === Infinity) {
      x.pause();
      pausedLoops.add(x);
    }
  }
}

/**
 * One journal line per drop out of active (`[ui] activity idle: ...`): what still animates
 * (should be nothing) and what was paused. For checking on the device without devtools.
 */
function report(a: Activity) {
  const left = typeof document.getAnimations === "function" ? document.getAnimations().filter((x) => x.playState === "running") : [];
  const name = (x: Animation) => (x as CSSAnimation).animationName ?? x.constructor.name;
  const msg = `activity ${a}: ${left.length} running animation(s)${left.length ? ` (${left.slice(0, 5).map(name).join(", ")})` : ""}, ${pausedLoops.size} loop(s) paused`;
  void import("./bridge").then(({ native }) => native.debugLog(msg)).catch(() => {});
}

let started = false;
/**
 * Binds the machine to the page: input events, focus, visibility, and the Tauri window's focus
 * (a game in front takes it). Writes the state to <html data-activity> for the CSS.
 * Dev and screenshots: ?idlems=5000 shortens the idle time.
 */
export function startActivity() {
  if (started || typeof window === "undefined") return;
  started = true;
  const root = document.documentElement;
  const show = (a: Activity) => {
    if (a === "active") delete root.dataset.activity;
    else root.dataset.activity = a;
  };
  activity.subscribe((a, prev) => {
    show(a);
    sweep(a);
    if (prev === "active") report(a);
  });
  const q = new URLSearchParams(location.search).get("idlems");
  if (q && Number(q) > 0) activity.setIdleMs(Number(q));
  for (const t of ["keydown", "pointerdown", "wheel", "touchstart"]) window.addEventListener(t, noteInput, { capture: true, passive: true });
  document.addEventListener("visibilitychange", () => activity.setVisible(document.visibilityState !== "hidden"));
  window.addEventListener("focus", () => activity.setFocused(true));
  window.addEventListener("blur", () => activity.setFocused(false));
  activity.setVisible(document.visibilityState !== "hidden");
  if ("__TAURI_INTERNALS__" in window) {
    // In gamescope the window's own focus is what says a game has the screen.
    void import("@tauri-apps/api/window")
      .then(async ({ getCurrentWindow }) => {
        const w = getCurrentWindow();
        const f = await w.isFocused().catch(() => null);
        if (f != null) activity.setFocused(f);
        await w.onFocusChanged(({ payload }) => activity.setFocused(payload));
      })
      .catch(() => {});
  } else if (!document.hasFocus()) {
    // A browser tab opened in the background (dev:ui): wait for focus. Headless pages report
    // no focus forever, so only a real blur counts there.
    if (!navigator.webdriver) activity.setFocused(false);
  }
  if (import.meta.env?.DEV) Object.assign(window, { __activity: activity });
}
