// Small UI sounds and controller rumble on move, select and back. Sounds are synthesised
// (no audio files); rumble goes to the native side, which plays a short force-feedback
// effect on pads that support it. Both can be turned off in Settings.

import { isTauri } from "../lib/bridge";

/**
 * The sound and haptic map. Every UI event the shell
 * plays goes through feedback(kind). Sounds are short synthesised blips under 120 ms; rumble
 * reuses the three native effects (move, select, back) so the Rust side stays unchanged.
 */
export type Feedback = "move" | "select" | "back" | "tab" | "open" | "close" | "launch" | "toggle" | "error" | "notify" | "boot";

/** Which native rumble each kind uses (null: no rumble). */
const RUMBLE: Record<Feedback, "move" | "select" | "back" | null> = {
  move: "move",
  select: "select",
  back: "back",
  tab: "move",
  open: "select",
  close: "back",
  launch: "select",
  toggle: "move",
  error: "back",
  notify: null,
  boot: null,
};

// Sounds only in the real app: dev:ui, mock screenshots and headless test browsers stay silent
// (they ran on the dev Mac and played navigation ticks through its speakers).
let soundsOn = isTauri;
let rumbleOn = true;
export function setFeedback(opts: { sounds?: boolean | null; rumble?: boolean | null }) {
  if (opts.sounds != null) soundsOn = isTauri && opts.sounds;
  if (opts.rumble != null) rumbleOn = opts.rumble;
}

let ctx: AudioContext | null = null;
let master: GainNode | null = null;
function audio(): AudioContext | null {
  if (ctx) return ctx;
  try {
    // 48 kHz matches the PipeWire graph; WebKitGTK otherwise opens at 44.1 kHz and the graph resamples.
    ctx = new AudioContext({ latencyHint: "interactive", sampleRate: 48000 });
    master = ctx.createGain();
    master.gain.value = 0.32;
    master.connect(ctx.destination);
  } catch {
    ctx = null;
  }
  return ctx;
}

type Tone = { f: number; to: number; ms: number; vol: number; type: OscillatorType; at?: number };
const TONES: Record<Feedback, Tone[]> = {
  move: [{ f: 1320, to: 1320, ms: 28, vol: 0.18, type: "sine" }],
  select: [{ f: 880, to: 1480, ms: 70, vol: 0.32, type: "triangle" }],
  back: [{ f: 740, to: 420, ms: 70, vol: 0.28, type: "triangle" }],
  // A tab change: a softer, lower tick than a move so LB/RB feel like a shelf turning.
  tab: [{ f: 990, to: 1180, ms: 45, vol: 0.2, type: "sine" }],
  // Overlays: rising when something opens over the page, falling when it closes.
  open: [{ f: 660, to: 990, ms: 90, vol: 0.24, type: "sine" }],
  close: [{ f: 880, to: 560, ms: 80, vol: 0.2, type: "sine" }],
  // Play: two rising notes, the one sound that says "something big is starting".
  launch: [
    { f: 660, to: 660, ms: 70, vol: 0.26, type: "triangle" },
    { f: 990, to: 1320, ms: 140, vol: 0.3, type: "triangle", at: 0.07 },
  ],
  toggle: [{ f: 1100, to: 1250, ms: 35, vol: 0.2, type: "triangle" }],
  error: [
    { f: 330, to: 300, ms: 70, vol: 0.26, type: "square" },
    { f: 300, to: 260, ms: 90, vol: 0.22, type: "square", at: 0.09 },
  ],
  // A sale or price alert arriving: one quiet bell, never on every poll.
  notify: [{ f: 1568, to: 1568, ms: 110, vol: 0.14, type: "sine" }],
  // Startup: a fifth that opens up, once per boot.
  boot: [
    { f: 523, to: 523, ms: 160, vol: 0.2, type: "sine" },
    { f: 784, to: 784, ms: 260, vol: 0.2, type: "sine", at: 0.12 },
  ],
};

function tone(kind: Feedback) {
  const c = audio();
  if (!c || !master) return;
  if (c.state === "suspended") void c.resume().catch(() => {});
  for (const t of TONES[kind]) {
    const now = c.currentTime + (t.at ?? 0);
    const o = c.createOscillator();
    const g = c.createGain();
    o.type = t.type;
    o.frequency.setValueAtTime(t.f, now);
    if (t.to !== t.f) o.frequency.exponentialRampToValueAtTime(t.to, now + t.ms / 1000);
    g.gain.setValueAtTime(0.0001, now);
    g.gain.exponentialRampToValueAtTime(t.vol, now + 0.004);
    g.gain.exponentialRampToValueAtTime(0.0001, now + t.ms / 1000);
    o.connect(g).connect(master);
    o.start(now);
    o.stop(now + t.ms / 1000 + 0.02);
  }
}

let lastMove = 0;
export function feedback(kind: Feedback) {
  const now = performance.now();
  // Held directions repeat quickly; keep the ticks from turning into a buzz.
  if (kind === "move") {
    if (now - lastMove < 45) return;
    lastMove = now;
  }
  if (soundsOn) tone(kind);
  const r = RUMBLE[kind];
  if (rumbleOn && isTauri && r) {
    void import("@tauri-apps/api/core").then(({ invoke }) => invoke("pad_rumble", { kind: r }).catch(() => {}));
  }
}
