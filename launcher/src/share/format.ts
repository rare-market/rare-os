// The share format rare.market's share page reads (rare.market/s/<quiltId>). One Walrus quilt per
// share: share.json, the media (shot.webp or clip.mp4) and, for clips, poster.jpg. Pure functions
// only, so the format is unit tested; the upload is in ./upload.

import type { Network } from "../sui/chain";
import { epochsFor, EPOCH_DAYS } from "../sui/saves/network";
import type { SuiGame } from "../lib/rare";
import type { Game, Library } from "../lib/types";
import { installedNative, profileForUrl } from "../games/catalog";
import type { Capture, CaptureKind } from "./native";

export const SHARE_FORMAT = "rare.share/1";
export const SHARE_APP_TAG = "rare-share";
export const SHARE_BASE = "https://rare.market/s/";

export interface ShareJson {
  format: typeof SHARE_FORMAT;
  kind: CaptureKind;
  file: "shot.webp" | "clip.mp4";
  mime: "image/webp" | "video/mp4";
  width: number;
  height: number;
  durationMs: number | null;
  /** size of the media file */
  bytes: number;
  poster: "poster.jpg" | null;
  game: { name: string; slug: string | null } | null;
  /** unix ms */
  created: number;
}

export const mediaFile = (kind: CaptureKind) => (kind === "clip" ? ("clip.mp4" as const) : ("shot.webp" as const));
export const mediaMime = (kind: CaptureKind) => (kind === "clip" ? ("video/mp4" as const) : ("image/webp" as const));

/** share.json for a capture. Never carries the player's address: the site reads the uploader from chain. */
export function buildShareJson(c: Capture, o: { bytes: number; slug: string | null; poster: boolean }): ShareJson {
  return {
    format: SHARE_FORMAT,
    kind: c.kind,
    file: mediaFile(c.kind),
    mime: mediaMime(c.kind),
    width: c.width,
    height: c.height,
    durationMs: c.kind === "clip" ? c.durationMs : null,
    bytes: o.bytes,
    poster: c.kind === "clip" && o.poster ? "poster.jpg" : null,
    game: c.game ? { name: c.game.name, slug: o.slug } : null,
    created: c.createdMs,
  };
}

/** The public link for a share. */
export function shareLink(quiltId: string, network: Network): string {
  return `${SHARE_BASE}${quiltId}${network === "testnet" ? "?n=testnet" : ""}`;
}

/** Blob attributes, so a later "My shares" list can find them on chain. */
export const shareAttributes = (kind: CaptureKind): Record<string, string> => ({ app: SHARE_APP_TAG, v: "1", kind });

export type DurationId = "month" | "year";
export const DURATIONS: Array<{ id: DurationId; label: string; days: number }> = [
  { id: "month", label: "1 month", days: 30 },
  { id: "year", label: "1 year", days: 365 },
];

/** Walrus epochs and the days they really buy (mainnet epochs are 14 days, so 1 month is 42 days). */
export function shareEpochs(id: DurationId, network: Network): { epochs: number; days: number } {
  const days = DURATIONS.find((d) => d.id === id)!.days;
  const epochs = epochsFor(days, network);
  return { epochs, days: epochs * EPOCH_DAYS[network] };
}

/**
 * Quilt size when the SDK's encoder isn't at hand: the files plus the quilt's column padding and
 * index. Measured against encodeQuilt on 1000 shards: 3 to 10 percent for 1 to 60 MB, a fixed
 * ~250 KB for small files. Errs high so a price is never low.
 */
export function estimateQuiltBytes(sizes: number[]): number {
  const sum = sizes.reduce((a, b) => a + b, 0);
  return Math.ceil(sum * 1.04) + 262_144;
}

/** Long edge capped at `max`, aspect kept, whole pixels. */
export function fitWithin(w: number, h: number, max = 1280): { w: number; h: number } {
  if (w <= 0 || h <= 0) return { w: max, h: Math.round((max * 9) / 16) };
  const s = Math.min(1, max / Math.max(w, h));
  return { w: Math.max(1, Math.round(w * s)), h: Math.max(1, Math.round(h * s)) };
}

/** Where to grab a clip's poster: about 1 s in, or 40 percent of a shorter clip. */
export function posterTime(durationS: number): number {
  if (!Number.isFinite(durationS) || durationS <= 0) return 0;
  return durationS >= 2.5 ? 1 : durationS * 0.4;
}

const norm = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, "");

/**
 * rare.market's slug for the game in a capture, where the launcher knows it: built-in and added
 * web games by id or URL host, native Sui titles by Steam app id or title. Steam and other games
 * that aren't Sui games get null and send just their name.
 */
export function slugFor(game: Capture["game"], catalog: SuiGame[], library: Library | null): string | null {
  if (!game) return null;
  const known = new Set(catalog.map((g) => g.slug));
  if (game.id.startsWith("web:")) {
    const id = game.id.slice(4);
    if (known.has(id)) return id;
    const lib: Game | undefined = library?.games.find((g) => g.id === game.id);
    const p = profileForUrl(id, lib?.launch?.url);
    if (p) {
      const hit = catalog.find((g) => profileForUrl(g.slug) === p);
      if (hit) return hit.slug;
    }
  }
  for (const g of catalog) {
    const installed = installedNative(g.slug, g.name, library);
    if (installed?.id === game.id) return g.slug;
  }
  return catalog.find((g) => norm(g.name) === norm(game.name))?.slug ?? null;
}

/** "0:30", "1:05" */
export function fmtDuration(ms: number | null | undefined): string {
  if (!ms) return "";
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}
