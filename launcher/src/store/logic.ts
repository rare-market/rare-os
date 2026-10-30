// Pure Store decisions, shared by the screens and the tests. The Rust side makes the same update
// and grace decisions again before it installs or launches (src-tauri/src/store).

import type { ChainBuild, ChainRelease, Installed, Licence, StoreGame } from "./types";

export const DAY_MS = 86_400_000;
/** A positive launch check keeps a game launching offline this long. */
export const LAUNCH_GRACE_MS = 7 * DAY_MS;

export type UpdateDecision = "install" | "update" | "current" | "unavailable";

/** Install when nothing is installed, Update only when the chain has a strictly higher version for the edition. */
export function updateDecision(installed: Pick<Installed, "edition" | "version"> | null | undefined, release: Pick<ChainRelease, "version" | "edition"> | null | undefined): UpdateDecision {
  if (!release) return installed ? "current" : "unavailable";
  if (!installed) return "install";
  if (installed.edition !== release.edition) return "update";
  return release.version > installed.version ? "update" : "current";
}

/** The build this device should download: the first platform it prefers that the release has. */
export function pickBuild(release: ChainRelease, platforms: string[]): ChainBuild | null {
  for (const p of platforms) {
    const b = release.builds.find((x) => x.platform === p);
    if (b) return b;
  }
  return null;
}

export function refundable(l: Pick<Licence, "refundable_until_ms" | "type">, now = Date.now()): boolean {
  return l.type === "licence" && l.refundable_until_ms != null && now < l.refundable_until_ms;
}

/** Make tradable is possible now (resale on, hold over, still a plain licence). */
export function tradableNow(l: Pick<Licence, "tradable_from_ms" | "type">, now = Date.now()): boolean {
  return l.type === "licence" && l.tradable_from_ms != null && now >= l.tradable_from_ms;
}

/** The offline grace: a check passed within the last 7 days. */
export function withinGrace(checkedMs: number, now = Date.now()): boolean {
  return checkedMs > 0 && now - checkedMs <= LAUNCH_GRACE_MS;
}

/** The price a purchase pays now, in micro-USD (game::current_price_usd on the API's numbers). */
export function priceNow(g: Pick<StoreGame, "price">, now = Date.now()): bigint | null {
  if (!g.price) return null;
  const sale = g.price.sale_usd ? BigInt(g.price.sale_usd) : 0n;
  if (sale > 0n && g.price.sale_ends_ms != null && now < g.price.sale_ends_ms) return sale;
  return BigInt(g.price.usd);
}

export function onSale(g: Pick<StoreGame, "price">, now = Date.now()): boolean {
  const p = priceNow(g, now);
  return p != null && !!g.price && p < BigInt(g.price.usd);
}

/** micro-USD as dollars ("$14.99", "Free"). */
export function usdText(micro: bigint): string {
  if (micro === 0n) return "Free";
  const cents = (micro + 5_000n) / 10_000n;
  const d = cents / 100n;
  const c = cents % 100n;
  return `$${d.toLocaleString("en-US")}.${c.toString().padStart(2, "0")}`;
}

/** "2 h", "1 day", "3 days". */
export function timeLeft(ms: number): string {
  const h = Math.max(1, Math.round(ms / 3_600_000));
  if (h < 48) return `${h} h`;
  const d = Math.round(h / 24);
  return `${d} ${d === 1 ? "day" : "days"}`;
}

export function steamAppId(url: string | undefined): number | null {
  const m = url?.match(/store\.steampowered\.com\/app\/(\d+)/);
  return m ? Number(m[1]) : null;
}

const PLATFORM: Record<string, string> = { "linux-x86_64": "Linux", "windows-x86_64": "Windows", web: "Web", pc: "PC", mobile: "Mobile", suiplay: "SuiPlay" };
export const platformText = (ps: string[]) => Array.from(new Set(ps.map((p) => PLATFORM[p] ?? p))).join(", ");

/** Bytes as "734 MB" / "1.2 GB". */
export const sizeText = (b: number | bigint) => {
  const n = Number(b);
  return n >= 1e9 ? `${(n / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(n / 1e6))} MB`;
};
