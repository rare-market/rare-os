// Copied verbatim from rare.market: market/packages/venues/src/media.ts (the shared image resolver).
// Keep it in sync by copying the file again; launcher-only code goes in ./media.ts.

/** The one NFT image resolver. Which field holds an item's image, how any reference (ipfs, arweave, walrus,
 * gateway URLs, data: URIs, sloppy strings) becomes the URLs to fetch, and which hosts are known dead.
 * Pure (no I/O, no Node APIs): the API, the indexer (content lane and market-img) and the web import this file,
 * so a card, the item page, the art score and the image cache always agree. docs/IMAGES_AUDIT.md has the why. */

// ---------- which field ----------

/** Display / metadata keys that can hold an item's image, best first. image_base_uri last: the SealionSui badges put the whole image there (a raw-leaf CID); when it is a directory it fails like any broken source. */
export const IMAGE_KEYS = ["image_url", "image", "img_url", "imageUrl", "image_uri", "imageUri", "media_url", "animation_url", "url", "thumbnail", "thumbnail_url", "image_link", "image_base_uri"] as const;
/** Keys that name any kind of media (or a page): only an image-looking value counts. */
const IMAGE_ONLY = new Set<string>(["animation_url", "url", "image_link"]);
const IMG_EXT = /\.(png|jpe?g|gif|webp|svg|avif|bmp)([?#].*)?$|^data:image\//i;
/** Move fields that commonly wrap an NFT's metadata (one level down is searched too). */
const NESTED = ["metadata", "data", "fields", "inner", "info", "nft", "content"];

const isStr = (v: unknown): v is string => typeof v === "string" && !!v.trim() && !/^(null|undefined|none)$/i.test(v.trim());

/** The first usable image reference in a Display-like object, with the key it came from. */
export function pickImageField(fields: Record<string, unknown> | null | undefined): { key: string; value: string } | null {
  if (!fields || typeof fields !== "object") return null;
  for (const k of IMAGE_KEYS) {
    const v = fields[k];
    if (!isStr(v)) continue;
    const s = v.trim();
    if (IMAGE_ONLY.has(k) && !IMG_EXT.test(s)) continue;
    if (parseRef(s).kind === "invalid") continue;
    return { key: k, value: s };
  }
  return null;
}

/** pickImageField's value only. */
export const pickImage = (fields: Record<string, unknown> | null | undefined): string | null => pickImageField(fields)?.value ?? null;

/** For objects with no Display image: an image field in the object's own Move JSON (top level, then one wrapper down,
 * e.g. `metadata.image_uri`), or an inscription's data: image in `contents`. */
export function pickImageFromJson(json: Record<string, unknown> | null | undefined): string | null {
  if (!json || typeof json !== "object") return null;
  const top = pickImage(json);
  if (top) return top;
  for (const k of NESTED) {
    const v = json[k];
    if (v && typeof v === "object" && !Array.isArray(v)) {
      const hit = pickImage(v as Record<string, unknown>);
      if (hit) return hit;
    }
  }
  const c = json.contents;
  if (Array.isArray(c) && typeof c[0] === "string" && /^data:image\//i.test(c[0]) && c[0].length > 64) return c[0];
  return null;
}

// ---------- references ----------

export type Ref =
  | { kind: "http"; url: string }
  | { kind: "ipfs"; path: string; gateway: string | null }
  | { kind: "arweave"; id: string; url: string | null }
  | { kind: "walrus"; path: string; original: string | null }
  | { kind: "data"; uri: string; mime: string }
  | { kind: "invalid"; reason: string };

const CIDV0 = /^Qm[1-9A-HJ-NP-Za-km-z]{44}$/;
const CIDV1 = /^b[a-z2-7]{50,}$/;
export const isCid = (s: string) => CIDV0.test(s) || CIDV1.test(s);
/** Walrus blob ids are 32 bytes unpadded base64url (43 chars); quilt patch ids are 37 bytes (50 chars). */
const WALRUS_BLOB = /^[A-Za-z0-9_-]{43}$/;
const WALRUS_PATCH = /^[A-Za-z0-9_-]{48,52}$/;
const ARWEAVE_ID = /^[A-Za-z0-9_-]{43}$/;

/** Hosts that no longer serve anything (DNS gone or content deleted). Only a rescue override fixes their items. */
export const DEAD_HOSTS = ["walrus.tusky.io", "ipfs.bluemove.net"];
/** IPFS gateways that refuse server and <img> fetches (403/429 or a Cloudflare challenge since Sep 2026) or are gone:
 * their URLs are rewritten to the live ones. */
const DEAD_GATEWAY =
  /^(ipfs\.io|gateway\.ipfs\.io|dweb\.link|nftstorage\.link|w3s\.link|cloudflare-ipfs\.com|cf-ipfs\.com|gateway\.pinata\.cloud|ipfs\.infura\.io|infura-ipfs\.io|4everland\.io|ipfs\.4everland\.io|gateway\.lighthouse\.storage|ipfs\.decentralized-content\.com|ipfs\.eth\.aragon\.network|hardbin\.com|ipfs\.runfission\.com|ipfs\.bluemove\.net|tradeport\.mypinata\.cloud|byzantion\.mypinata\.cloud|ipfs\.thirdwebcdn\.com|gateway\.ipfscdn\.io)$|\.ipfs\.(dweb\.link|nftstorage\.link|w3s\.link|cf-ipfs\.com|4everland\.io)$|^ipfs\.tradeport\.xyz$/;
/** Hosts whose project moved its files to a new domain under the same paths (checked by fetching both):
 * old host -> new host. The new one is tried first, the old one stays as a fallback. */
export const HOST_MOVES: Record<string, string> = {
  // The Birds (WORM, 281k items): birds.dog's DNS is gone; thebirds.ai serves the same /img/<level>/<n>.png files.
  "asset.birds.dog": "assets.thebirds.ai",
};
/** The URL on its project's new host, or null. */
export function movedUrl(url: string): string | null {
  try {
    const u = new URL(url);
    const to = HOST_MOVES[u.hostname.toLowerCase()];
    if (!to) return null;
    u.hostname = to;
    u.protocol = "https:";
    return u.href;
  } catch {
    return null;
  }
}
/** Live public IPFS gateways (tested Oct 2026: plain HTTP to servers and browsers), best first. */
export const IPFS_GATEWAYS = ["ipfs.filebase.io", "ipfs.raribleuserdata.com", "gw.ipfs-lens.dev", "dget.top"];
/** Public Walrus mainnet aggregators, best first. */
export const WALRUS_AGGREGATORS = [
  "aggregator.walrus-mainnet.walrus.space",
  "aggregator.mainnet.walrus.mirai.cloud",
  "aggregator.suicore.com",
  "walrus.globalstake.io",
  "walrus-mainnet-aggregator.stakecraft.com",
  "aggregator.walrus-mainnet.h2o-nodes.com",
  "walrus-agg.mainnet.obelisk.sh",
];
export const ARWEAVE_GATEWAYS = ["arweave.net", "permagate.io"];

const hostOf = (u: string): string => {
  try {
    return new URL(u).hostname.toLowerCase();
  } catch {
    return "";
  }
};
export const isDeadHost = (host: string) => DEAD_HOSTS.some((d) => host === d || host.endsWith(`.${d}`));

/** Trim quotes, brackets, whitespace and trailing junk that creators leave in Display values. */
export function cleanRef(raw: string): string {
  let s = raw.replace(/[\u0000-\u001f​-‍﻿]/g, "").trim();
  s = s.replace(/^["'`(\[]+|["'`)\],;]+$/g, "").trim();
  return s;
}

/** IPFS path ("cid/rest?query") with duplicate slashes collapsed. */
const ipfsPathOf = (p: string) => p.replace(/^\/+/, "").replace(/([^:])\/{2,}/g, "$1/");

/** One reference, classified. Never throws. */
export function parseRef(input: unknown): Ref {
  if (typeof input !== "string") return { kind: "invalid", reason: "not a string" };
  let s = cleanRef(input);
  if (!s || /^(null|undefined|none|nan)$/i.test(s)) return { kind: "invalid", reason: "empty" };
  if (/^data:/i.test(s)) {
    const mime = s.slice(5, s.search(/[;,]|$/)).trim().toLowerCase() || "text/plain";
    return { kind: "data", uri: s, mime };
  }
  if (/^<(svg|\?xml)/i.test(s)) return { kind: "data", uri: `data:image/svg+xml;charset=utf-8,${encodeURIComponent(s)}`, mime: "image/svg+xml" };
  // ipfs:// ipfs://ipfs/ ipfs:/ ipfs: ipns:// /ipfs/ and bare CIDs
  const ip = s.match(/^(?:ipfs:\/{0,2}(?:ipfs\/)?|\/ipfs\/)(.+)$/i);
  if (ip) {
    const path = ipfsPathOf(ip[1]);
    return isCid(path.split(/[/?#]/)[0]) ? { kind: "ipfs", path, gateway: null } : { kind: "invalid", reason: "bad ipfs cid" };
  }
  if (/^ipns:\/\//i.test(s)) return { kind: "http", url: `https://${IPFS_GATEWAYS[0]}/ipns/${s.slice(7)}` };
  if (isCid(s.split(/[/?#]/)[0])) return { kind: "ipfs", path: ipfsPathOf(s), gateway: null };
  if (/^walrus:\/\//i.test(s)) return walrusRef(s.slice(9).replace(/^\/+/, ""), null);
  if (/^ar:\/\//i.test(s)) {
    const id = s.slice(5).replace(/^\/+/, "");
    return ARWEAVE_ID.test(id.split(/[/?#]/)[0]) ? { kind: "arweave", id, url: null } : { kind: "invalid", reason: "bad arweave id" };
  }
  if (WALRUS_BLOB.test(s)) return walrusRef(s, null);
  if (s.startsWith("//")) s = `https:${s}`;
  // Bare host/path ("example.com/a.png").
  if (!/^[a-z][a-z0-9+.-]*:/i.test(s) && /^[a-z0-9-]+(\.[a-z0-9-]+)+\//i.test(s)) s = `https://${s}`;
  let u: URL;
  try {
    u = new URL(s);
  } catch {
    return { kind: "invalid", reason: "bad url" };
  }
  if (u.protocol !== "http:" && u.protocol !== "https:") return { kind: "invalid", reason: `scheme ${u.protocol.replace(":", "")}` };
  if (u.username || u.password) return { kind: "invalid", reason: "credentials in url" };
  u.hash = "";
  const host = u.hostname.toLowerCase();
  // IPFS path and subdomain gateways.
  const pg = u.pathname.match(/^\/ipfs\/([^/]+)(.*)$/);
  if (pg && isCid(pg[1])) return { kind: "ipfs", path: ipfsPathOf(safeDecode(pg[1] + pg[2]) + u.search), gateway: DEAD_GATEWAY.test(host) ? null : `${u.protocol}//${u.host}` };
  const sd = host.match(/^([a-z0-9]+)\.ipfs\./);
  if (sd && isCid(sd[1])) return { kind: "ipfs", path: ipfsPathOf(sd[1] + safeDecode(u.pathname === "/" ? "" : u.pathname) + u.search), gateway: null };
  // Walrus aggregators: /v1/blobs/<id>, /v1/<id> (legacy), /v1/blobs/by-quilt-patch-id/<id>, /v1/blobs/by-quilt-id/<q>/<name>.
  if (/walrus|wal\.app$|^wal\.gg$/.test(host) || /^\/v1\/blobs\//.test(u.pathname)) {
    const wm = u.pathname.match(/^\/v1\/(?:blobs\/)?((?:by-quilt-patch-id\/|by-quilt-id\/[^/]+\/)?[A-Za-z0-9_-]{20,}.*)$/);
    if (wm) return walrusRef(safeDecode(wm[1]), isDeadHost(host) ? null : u.href);
  }
  // Blob-id hosts: https://wal.gg/<blob>, https://walrus.tusky.io/<blob>
  const bare = u.pathname.slice(1);
  if ((WALRUS_BLOB.test(bare) || WALRUS_PATCH.test(bare)) && /(^|\.)wal\.gg$|walrus/.test(host)) return walrusRef(bare, isDeadHost(host) ? null : u.href);
  if (host === "arweave.net" || host.endsWith(".arweave.net") || host === "ar-io.net" || host.endsWith(".ar-io.net")) {
    const id = u.pathname.slice(1);
    if (ARWEAVE_ID.test(id.split("/")[0])) return { kind: "arweave", id: id + u.search, url: host.includes(".") && host.split(".").length > 2 ? u.href : null };
    return { kind: "http", url: u.href };
  }
  return { kind: "http", url: u.href };
}

const safeDecode = (s: string) => {
  try {
    return decodeURI(s);
  } catch {
    return s;
  }
};

function walrusRef(id: string, original: string | null): Ref {
  if (/^by-quilt-(patch-)?id\//.test(id)) return { kind: "walrus", path: `/v1/blobs/${id}`, original };
  const head = id.split(/[/?#]/)[0];
  if (WALRUS_BLOB.test(head)) return { kind: "walrus", path: `/v1/blobs/${id}`, original };
  if (WALRUS_PATCH.test(head)) return { kind: "walrus", path: `/v1/blobs/by-quilt-patch-id/${id}`, original };
  return original ? { kind: "http", url: original } : { kind: "invalid", reason: "bad walrus id" };
}

/** A canonical string per image (one cache key whatever gateway or spelling was used). */
export function refKey(ref: Ref): string {
  switch (ref.kind) {
    case "http":
      return ref.url;
    case "ipfs":
      return `ipfs://${ref.path}`;
    case "arweave":
      return `ar://${ref.id}`;
    case "walrus":
      return `walrus://${ref.path.replace(/^\/v1\/blobs\//, "")}`;
    case "data":
      return ref.uri;
    default:
      return "";
  }
}

/** URLs to fetch for a reference, best first: a creator's own live gateway, then the live public ones. Plain http
 * also gets its https form first (a page on https cannot show http images reliably). Data URIs return themselves. */
export function candidates(ref: Ref): string[] {
  switch (ref.kind) {
    case "http": {
      const host = hostOf(ref.url);
      if (isDeadHost(host)) return [];
      return uniq([movedUrl(ref.url), ...(ref.url.startsWith("http://") ? [`https://${ref.url.slice(7)}`, ref.url] : [ref.url])]);
    }
    case "ipfs":
      return uniq([ref.gateway ? `${ref.gateway}/ipfs/${ref.path}` : null, ...IPFS_GATEWAYS.map((g) => `https://${g}/ipfs/${ref.path}`)]);
    case "arweave":
      return uniq([ref.url, ...ARWEAVE_GATEWAYS.map((g) => `https://${g}/${ref.id}`)]);
    case "walrus":
      return uniq([ref.original, ...WALRUS_AGGREGATORS.map((a) => `https://${a}${ref.path}`)]);
    case "data":
      return [ref.uri];
    default:
      return [];
  }
}

const uniq = (xs: (string | null | undefined)[]) => [...new Set(xs.filter((x): x is string => !!x))];

/** Every URL to try for one raw reference (shorthand for candidates(parseRef(raw))). */
export const mediaCandidates = (raw: unknown): string[] => candidates(parseRef(raw));

/** The data: URI's payload as UTF-8 text (JSON or SVG), or null for binary or bad URIs. */
export function dataText(uri: string): string | null {
  const m = uri.match(/^data:([^,]*?),(.*)$/s);
  if (!m) return null;
  try {
    if (/;base64/i.test(m[1])) {
      const bin = typeof atob === "function" ? atob(m[2].replace(/\s+/g, "")) : "";
      return decodeURIComponent(Array.from(bin, (c) => `%${c.charCodeAt(0).toString(16).padStart(2, "0")}`).join(""));
    }
    return decodeURIComponent(m[2]);
  } catch {
    return null;
  }
}

/** Follows a metadata reference that is inline (a data:application/json URI) to its image. Other refs pass through. */
export function inlineImage(raw: string): string {
  const r = parseRef(raw);
  if (r.kind !== "data" || !/json|text\/plain/.test(r.mime)) return raw;
  const t = dataText(r.uri);
  if (!t) return raw;
  if (/^\s*<svg/i.test(t)) return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(t)}`;
  try {
    const j = JSON.parse(t) as Record<string, unknown>;
    return pickImage(j) ?? raw;
  } catch {
    return raw;
  }
}

/** Placeholder art and "no image" stand-ins. */
const PLACEHOLDER = /placehold|\/(null|undefined|none|nan)(\.\w+)?$|no[-_]?image|image[-_]?(not[-_]?found|missing|unavailable)/i;

/** A reference that can show an image at all (before anything is fetched): parses, is not a placeholder, not on a dead
 * host, and a data: URI declares an image (or inline metadata that leads to one). */
export function usableRef(raw: string | null | undefined): boolean {
  if (!raw || !raw.trim()) return false;
  const r = parseRef(inlineImage(raw));
  if (r.kind === "invalid") return false;
  if (r.kind === "data") return /^image\//.test(r.mime) && r.uri.length > 64;
  if (r.kind === "http" && (PLACEHOLDER.test(r.url) || isDeadHost(hostOf(r.url)))) return false;
  return candidates(r).length > 0;
}

/** Browser-like request headers for a fetch on behalf of a viewer: a current Chrome UA and the image host's own origin
 * as Referer (hotlink rules allow their own site). Used by market-img's passthrough and the art probe. */
export const BROWSER_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
export function browserHeaders(url: string): Record<string, string> {
  let origin = "";
  try {
    origin = `${new URL(url).origin}/`;
  } catch {
    /* no referer */
  }
  return { "user-agent": BROWSER_UA, accept: "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8", "accept-language": "en-US,en;q=0.9", ...(origin ? { referer: origin } : {}) };
}

/** Hosts wsrv.nl refuses ("Domain or TLD blocked by policy", seen Oct 2026): .xyz and .fun, and a few named hosts. */
const WSRV_BLOCKED = /\.(xyz|fun)$|(^|\.)(thebirds\.ai|gamexchange\.app)$/i;
export const wsrvBlocked = (url: string) => WSRV_BLOCKED.test(hostOf(url));
