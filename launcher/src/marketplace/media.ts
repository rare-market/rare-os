// Image chains for marketplace art, the same order as rare.market's web/lib/media.ts (wsrv mode):
// rare's cached image front, wsrv.nl directly, the original URLs, then rare's passthrough. The last
// link is a quiet tile (see MImg). Relative /img/... paths from the API are served by rare.market.

import { RARE } from "../lib/rare";
import { artSrc, sizeFor } from "../lib/state";
import { candidates, inlineImage, parseRef, wsrvBlocked } from "./venues-media";

const IMG_W = `${RARE}/img/w/i.webp`;
const WSRV = "https://wsrv.nl/";

export interface ImgOpts {
  /** "cover" crops to a square (grids), "inside" keeps the aspect (item page). */
  fit?: "cover" | "inside";
}

function query(url: string, width: number, opts: ImgOpts): string {
  const w = Math.max(16, Math.min(2048, Math.round(width)));
  const fit = opts.fit ?? "cover";
  return `?url=${encodeURIComponent(url)}&w=${w}&h=${w}&fit=${fit}&output=webp&q=80${fit === "inside" ? "&we" : ""}`;
}

const proxyable = (url: string) => /^https?:\/\//i.test(url) && !wsrvBlocked(url);
const raw = (input: unknown) => (typeof input === "string" ? inlineImage(input.trim()) : "");

function mediaCandidates(input: unknown): string[] {
  const r = raw(input);
  if (!r || r === "null" || r === "undefined") return [];
  if (r.startsWith("/") && !r.startsWith("//") && !r.startsWith("/ipfs/")) return [`${RARE}${r}`];
  const ref = parseRef(r);
  if (ref.kind === "data") return /^image\//.test(ref.mime) ? [ref.uri] : [];
  return candidates(ref);
}

/** Ordered sources to try for one image, duplicates removed. In the app each link goes through Rust's
 *  image cache (lib/state.ts artSrc): fetched once, kept on disk and in memory at this width. */
export function imageChain(input: unknown, width = 400, opts: ImgOpts = {}): string[] {
  const size = sizeFor(width);
  return chain(input, width, opts).map((u) => (u.startsWith("data:") ? u : artSrc(u, size)));
}

function chain(input: unknown, width: number, opts: ImgOpts): string[] {
  const r = raw(input);
  const pass = r.match(/^\/img\/pass\?url=([^&]+)/);
  if (pass) {
    let src = "";
    try {
      src = decodeURIComponent(pass[1]);
    } catch {
      /* keep the link alone */
    }
    return [...new Set([`${RARE}/img/pass?url=${pass[1]}`, ...(src ? mediaCandidates(src).slice(0, 2) : [])])];
  }
  const cands = mediaCandidates(input);
  if (!cands.length) return [];
  const c0 = cands[0];
  if (c0.startsWith("data:") || c0.startsWith(RARE)) return [c0];
  const direct = cands.slice(0, 2);
  const front = direct.filter(proxyable).map((u) => `${IMG_W}${query(u, width, opts)}`);
  if (proxyable(c0)) front.splice(1, 0, `${WSRV}${query(c0, width, opts)}`);
  // Hosts wsrv refuses (.xyz, .fun, ...) often serve AVIF only: the passthrough converts it to WebP, so it goes before the raw original.
  const passLink = `${RARE}/img/pass?url=${encodeURIComponent(r)}`;
  const blocked = direct.some((u) => !proxyable(u));
  return [...new Set([...front, ...(blocked ? [passLink, ...direct] : [...direct, passLink])])];
}
