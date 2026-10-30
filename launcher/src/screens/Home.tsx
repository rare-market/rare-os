// Home: a console home with a trading terminal's live data.
//
//   Stage     the focused game full bleed behind the top, its logo and Play, and the
//             Sui panel on the right: SUI price with its 24 h line, your portfolio, network.
//   Shelf     your games; moving along it changes the stage.
//   Market    top collections and the latest sales, live from rare.market.
//   Then      Sui Games, your collectibles, Rare.

import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { Art } from "../components/Art";
import { scrollBackdrop, setBackdrop } from "../components/Backdrop";
import { IconPlay, IconSteam, IconWallet } from "../components/icons";
import { AppArt, GameTile, fmtAgo } from "../components/ui";
import { LayoutGrid } from "lucide-react";
import { usePadHandler } from "../nav/input";
import { focusEl } from "../nav/focus";
import { feedback } from "../nav/feedback";
import { native } from "../lib/bridge";
import { loadTokens, useLive } from "../lib/live";
// Bets and coinflip are hidden from every entry point for now; flip to bring them back.
const SHOW_BETS = false;
import { RARE, gamesNow, loadGames, rare, type ItemCard, type SuiGame } from "../lib/rare";
import { MImg } from "../marketplace/MImg";
import { imageChain } from "../marketplace/media";
import { seed } from "../lib/state";
import { useApp } from "../lib/store";
import { refreshInventory, useInventory } from "../inventory/store";
import { applyPending, summarize } from "../inventory/logic";
import { fmtInt, fmtSui } from "../marketplace/format";
import { startLine, useStart } from "../lib/progress";
import { useFavourites } from "../lib/favourites";
import { SOURCE_LABEL, type Game } from "../lib/types";
import { openInMarket } from "../marketplace/nav";
import { Avatar, Delta, Sui, Verified } from "../marketplace/parts";
import { shortName, timeAgo } from "../marketplace/format";
import { Button, Dot, Empty, HeadRow, Num, Panel, Rail, Row, SectionHead, SuiLogo, Thumb } from "../ui";
import { SuiShelf } from "../sui/ui/HomeSui";
import type { RareAccount } from "../wallet/core/types";
import { SuiGameCard } from "./SuiGames";
import { isActive } from "../games/catalog";
import { AppsRail } from "../apps/tiles";
import { OnYourPcRail } from "../stream/tiles";
import { hideMoveCard, moveCardVisible } from "../lib/setup";
import { devSign } from "../wallet/ui/dev";
import { useWallet, useWalletNav } from "../wallet/ui/store";
import { Suspense } from "react";
import { lazyNamed } from "../lib/lazy";
import { rarelingsOn } from "../perks/config";

// Friends and parties are empty until the friends service starts after first paint, so they load with it.
const HomeFriends = lazyNamed(() => import("../friends/ui/HomeFriends"), "HomeFriends");

/** Emulator frontends first, then the rest by name. */
const APP_ORDER = ["es-de", "esde", "emulationstation", "retroarch"];
export function sortApps(apps: Game[]): Game[] {
  const rank = (g: Game) => {
    const n = g.title.toLowerCase().replace(/[^a-z0-9]/g, "");
    const i = APP_ORDER.findIndex((k) => n.startsWith(k.replace(/[^a-z0-9]/g, "")));
    return i < 0 ? 99 : i;
  };
  return [...apps].sort((a, b) => rank(a) - rank(b) || a.title.localeCompare(b.title));
}

export function HeroLogo({ sources, title, big }: { sources: string[]; title: string; big?: boolean }) {
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [title]);
  return (
    <div className={`flex items-end ${big ? "h-[8.5rem] max-w-[34rem]" : "h-[6rem] max-w-[26rem]"}`}>
      {!failed && sources.length > 0 ? (
        // Full opacity, with a tight dark shadow (no glow) so the logo stays crisp over busy art.
        <Art sources={sources} title={title} variant="logo" className="h-full w-auto max-w-full object-contain object-left-bottom drop-shadow-[0_2px_6px_rgb(0_0_0/0.6)]" onFail={() => setFailed(true)} />
      ) : (
        <div className={`font-bold tracking-[-0.03em] text-fg [text-shadow:0_2px_8px_rgb(0_0_0/0.6)] ${big ? "text-[3rem] leading-none" : "text-hero"}`}>{title}</div>
      )}
    </div>
  );
}

/** The game on the stage: the one focused on the shelf, else the last played. */
const useStage = create<{ id: string | null }>(() => ({ id: null }));
let stageTimer: ReturnType<typeof setTimeout> | null = null;
function stageLater(id: string) {
  // A held direction sweeps the shelf: only settle the art once the move pauses.
  if (stageTimer) clearTimeout(stageTimer);
  stageTimer = setTimeout(() => useStage.setState({ id }), 220);
}

const fmtPlaytime = (m?: number | null) => (!m ? "" : m < 60 ? `${m} min played` : `${Math.round(m / 60)} h played`);

function Stage({ fallback }: { fallback: Game | undefined }) {
  const id = useStage((s) => s.id);
  const game = useApp((s) => (id ? s.library?.games.find((g) => g.id === id) : undefined)) ?? fallback;

  useEffect(() => {
    if (game) setBackdrop({ key: game.id, sources: game.art.hero.length ? game.art.hero : game.art.header, title: game.title });
  }, [game]);
  useEffect(() => () => setBackdrop(null), []);

  // The stage never blanks while you sweep the shelf: the outgoing game stays (inert) under the incoming one,
  // which fades in over it; the old one is dropped once the fade is done. Same-game updates change in place.
  const [prev, setPrev] = useState<Game | null>(null);
  const [last, setLast] = useState<Game | undefined>(game);
  if (game && last?.id !== game.id) {
    setLast(game);
    if (last) setPrev(last);
  } else if (game && last !== game) setLast(game);
  useEffect(() => {
    if (!prev) return;
    const t = setTimeout(() => setPrev(null), 320);
    return () => clearTimeout(t);
  }, [prev]);

  if (!game)
    return (
      <div className="flex min-w-0 flex-1 flex-col justify-end gap-[1rem] pb-[0.5rem]">
        <div className="text-hero font-bold">Welcome to Rare</div>
        <div className="max-w-[32rem] text-body text-muted">Your games from Steam, Epic, GOG, itch.io and your folders show up here.</div>
        <div data-nav-group="hero" data-nav-remember className="flex gap-[0.75rem]">
          <Button variant="primary" size="lg" def icon={<IconSteam size="1.15rem" />} onClick={() => void native.steamBigPicture()}>
            Open Steam
          </Button>
          <Button size="lg" onClick={() => useApp.getState().setTab("games")}>
            Browse Sui games
          </Button>
        </div>
      </div>
    );

  return (
    <div className="grid min-w-0 flex-1">
      {prev && (
        <div key={`out-${prev.id}`} inert aria-hidden className="pointer-events-none col-start-1 row-start-1 flex min-w-0 flex-col justify-end">
          <StageBody game={prev} />
        </div>
      )}
      <div key={game.id} className={`col-start-1 row-start-1 flex min-w-0 flex-col justify-end ${prev ? "fade" : ""}`}>
        <StageBody game={game} />
      </div>
    </div>
  );
}

function StageBody({ game }: { game: Game }) {
  const running = useApp((s) => s.running.includes(game.id));
  const launch = useApp((s) => s.launch);
  const open = useApp((s) => s.open);
  // While it starts: "Starting Steam", "Starting {game}", then "Still starting. B to cancel."
  const start = useStart((s) => s.starts[game.id]);
  const [now, setNow] = useState(() => Date.now());
  const starting = running && !!start && start.phase !== "playing" && start.phase !== "none";
  useEffect(() => {
    if (!starting) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [starting]);
  const startText = running ? startLine(game.title, start, now) : null;
  const meta = [SOURCE_LABEL[game.source], fmtAgo(game.lastPlayed), fmtPlaytime(game.playtimeMinutes)].filter(Boolean);
  return (
    <div className="flex min-w-0 flex-1 flex-col justify-end gap-[1rem]">
      <div className="flex items-center gap-[0.5rem] text-small font-medium text-fg/75">
        {running && <Dot tone="brand" live />}
        {running ? startText ?? "Now playing" : game.lastPlayed ? "Continue playing" : "In your library"}
      </div>
      <HeroLogo sources={game.art.logo} title={game.title} big />
      <div className="flex items-center gap-[0.6rem] text-body text-fg/75">
        {meta.map((m, i) => (
          <span key={m} className="flex items-center gap-[0.6rem]">
            {i > 0 && <span className="text-fg/30">·</span>}
            {m}
          </span>
        ))}
      </div>
      <div data-nav-group="hero" data-nav-remember data-game={game.id} className="mt-[0.35rem] flex gap-[0.75rem]">
        <Button variant="primary" size="lg" hint={running ? "Resume" : "Play"} icon={<IconPlay size="1.1rem" />} onClick={() => void launch(game)} className="min-w-[10rem]">
          {running ? "Resume" : game.source === "apps" ? "Open" : "Play"}
        </Button>
        <Button size="lg" onClick={() => open({ kind: "game", id: game.id })}>
          Details
        </Button>
      </div>
    </div>
  );
}

/**
 * Your Rare: what your game items are worth first (a calm number, then each game), then your
 * money, with the SUI price in small type. A widget in the corner of the stage; solid, never over
 * art, no skeletons (a thin bar while the inventory loads). Signed out it asks for a passkey.
 */
function YourRare({ address }: { address: string | null }) {
  const live = useLive();
  const p = live.portfolio;
  const base = useInventory((s) => s.base);
  const pending = useInventory((s) => s.pending);
  const owner = useInventory((s) => s.address);
  const loading = useInventory((s) => s.loading);
  useEffect(() => {
    if (address) void refreshInventory(address);
  }, [address]);

  if (!address) {
    return (
      <Panel className="flex w-[24rem] shrink-0 flex-col gap-[1rem] self-end p-[1.25rem]">
        <div className="text-label text-secondary">Your items</div>
        <div className="text-body text-muted">Sign in with a passkey to see your items.</div>
        <div data-nav-group="yourrare" data-nav-remember>
          <Button variant="primary" onClick={() => useApp.getState().open({ kind: "signin" })}>
            Sign in
          </Button>
        </div>
        <SuiPrice />
      </Panel>
    );
  }

  const sum = base && owner === address ? summarize(applyPending(base, pending)) : null;
  return (
    <button
      data-nav
      data-hint-a="Open your items"
      onClick={() => {
        useApp.getState().setTab("wallet");
      }}
      className="tile tile-wide relative flex w-[24rem] shrink-0 flex-col self-end overflow-hidden rounded-panel bg-l1 p-[1.25rem] text-left"
    >
      <div className={`pointer-events-none absolute inset-x-0 top-0 h-[2px] ${loading ? "load-bar" : ""}`} />
      <div className="text-label text-secondary">Items in your games</div>
      <div className="mt-[0.25rem] min-h-[2.5rem]">
        {sum && <span className="num text-display font-semibold">{fmtSui(sum.valueMist, { dp: 2, trim: true })} <span className="text-row font-medium text-secondary">SUI</span></span>}
      </div>
      <div className="min-h-[1.25rem] text-meta text-muted">
        {sum && (sum.items > 0 ? `${fmtInt(sum.items)} ${sum.items === 1 ? "item" : "items"}${sum.offers > 0 ? `, ${fmtInt(sum.offers)} ${sum.offers === 1 ? "offer" : "offers"}` : ""}` : "No game items yet")}
      </div>
      {sum && sum.games.length > 0 && (
        <div className="mt-[0.9rem] flex flex-col gap-[0.5rem]">
          {sum.games.slice(0, 4).map((g) => (
            <div key={g.slug} className="flex items-center gap-[0.6rem] text-small">
              <Thumb src={g.logoUrl || null} label={g.name} className="h-[1.5rem] w-[1.5rem] shrink-0 rounded-[0.3rem]" />
              <span className="min-w-0 flex-1 truncate font-medium">{g.name}</span>
              <span className="num shrink-0 text-muted">{fmtSui(g.valueMist, { dp: 2, trim: true })} SUI</span>
            </div>
          ))}
          {sum.games.length > 4 && <div className="text-meta text-muted">and {sum.games.length - 4} more</div>}
        </div>
      )}
      <div className="my-[1rem] h-px bg-hair" />
      <div className="flex items-end justify-between gap-[1rem]">
        <div className="min-w-0">
          <div className="text-label text-secondary">Your total</div>
          <span className="block h-[1.6rem]">{p && <Num kind="money" value={p.totalUsd} live className="text-row font-semibold" />}</span>
        </div>
        {p && <Num kind="pnl" value={p.change24hUsd} live className="text-small font-medium" />}
      </div>
      <SuiPrice />
    </button>
  );
}

/** The SUI price, small. */
function SuiPrice() {
  const live = useLive();
  return (
    <div className="mt-[0.9rem] flex items-center gap-[0.45rem] text-meta text-muted">
      <SuiLogo size="0.9rem" />
      <span>SUI</span>
      {live.suiUsd != null && <Num kind="price" value={live.suiUsd} live className="text-meta" />}
      <span className="ml-auto">
        <Num kind="change" value={live.suiChange24h} className="text-meta" />
      </span>
    </div>
  );
}

/**
 * The game strip: small square tiles in one row under the top bar. The focused
 * tile grows by 45 % from its top-left corner and the tiles after it slide right to make room
 * (transform only, see .strip-tile in app.css); its name appears under it. Moving along the strip
 * changes the art behind the whole screen. A plays, X opens the game page.
 */
function StripTile({ game, def, first }: { game: Game; def?: boolean; first?: boolean }) {
  const running = useApp((s) => s.running.includes(game.id));
  const cover = game.art.cover.length ? game.art.cover : game.art.header;
  return (
    <button
      data-nav
      data-game={game.id}
      data-hint-a={running ? "Resume" : "Play"}
      data-hint-x="Details"
      data-nav-default={def ? "" : undefined}
      onFocus={() => stageLater(game.id)}
      onClick={() => void useApp.getState().launch(game)}
      className="strip-tile relative h-[5.5rem] w-[5.5rem] shrink-0 rounded-card bg-l3 text-left"
    >
      <span className="absolute inset-0 overflow-hidden rounded-card">
        {game.source === "apps" && !game.art.cover.length ? (
          <AppArt game={game} />
        ) : (
          <Art sources={cover} title={game.title} variant="cover" priority={first} className="absolute inset-0 h-full w-full object-cover object-[center_28%]" />
        )}
      </span>
      {running && <Dot tone="primary" live className="absolute right-[0.4rem] top-[0.4rem] ring-2 ring-black/40" />}
      <span className="strip-label pointer-events-none absolute left-0 top-full mt-[0.45rem] whitespace-nowrap text-[0.68rem] font-semibold leading-tight text-fg">
        {game.title}
        <span className="block text-[0.55rem] font-normal text-fg/60">{running ? "Playing" : SOURCE_LABEL[game.source]}</span>
      </span>
    </button>
  );
}

function GameStrip({ games, total }: { games: Game[]; total: number }) {
  const ref = useRef<HTMLDivElement>(null);
  // Home opens on the strip, like a console: unless the player already moved, the first tile
  // takes focus when the strip appears (the library can land after the stage's buttons).
  useEffect(() => {
    const t = setTimeout(() => {
      const a = document.activeElement as HTMLElement | null;
      if (!a || a === document.body || a.closest('[data-nav-group="hero"]')) focusEl(ref.current?.querySelector<HTMLElement>("[data-nav-default]"), { smooth: false });
    }, 60);
    return () => clearTimeout(t);
  }, []);
  // X on a strip tile: the game page.
  usePadHandler((b) => {
    const id = b === "x" ? (document.activeElement as HTMLElement | null)?.dataset?.game : undefined;
    if (!id) return false;
    feedback("open");
    useApp.getState().open({ kind: "game", id });
    return true;
  });
  return (
    <div ref={ref} data-rail data-nav-group="strip" className="relative flex h-[13.5rem] items-start gap-[0.75rem] overflow-x-auto px-[2.5rem]">
      {games.map((g, i) => (
        <StripTile key={g.id} game={g} def={i === 0} first={i < 8} />
      ))}
      <button
        data-nav
        data-hint-a="Library"
        onClick={() => useApp.getState().setTab("library")}
        className="strip-tile relative flex h-[5.5rem] w-[5.5rem] shrink-0 flex-col items-center justify-center gap-[0.25rem] rounded-card bg-l2 text-fg"
      >
        <LayoutGrid size="1.5rem" strokeWidth={1.75} />
        <span className="strip-label pointer-events-none absolute left-0 top-full mt-[0.45rem] whitespace-nowrap text-left text-[0.68rem] font-semibold leading-tight">
          All games
          <span className="block text-[0.55rem] font-normal text-fg/60">{total} in your library</span>
        </span>
      </button>
    </div>
  );
}

const COLL_COLS = "grid-cols-[1.25rem_minmax(0,1fr)_6.5rem_4.75rem]";
function TopCollections() {
  const rows = useLive((s) => s.collections);
  return (
    <Panel className="flex min-w-0 flex-col p-[1rem]">
      <SectionHead title="Top collections" note="24h volume" right={<span>rare.market</span>} className="mb-[0.5rem] px-[0.75rem]" />
      <HeadRow cols={COLL_COLS}>
        <span>#</span>
        <span>Collection</span>
        <span className="text-right">Floor</span>
        <span className="text-right">24h</span>
      </HeadRow>
      <div data-nav-group="home-collections" className="flex flex-col">
        {(rows ?? []).slice(0, 5).map((r, i) => (
          <Row key={r.slug} cols={COLL_COLS} idx={i} hint="Open collection" onClick={() => openInMarket({ kind: "collection", slug: r.slug })}>
            <span className="tabular-nums font-semibold text-muted">{r.rank}</span>
            <span className="flex min-w-0 items-center gap-[0.65rem]">
              <Avatar src={r.imageUrl} label={r.name} size="2.1rem" />
              <span className="truncate font-semibold">{r.name}</span>
              {r.verified && <Verified size="0.9rem" />}
            </span>
            <Sui mist={r.floorMist} dp={2} className="justify-end text-right font-semibold" />
            <span className="text-right font-semibold">
              <Delta value={r.floorChangePct} />
            </span>
          </Row>
        ))}
        {!rows && <div className="h-[16.25rem]" />}
      </div>
    </Panel>
  );
}

const SALE_COLS = "grid-cols-[minmax(0,1fr)_6.5rem_3.25rem]";
function LiveSales() {
  const sales = useLive((s) => s.sales);
  const fresh = useLive((s) => s.fresh);
  return (
    <Panel className="flex min-w-0 flex-col p-[1rem]">
      <SectionHead
        title="Latest sales"
        right={
          <span className="flex items-center gap-[0.45rem]">
            <Dot tone="brand" live />
            Live
          </span>
        }
        className="mb-[0.5rem] px-[0.75rem]"
      />
      <HeadRow cols={SALE_COLS}>
        <span>Item</span>
        <span className="text-right">Price</span>
        <span className="text-right">Time</span>
      </HeadRow>
      <div data-nav-group="home-sales" className="flex flex-col">
        {(sales ?? []).slice(0, 5).map((e, i) => (
          <Row
            key={e.id}
            cols={SALE_COLS}
            idx={i}
            hint="View item"
            className={fresh.includes(e.id) ? "tint-up" : ""}
            onClick={() => (e.item?.id ? openInMarket({ kind: "item", id: e.item.id }) : e.collection && openInMarket({ kind: "collection", slug: e.collection.slug }))}
          >
            <span className="flex min-w-0 items-center gap-[0.65rem]">
              <Avatar src={e.item?.imageUrl ?? e.collection?.imageUrl} label={e.item?.name ?? ""} size="2.1rem" square />
              <span className="min-w-0">
                <span className="block truncate font-semibold">{e.item ? shortName({ name: e.item.name, collection: e.collection }) : e.collection?.name}</span>
                <span className="block truncate text-meta text-muted">{e.collection?.name}</span>
              </span>
            </span>
            <Sui mist={e.priceMist} dp={2} className="justify-end text-right font-semibold" />
            <span className="text-right text-meta text-muted">{timeAgo(e.at)}</span>
          </Row>
        ))}
        {!sales && <div className="h-[16.25rem]" />}
      </div>
    </Panel>
  );
}

interface RareLink {
  label: string;
  sub: string;
  url?: string;
  tab?: "market" | "games";
  image: string | null;
  mosaic?: string[];
}

/** First slot below the stage for 14 days after setup: moving assets from the old SuiPlay wallet. */
function MoveCard() {
  const [hidden, setHidden] = useState(false);
  const acct = useWallet((w) => w.current);
  const show = !hidden && moveCardVisible() && !!acct && (acct.kind !== "watch" || devSign);
  if (!show) return null;
  return (
    <section className="px-[2.5rem]">
      <Panel className="flex items-center gap-[1.5rem] px-[1.6rem] py-[1.2rem]">
        <span className="min-w-0 flex-1">
          <span className="block text-row font-semibold">Move from your old SuiPlay wallet</span>
          <span className="block text-body text-muted">Bring your coins and collectibles over. It takes a few minutes.</span>
        </span>
        <Button
          variant="primary"
          hint="Start moving"
          onClick={() => {
            useWalletNav.getState().reset([{ kind: "playtron" }]);
            useApp.getState().setTab("wallet");
          }}
        >
          Move
        </Button>
        <Button
          variant="ghost"
          hint="Hide this card"
          onClick={() => {
            hideMoveCard();
            setHidden(true);
          }}
        >
          Not now
        </Button>
      </Panel>
    </section>
  );
}

export function Home() {
  const library = useApp((s) => s.library);
  const toast = useApp((s) => s.toast);
  // Last session's catalog and Marketplace art from Rust's cache: drawn in the first frame.
  const [games, setGames] = useState<SuiGame[]>(() => (gamesNow() ?? []).filter(isActive));
  const [account, setAccount] = useState<RareAccount | null>(null);
  const [items, setItems] = useState<ItemCard[] | null>(null);
  const [marketArt, setMarketArt] = useState<string | null>(() => seed<{ featured?: Array<{ imageUrl: string | null }> }>("rare:/v1/home")?.v.featured?.find((f) => f.imageUrl)?.imageUrl ?? null);
  const scroller = useRef<HTMLDivElement>(null);

  const liveAccount = useLive((s) => s.portfolio?.address ?? null);
  useEffect(() => {
    void import("../wallet").then((w) => w.keystore.current()).then(async (a) => {
      setAccount(a);
      if (a) setItems((await rare.items(a.address, 20).catch(() => ({ data: [] as ItemCard[] }))).data);
    });
  }, [liveAccount]);
  useEffect(() => {
    void loadGames().then((r) => {
      // Only games proven live or in beta; the rest wait under Coming on the Sui Games tab.
      setGames(r.games.filter(isActive));
      loadTokens(r.games.flatMap((g) => (g.token ? [g.token.coinType] : [])));
    });
    void native
      .rareGet<{ featured: Array<{ imageUrl: string | null }> }>("/v1/home")
      .then((h) => setMarketArt(h.featured.find((f) => f.imageUrl)?.imageUrl ?? null))
      .catch(() => {});
  }, []);

  const all = library?.games ?? [];
  const playable = all.filter((g) => g.source !== "apps");
  const apps = sortApps(all.filter((g) => g.source === "apps"));
  const favs = useFavourites((s) => s.ids);
  // Favourites lead the shelf, then everything by last played.
  const recent = [...playable].sort((a, b) => Number(favs.includes(b.id)) - Number(favs.includes(a.id)) || (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0));
  const hero = [...playable].sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0))[0];

  const links: RareLink[] = [
    { label: "Marketplace", sub: "Buy and sell collectibles", tab: "market", image: marketArt ? imageChain(marketArt, 480)[0] ?? null : null },
    { label: "Sui Games", sub: "Every game on Sui", tab: "games", image: null, mosaic: games.filter((g) => g.logoUrl).slice(0, 4).map((g) => g.logoUrl) },
    { label: "Launchpad", sub: "New drops", url: `${RARE}/launchpad`, image: rarelingsOn() ? "home/launchpad-rarelings.webp" : null },
    ...(SHOW_BETS ? [{ label: "Bets", sub: "Coinflip and more", url: `${RARE}/bets`, image: "home/coinflip.jpg" }] : []),
  ];

  return (
    <div ref={scroller} data-scroll onScroll={(e) => scrollBackdrop(e.currentTarget.scrollTop)} className="relative h-full overflow-y-auto pb-[3rem]">
      {/* The first screen: the strip on top, the game's lockup and the SUI widget at the bottom, art behind it all. */}
      <section className="relative flex h-[42.75rem] flex-col pb-[1.5rem]">
        {recent.length > 0 && <GameStrip games={recent.slice(0, 16)} total={all.length} />}
        <div className="flex min-h-0 flex-1 items-end gap-[2rem] px-[2.5rem]">
          <Stage fallback={recent[0] ?? hero} />
          <YourRare address={account?.address ?? null} />
        </div>
      </section>

      <div className="mt-[1rem] flex flex-col gap-[2.25rem]">
        <MoveCard />

        {games.length > 0 && (
          <Rail id="suigames" title="Sui Games" note="from Rare">
            {games.slice(0, 10).map((g) => (
              <SuiGameCard key={g.slug} game={g} />
            ))}
          </Rail>
        )}

        <SuiShelf />

        <Suspense fallback={null}><HomeFriends /></Suspense>

        <Rail id="collectibles" title="Your collectibles" note={account ? account.label : undefined}>
          {account && items && items.length > 0 ? (
            items.slice(0, 16).map((it) => (
              <button
                key={it.id}
                data-nav
                data-hint-a="View"
                className="tile relative aspect-square w-[9rem] shrink-0 overflow-hidden rounded-card border border-hair bg-l3"
                onClick={() => openInMarket({ kind: "item", id: it.id })}
              >
                <MImg src={it.imageUrl} label={it.name} width={256} className="absolute inset-0 h-full w-full" />
                <span className="absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/95 via-black/50 to-transparent" />
                <span className="absolute inset-x-[0.6rem] bottom-[0.5rem] text-left text-meta font-semibold text-white">
                  <span className="block truncate">{it.name}</span>
                  {it.collection && <span className="block truncate text-label font-normal text-white/60">{it.collection.name}</span>}
                </span>
              </button>
            ))
          ) : (
            <div className="w-[38rem] shrink-0">
              <Empty
                title={account ? "No collectibles yet" : "See your collectibles here"}
                body={account ? "Items you hold show up here, straight from Rare Marketplace." : "Add your Sui address to Rare Wallet to see your NFTs and game items on the home screen."}
              >
                {!account && (
                  <Button variant="primary" icon={<IconWallet size="1.1rem" />} onClick={() => useApp.getState().setTab("wallet")}>
                    Open Rare Wallet
                  </Button>
                )}
              </Empty>
            </div>
          )}
        </Rail>

        <AppsRail />

        {apps.length > 0 && (
          <Rail id="apps" title="Emulators" note={`${apps.length}`}>
            {apps.slice(0, 16).map((g) => (
              <GameTile key={g.id} game={g} />
            ))}
          </Rail>
        )}

        <OnYourPcRail />
        <section className="grid grid-cols-2 gap-[1.25rem] px-[2.5rem]">
          <TopCollections />
          <LiveSales />
        </section>

        <Rail id="rare" title="Rare">
          {links.map((q) => (
            <RareCard key={q.label} link={q} onClick={() => (q.tab ? useApp.getState().setTab(q.tab) : q.url ? void native.openWeb(q.url) : toast("Coming soon"))} />
          ))}
        </Rail>
      </div>
    </div>
  );
}

/** rare.market's featured card: square art, dark fade at the bottom, white title and subline. */
function RareCard({ link, onClick }: { link: RareLink; onClick: () => void }) {
  const [bad, setBad] = useState(false);
  const mosaic = link.mosaic && link.mosaic.length >= 4 ? link.mosaic : null;
  const art = (link.image && !bad) || !!mosaic;
  return (
    <button data-nav data-hint-a="Open" onClick={onClick} className="tile relative aspect-square w-[11rem] shrink-0 overflow-hidden rounded-card border border-hair bg-l1 text-left">
      {link.image && !bad && <img src={link.image} alt="" draggable={false} onError={() => setBad(true)} className="absolute inset-0 h-full w-full object-cover" />}
      {mosaic && (
        <span className="absolute inset-0 grid grid-cols-2 grid-rows-2">
          {mosaic.map((m) => (
            <Thumb key={m} src={m} label="" className="h-full w-full" />
          ))}
        </span>
      )}
      {art && <span className="absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/95 via-black/50 to-transparent" />}
      <span className="absolute inset-x-[1rem] bottom-[0.9rem]">
        <span className={`block truncate text-row font-bold ${art ? "text-white" : "text-fg"}`}>{link.label}</span>
        <span className={`block truncate text-meta ${art ? "text-white/60" : "text-muted"}`}>{link.sub}</span>
      </span>
    </button>
  );
}
