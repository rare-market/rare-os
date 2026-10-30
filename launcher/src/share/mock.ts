// Mock native Share side for browser development and screenshots. Screenshots are drawn on a
// canvas and encoded to WebP; a clip is a few seconds of canvas animation recorded with
// MediaRecorder (MP4 where Chrome can, else WebM), shared by every mock clip.
//   ?shareclips=off  the clip recorder is missing
//   ?shareempty      no captures yet

import type { Capture, ShareStatus } from "./native";

const params = typeof location !== "undefined" ? new URLSearchParams(location.search) : new URLSearchParams();
const H = 3_600_000;

let state: ShareStatus = {
  session: "gamescope",
  screenshot: { available: true, method: "gamescope" },
  clips:
    params.get("shareclips") === "off"
      ? { available: false, enabled: false, recording: false, method: null, seconds: 30 }
      : { available: true, enabled: true, recording: true, method: "gpu-screen-recorder", seconds: 30 },
  dir: "/home/rare/Pictures/Rare",
};

interface Scene {
  game: { id: string; name: string };
  hue: number;
}
const SCENES: Scene[] = [
  { game: { id: "steam:1145350", name: "Hades II" }, hue: 18 },
  { game: { id: "web:act", name: "ACT" }, hue: 200 },
  { game: { id: "steam:2379780", name: "Balatro" }, hue: 340 },
  { game: { id: "steam:504230", name: "Celeste" }, hue: 265 },
  { game: { id: "web:wordgame", name: "Word Game" }, hue: 150 },
];

function paint(ctx: CanvasRenderingContext2D, w: number, h: number, s: Scene, t = 0) {
  const g = ctx.createLinearGradient(0, 0, w, h);
  g.addColorStop(0, `hsl(${s.hue} 70% 22%)`);
  g.addColorStop(1, `hsl(${(s.hue + 50) % 360} 60% 8%)`);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, w, h);
  for (let i = 0; i < 14; i++) {
    const x = ((i * 337 + t * (40 + i * 6)) % (w + 400)) - 200;
    const y = h * (0.25 + 0.5 * Math.abs(Math.sin(i * 1.7 + t * 0.6)));
    const r = (h / 14) * (1 + (i % 4));
    ctx.fillStyle = `hsl(${(s.hue + i * 18) % 360} 80% ${45 + (i % 3) * 10}% / 0.35)`;
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.fill();
  }
  // A health bar, so it reads as a game.
  ctx.fillStyle = "rgb(0 0 0 / 0.45)";
  ctx.fillRect(w * 0.04, h * 0.06, w * 0.22, h * 0.03);
  ctx.fillStyle = `hsl(${(s.hue + 120) % 360} 70% 55%)`;
  ctx.fillRect(w * 0.04, h * 0.06, w * 0.15, h * 0.03);
}

function canvasOf(w: number, h: number) {
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  return c;
}

const bytes = new Map<string, Promise<ArrayBuffer>>();
const scenes = new Map<string, Scene>();

function shot(s: Scene): Promise<ArrayBuffer> {
  const c = canvasOf(1600, 900);
  paint(c.getContext("2d")!, c.width, c.height, s);
  return new Promise((res, rej) => c.toBlob((b) => (b ? res(b.arrayBuffer()) : rej(new Error("canvas"))), "image/webp", 0.9));
}

let clipBytes: Promise<ArrayBuffer> | null = null;
function recordClip(): Promise<ArrayBuffer> {
  clipBytes ??= new Promise<ArrayBuffer>((res, rej) => {
    const c = canvasOf(960, 540);
    const ctx = c.getContext("2d")!;
    const s = SCENES[1];
    const type = ["video/mp4;codecs=avc1", "video/mp4", "video/webm"].find((t) => MediaRecorder.isTypeSupported(t));
    const rec = new MediaRecorder(c.captureStream(30), type ? { mimeType: type, videoBitsPerSecond: 2_500_000 } : undefined);
    const parts: Blob[] = [];
    rec.ondataavailable = (e) => e.data.size && parts.push(e.data);
    rec.onstop = () => void new Blob(parts).arrayBuffer().then(res, rej);
    const t0 = performance.now();
    const tick = setInterval(() => paint(ctx, c.width, c.height, s, (performance.now() - t0) / 1000), 33);
    paint(ctx, c.width, c.height, s);
    rec.start(500);
    setTimeout(() => {
      clearInterval(tick);
      rec.stop();
    }, 4000);
  });
  return clipBytes;
}

let seq = 0;
function make(kind: Capture["kind"], s: Scene, createdMs: number): Capture {
  const id = `${kind === "clip" ? "clip" : "shot"}-${createdMs}-${++seq}`;
  scenes.set(id, s);
  return {
    id,
    kind,
    path: `${state.dir}/${id}.${kind === "clip" ? "mp4" : "webp"}`,
    mime: kind === "clip" ? "video/mp4" : "image/webp",
    // A real 30 second clip and a 1080p WebP are about this size.
    bytes: kind === "clip" ? 14_800_000 : 640_000 + (createdMs % 300_000),
    width: kind === "clip" ? 1920 : 1920,
    height: 1080,
    durationMs: kind === "clip" ? 30_000 : null,
    createdMs,
    game: s.game,
  };
}

let list_: Capture[] | null = null;
function all(): Capture[] {
  if (!list_) {
    const now = Date.now();
    list_ = params.has("shareempty")
      ? []
      : [
          make("screenshot", SCENES[0], now - 0.2 * H),
          make("clip", SCENES[1], now - 0.5 * H),
          make("screenshot", SCENES[2], now - 3 * H),
          make("screenshot", SCENES[1], now - 5 * H),
          make("clip", SCENES[0], now - 26 * H),
          make("screenshot", SCENES[3], now - 50 * H),
          make("screenshot", SCENES[4], now - 75 * H),
        ];
  }
  return list_;
}

const listeners = new Set<(c: Capture) => void>();

export const status = async () => state;
export async function setClips(enabled: boolean) {
  state = { ...state, clips: { ...state.clips, enabled, recording: enabled && state.clips.available } };
  return state;
}
export async function list() {
  return [...all()].sort((a, b) => b.createdMs - a.createdMs);
}
async function add(kind: Capture["kind"]) {
  await new Promise((r) => setTimeout(r, 300));
  const c = make(kind, SCENES[(seq + 2) % SCENES.length], Date.now());
  all().unshift(c);
  listeners.forEach((l) => l(c));
  return c;
}
export const screenshot = () => add("screenshot");
export async function clip() {
  if (!state.clips.available) throw new Error("Clips need gpu-screen-recorder or GStreamer with VA-API");
  if (!state.clips.enabled) throw new Error("Turn on Keep the last 30 seconds first");
  return add("clip");
}
export async function read(id: string): Promise<ArrayBuffer> {
  const s = scenes.get(id);
  if (!s) throw new Error("No such capture");
  let b = bytes.get(id);
  if (!b) bytes.set(id, (b = id.startsWith("clip") ? recordClip() : shot(s)));
  return b;
}
export async function remove(id: string) {
  list_ = all().filter((c) => c.id !== id);
  bytes.delete(id);
  scenes.delete(id);
}
export function onCaptured(cb: (c: Capture) => void) {
  listeners.add(cb);
  return () => void listeners.delete(cb);
}
