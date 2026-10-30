// Browser-only answers for the Rare Store API (UI development and screenshots, never in the app).
// Link listings come from the bundled Sui Games catalog, as rare.market seeds them from
// games.json; the uploaded titles are made-up demos so the price, buy and install states can be
// built before any studio has listed one. URL flags: ?storedemo=sell shows deployed ids, =owned adds a
// licence, =both does both.

import seed from "../lib/games.seed.json";
import type { ChainGame, Licence, StoreGame, StoreGameDetail } from "./types";

const flags = () => (typeof location === "undefined" ? "" : new URLSearchParams(location.search).get("storedemo") ?? "");
const selling = () => /sell|both/.test(flags());
const owning = () => /owned|both/.test(flags());

const DAY = 86_400_000;
const NOW = Date.now();
const demoId = (n: number) => `0xde${String(n).padStart(62, "0")}`;

interface SeedGame {
  slug: string;
  name: string;
  studio?: string;
  genre?: string;
  status?: string;
  platforms?: string[];
  website?: string;
  playUrl?: string | null;
  x?: string;
  description?: string;
  logoUrl?: string;
  bannerUrl?: string;
  featured?: boolean;
}

const links: StoreGame[] = (seed as SeedGame[])
  .filter((g) => g.status === "live" || g.status === "beta")
  .map((g, i) => ({
    slug: g.slug,
    kind: "link",
    name: g.name,
    studio: { id: null, name: g.studio ?? "" },
    genre: g.genre || null,
    description: g.description ?? "",
    status: g.status as "live" | "beta",
    platforms: g.platforms ?? [],
    images: { logo: g.logoUrl || null, banner: g.bannerUrl || null, cover: null },
    links: { website: g.website || undefined, x: g.x || undefined, play: g.playUrl || (g.platforms?.includes("web") ? g.website : undefined) || undefined },
    price: null,
    game_id: null,
    featured: !!g.featured,
    sales: 0,
    created_ms: NOW - (i + 3) * 9 * DAY,
  }));

function uploaded(n: number, name: string, studio: string, genre: string, usd: number, o: { sale?: number; featured?: boolean; sales?: number; age?: number; sui?: boolean; about?: string } = {}): StoreGameDetail {
  const id = demoId(n);
  return {
    slug: name.toLowerCase().replace(/[^a-z0-9]+/g, "-"),
    kind: "uploaded",
    name,
    studio: { id: demoId(100 + n), name: studio },
    genre,
    description: o.about ?? "",
    status: "live",
    platforms: ["linux-x86_64"],
    images: { logo: null, banner: null, cover: null },
    links: {},
    price: { usd: String(usd), sale_usd: o.sale ? String(o.sale) : null, sale_ends_ms: o.sale ? NOW + 3 * DAY : null, accept_sui: o.sui ?? true },
    game_id: id,
    featured: !!o.featured,
    sales: o.sales ?? 0,
    created_ms: NOW - (o.age ?? 2) * DAY,
    page: { about: o.about ?? "", media: [], tags: [genre], requirements: "Runs on SuiPlay0X1 and any Linux x86_64 handheld." },
    latest: { version: 3, label: "1.2.0", edition: 1, published_ms: NOW - DAY, builds: [{ platform: "linux-x86_64", sha256: "ab".repeat(32), size: "734003200" }] },
    edition: 1,
    resale: n === 1 ? { enabled: true, royalty_bps: 1000, min_hold_ms: 7 * DAY, min_royalty_mist: "100000000" } : null,
    launch_check: n === 1,
    listed: true,
    blocked: false,
  };
}

const demos: StoreGameDetail[] = [
  uploaded(1, "Driftline", "Northbound Games", "Racing", 14_990_000, {
    featured: true,
    sales: 412,
    sui: true,
    about: "Arcade racing on coastal roads. Twelve tracks, split-screen for two, and weekly time trials with ghosts from the leaderboard.",
  }),
  uploaded(2, "Hollow Keep", "Small Lantern", "Roguelike", 9_990_000, { sale: 6_990_000, sales: 198, age: 12, about: "A short-run dungeon crawler built for handhelds. Each run takes about twenty minutes." }),
  uploaded(3, "Pocket Orchard", "Field Day", "Farming", 0, { sales: 1302, age: 1, about: "Grow a small orchard, trade fruit with neighbours, and keep it going through four seasons." }),
];

const all = (): StoreGame[] => [...demos, ...links];

function shelf(name: string | null): StoreGame[] {
  const list = all();
  switch (name) {
    case "featured":
      return list.filter((g) => g.featured);
    case "new":
      return [...list].sort((a, b) => (b.created_ms ?? 0) - (a.created_ms ?? 0));
    case "top":
      return [...list].filter((g) => g.kind === "uploaded").sort((a, b) => b.sales - a.sales);
    case "sale":
      return list.filter((g) => g.price?.sale_usd && (g.price.sale_ends_ms ?? 0) > Date.now());
    default:
      return list;
  }
}

const licences = (): Licence[] =>
  owning()
    ? [
        {
          id: demoId(900),
          type: "licence",
          game_id: demos[0].game_id!,
          slug: demos[0].slug,
          name: demos[0].name,
          image_url: "",
          edition: 1,
          version: 2,
          purchased_ms: NOW - 3 * 3_600_000,
          number: 413,
          kiosk_id: null,
          refundable_until_ms: NOW + 45 * 3_600_000,
          tradable_from_ms: NOW + 7 * DAY,
        },
      ]
    : [];

/** The API answer for `path`, or undefined when the mock has none. */
export function answer(path: string): unknown {
  const u = new URL(path, "https://rare.market");
  const p = u.pathname;
  if (p === "/v1/store/config")
    return selling()
      ? { package: demoId(500), origin: demoId(500), store: demoId(501), store_version: "1", policy: demoId(502), policy_version: "1", usdc_type: "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC", uploads_open: false }
      : { package: null, origin: null, store: null, store_version: null, policy: null, policy_version: null, usdc_type: null, uploads_open: false };
  if (p === "/v1/store/games") {
    let list = shelf(u.searchParams.get("shelf"));
    const kind = u.searchParams.get("kind");
    if (kind) list = list.filter((g) => g.kind === kind);
    const q = u.searchParams.get("q")?.toLowerCase();
    if (q) list = list.filter((g) => g.name.toLowerCase().includes(q));
    return { games: list.slice(0, Number(u.searchParams.get("limit") ?? 24)), next: null };
  }
  if (p.startsWith("/v1/store/games/")) {
    const slug = decodeURIComponent(p.slice("/v1/store/games/".length));
    const d = demos.find((g) => g.slug === slug);
    if (d) return { game: d };
    const l = links.find((g) => g.slug === slug);
    if (l) return { game: { ...l, page: { about: l.description, media: [], tags: [], requirements: null }, latest: null, edition: null, resale: null, launch_check: false, listed: true, blocked: false } };
    return undefined;
  }
  if (p === "/v1/store/search") {
    const q = (u.searchParams.get("q") ?? "").toLowerCase();
    return { games: all().filter((g) => g.name.toLowerCase().includes(q) || (g.genre ?? "").toLowerCase().includes(q) || g.studio.name.toLowerCase().includes(q)) };
  }
  if (p === "/v1/store/licences") return { licences: licences() };
  if (p === "/v1/store/check") {
    const own = licences().find((l) => l.slug === u.searchParams.get("slug"));
    return { owns: !!own, licence_id: own?.id ?? null, edition: own?.edition ?? null };
  }
  if (p === "/v1/store/price/sui") return { price_e8: "90600000", conf_e8: "40000", publish_time: Math.floor(Date.now() / 1000), update_b64: "", fresh: true, age_s: 3 };
  return undefined;
}

/** The demo games' on-chain state, for the chain reader in a browser. */
export function chainGame(id: string): ChainGame | null {
  const d = demos.find((g) => g.game_id === id);
  if (!d || !d.latest || !d.price) return null;
  const release = { version: d.latest.version, label: d.latest.label, edition: d.latest.edition, publishedMs: d.latest.published_ms, builds: d.latest.builds.map((b) => ({ platform: b.platform, sha256: b.sha256, size: BigInt(b.size), path: `games/${d.slug}/${d.latest!.version}/${b.platform}.tar.zst` })) };
  return {
    id,
    slug: d.slug,
    name: d.name,
    priceUsd: BigInt(d.price.usd),
    salePriceUsd: BigInt(d.price.sale_usd ?? 0),
    saleEndsMs: d.price.sale_ends_ms ?? 0,
    acceptSui: d.price.accept_sui,
    listed: true,
    blocked: false,
    launchCheck: d.launch_check,
    edition: 1,
    latest: release,
    editionsTable: demoId(700),
    resale: { enabled: !!d.resale?.enabled, royaltyBps: d.resale?.royalty_bps ?? 0, minHoldMs: d.resale?.min_hold_ms ?? 0, minRoyaltyMist: BigInt(d.resale?.min_royalty_mist ?? 0), royaltyTo: demoId(800) },
  };
}
