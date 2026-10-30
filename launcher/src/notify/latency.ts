// Delivery latency per live notice, measured against the notice's checkpoint time (cpTs):
//   server: sentAt - cpTs        (rare.market's indexer and stream, by the server clock)
//   recv:   Date.now() - cpTs    when the frame reaches the webview
//   paint:  Date.now() - cpTs    in the frame after the toast mounted (null when no toast showed)
// The last 50 are kept in window.__rareNotifyLatency (same array, oldest first) and each one is
// logged to the console with the running p50 and p95.

import { percentile } from "./logic";
import type { Notice } from "./types";

export interface LatencySample {
  id: string;
  kind: string;
  cpTs: number;
  serverMs: number | null;
  recvMs: number;
  paintMs: number | null;
  /** server clock minus ours at hello, ms (positive: the server is ahead). */
  skewMs: number | null;
}

const CAP = 50;
const samples: LatencySample[] = [];
let skew: number | null = null;

declare global {
  interface Window {
    __rareNotifyLatency?: LatencySample[];
  }
}
if (typeof window !== "undefined") window.__rareNotifyLatency = samples;

export function setSkew(serverNow: number) {
  if (Number.isFinite(serverNow)) skew = serverNow - Date.now();
}

function log(s: LatencySample, what: "recv" | "paint") {
  const recv = samples.map((x) => x.recvMs);
  const paint = samples.map((x) => x.paintMs).filter((x): x is number => x != null);
  const p = (v: number[]) => `p50 ${percentile(v, 50) ?? "-"} p95 ${percentile(v, 95) ?? "-"}`;
  console.info(
    `[notify] ${s.id} ${s.kind} ${what} ${what === "recv" ? s.recvMs : s.paintMs} ms (server ${s.serverMs ?? "-"} ms, skew ${skew ?? "-"} ms) | recv ${p(recv)} | paint ${p(paint)} | n=${samples.length}`,
  );
}

export function recordReceipt(n: Notice) {
  if (!n.cpTs || samples.some((s) => s.id === n.id)) return;
  const s: LatencySample = { id: n.id, kind: n.kind, cpTs: n.cpTs, serverMs: n.sentAt ? n.sentAt - n.cpTs : null, recvMs: Date.now() - n.cpTs, paintMs: null, skewMs: skew };
  samples.push(s);
  if (samples.length > CAP) samples.splice(0, samples.length - CAP);
  log(s, "recv");
}

export function recordPaint(id: string) {
  const s = samples.find((x) => x.id === id);
  if (!s || s.paintMs != null) return;
  s.paintMs = Date.now() - s.cpTs;
  log(s, "paint");
}

export const latencySamples = () => samples;
