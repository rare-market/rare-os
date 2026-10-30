// Presence in words, pure (format.test.ts).

import type { Presence } from "./types";

const mins = (ms: number) => {
  const m = Math.max(1, Math.round(ms / 60_000));
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60 ? `${m % 60} min` : ""}`.trim();
};

export function ago(t: number, now = Date.now()) {
  const s = Math.max(0, (now - t) / 1000);
  if (s < 120) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  if (s < 86400 * 2) return "yesterday";
  return `${Math.round(s / 86400)} days ago`;
}

/** "Playing ACT", "Online", "Away", "Last seen 3 h ago", "Offline". `long` adds how long. */
export function presenceText(p: Presence, long = false, now = Date.now()): string {
  if (p.state === "offline") return p.lastSeen ? `Last seen ${ago(p.lastSeen, now)}` : "Offline";
  if (p.state === "away") return p.game ? `Away, in ${p.game.title}` : "Away";
  if (p.game) return long && p.since ? `Playing ${p.game.title} for ${mins(now - p.since)}` : `Playing ${p.game.title}`;
  return "Online";
}
