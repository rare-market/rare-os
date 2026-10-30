import React, { useEffect, useRef, useState } from "react";
import { Button } from "../components/ui";
import { useLive, loadTokens } from "../lib/live";
import { Dot, Num, SectionHead } from "../ui";
import { native } from "../lib/bridge";
import { RARE, bundledGames, gamesNow, loadGames, type SuiGame } from "../lib/rare";
import { exitTipOnce, launchAllowed, useApp } from "../lib/store";
import { focusFirst } from "../nav/focus";
import { exitHint, useHints } from "../nav/hints";
import { addWebGame } from "../lib/webgames";
import { OwnedActions, OwnedLine, useOwnedGames } from "../sui/ui/Owned";
import { useOwnership } from "../sui/ownership";
import { STATUS_LABEL as STATUS, installedNative, isActive, playUrl, profile, unavailable } from "../games/catalog";
import { artDecoding, isReady, whenDecoded } from "../lib/img";
import { failed } from "../lib/fail";
import { artSrc } from "../lib/state";

const PLATFORM: Record<string, string> = { web: "Web", pc: "PC", mobile: "Mobile", suiplay: "SuiPlay" };

/** A game's token on one line: symbol, price, 24 h change (the Trade API prices). */
function TokenLine({ game, className = "" }: { game: SuiGame; className?: string }) {
  const t = useLive((s) => (game.token ? s.tokens[game.token.coinType] : undefined));
  if (!game.token) return null;
  return (
    <span className={`flex items-center gap-[0.45rem] whitespace-nowrap ${className}`}>
      <span className="font-medium text-fg">${game.token.symbol}</span>
      {t?.priceUsd != null && <Num kind="price" value={t.priceUsd} live className="text-fg/80" />}
      <Num kind="change" value={t?.change24h} />
    </span>
  );
}

/**
 * Key art without baked-in text, by game. Most catalog banners are promos with words on them
 * ("Play ... On Sui Today", logos, store badges), and text never goes over text: those games show
 * their logo on the layer instead. Local files ship with the launcher.
 */
const CLEAN_ART: Record<string, string> = {
  act: "/games/act/hero.jpg",
};
// Hosted banners (rare.market/games/<slug>-banner.jpg) checked to have no text in them.
const CLEAN_BANNERS = new Set(["warped-universe", "eve-frontier", "pawtato-land", "cosmocadia", "brave-frontier-versus", "doubleup", "m10", "darktimes"]);
export function cleanArt(game: SuiGame): string | null {
  return CLEAN_ART[game.slug] ?? (CLEAN_BANNERS.has(game.slug) && usableBanner(game) ? game.bannerUrl : null);
}

/** Narrower banners are promo thumbnails: stretched over a full screen they blur, so they are never used as key art. */
export const MIN_BANNER_WIDTH = 1280;
/** A tiny logo is shown at its own size, never stretched to a tile or a banner. */
export const MIN_LOGO_WIDTH = 128;
const usableBanner = (game: Pick<SuiGame, "bannerUrl">) => !!game.bannerUrl && !/\.ico(\?|#|$)/i.test(game.bannerUrl);

/** Art that fades in once loaded and hides itself when it fails, leaving the layer behind it. */
function ArtImg({ src, className, minWidth = 0 }: { src: string; className: string; minWidth?: number }) {
  const [st, setSt] = useState(() => ({ src, failed: false, loaded: isReady(src) }));
  const cur = st.src === src ? st : { src, failed: false, loaded: isReady(src) };
  if (cur !== st) setSt(cur);
  if (cur.failed) return null;
  const ready = (el: HTMLImageElement) => {
    if (el.naturalWidth < minWidth) setSt((s) => (s.src === src ? { ...s, failed: true } : s));
    else whenDecoded(el, () => setSt((s) => (s.src === src && !s.loaded ? { ...s, loaded: true } : s)));
  };
  return (
    <img
      // WebKitGTK may not fire onLoad for a cached image; the ref catches that so it never stays hidden.
      ref={
        cur.loaded
          ? undefined
          : (el) => {
              if (el && el.complete && el.naturalWidth > 0) ready(el);
            }
      }
      src={artSrc(src, "hero")}
      alt=""
      draggable={false}
      decoding={artDecoding()}
      onLoad={(e) => ready(e.currentTarget)}
      onError={() => setSt((s) => (s.src === src ? { ...s, failed: true } : s))}
      className={`${className} object-cover transition-opacity duration-300 ${cur.loaded ? "opacity-100" : "opacity-0"}`}
    />
  );
}

/** Game logo on a rounded tile that falls back to the first letter (rare.market's GameLogo). */
export function GameLogo({ game, size }: { game: Pick<SuiGame, "name" | "logoUrl">; size: string }) {
  const [failed, setFailed] = useState(false);
  // Natural size once loaded: a logo smaller than MIN_LOGO_WIDTH keeps its own size on the calm tile.
  const [native, setNative] = useState<{ w: number; h: number } | null>(null);
  const small = native && native.w < MIN_LOGO_WIDTH ? native : null;
  return (
    <span className="flex shrink-0 items-center justify-center overflow-hidden rounded-card bg-l3 text-[1.25rem] font-semibold text-fg/50" style={{ width: size, height: size }}>
      {game.logoUrl && !failed ? (
        <img
          src={artSrc(game.logoUrl, "w512")}
          alt=""
          draggable={false}
          decoding={artDecoding()}
          onLoad={(e) => setNative({ w: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight })}
          onError={() => setFailed(true)}
          className={small ? "max-h-full max-w-full object-contain" : "h-full w-full object-cover"}
          style={small ? { width: small.w, height: small.h } : undefined}
        />
      ) : (
        game.name.slice(0, 1).toUpperCase()
      )}
    </span>
  );
}

const statusLine = (game: SuiGame) => [game.status ? STATUS[game.status] ?? game.status : "", game.platforms.map((p) => PLATFORM[p] ?? p).join(", ")].filter(Boolean).join(" · ");

/**
 * A Sui game as a store tile: logo first, like an app on a console store. Every card has the same
 * shape and never shows promo art, so a row reads as one set.
 */
export function SuiGameCard({ game, wide, def }: { game: SuiGame; wide?: boolean; def?: boolean }) {
  const owned = useOwnership((s) => s.bySlug.has(game.slug));
  return (
    <button
      data-nav
      data-hint-a="Details"
      data-nav-default={def ? "" : undefined}
      onClick={() => useApp.getState().open({ kind: "suigame", slug: game.slug })}
      className={`tile flex h-[8rem] shrink-0 flex-col justify-between rounded-card border border-hair bg-l1 p-[1rem] text-left ${wide ? "tile-wide w-full" : "w-[20rem]"}`}
    >
      <span className="flex min-w-0 items-center gap-[1rem]">
        <GameLogo game={game} size="3.5rem" />
        <span className="min-w-0">
          <span className="block truncate text-row font-semibold text-fg">{game.name}</span>
          <span className="block truncate text-small text-muted">{[game.studio, game.genre].filter(Boolean).join(" · ")}</span>
        </span>
      </span>
      <span className="flex min-w-0 items-center justify-between gap-[0.75rem] text-meta text-muted">
        {owned ? (
          <OwnedLine slug={game.slug} />
        ) : (
          <span className="flex min-w-0 items-center gap-[0.45rem]">
            {game.status === "live" && <Dot tone="primary" />}
            <span className="truncate">{statusLine(game)}</span>
          </span>
        )}
        <TokenLine game={game} />
      </span>
    </button>
  );
}

/** The first featured game with clean key art, store-front size: art on the right, words on the dark left. */
function Spotlight({ game, art }: { game: SuiGame; art: string }) {
  return (
    <button
      data-nav
      data-hint-a="Details"
      data-nav-default=""
      onClick={() => useApp.getState().open({ kind: "suigame", slug: game.slug })}
      className="tile tile-wide relative flex h-[16rem] w-full shrink-0 items-center overflow-hidden rounded-panel bg-l1 text-left"
    >
      <span className="absolute inset-y-0 left-[30%] right-0">
        <ArtImg src={art} className="absolute inset-0 h-full w-full object-[80%_center] mix-blend-lighten" />
        <span className="absolute inset-y-0 left-0 w-1/2 bg-gradient-to-r from-l1 to-transparent" />
      </span>
      <span className="relative flex max-w-[30rem] flex-col gap-[0.5rem] px-[2rem]">
        <span className="flex items-center gap-[0.5rem] text-small text-fg/70">
          {game.status === "live" && <Dot tone="primary" />}
          {[game.status ? STATUS[game.status] : "", game.studio, game.genre].filter(Boolean).join(" · ")}
        </span>
        <span className="text-display font-semibold">{game.name}</span>
        {game.description && <span className="line-clamp-2 text-small text-fg/70">{game.description}</span>}
        <TokenLine game={game} className="mt-[0.25rem] text-small" />
      </span>
    </button>
  );
}

export function SuiGames() {
  useOwnedGames();
  const [games, setGames] = useState<SuiGame[] | null>(() => gamesNow() ?? bundledGames());
  useEffect(() => {
    void loadGames().then((r) => {
      setGames(r.games);
      loadTokens(r.games.flatMap((g) => (g.token ? [g.token.coinType] : [])));
    });
  }, []);
  // Only games proven live or in beta are shown as games; the rest wait under Coming.
  const active = (games ?? []).filter(isActive);
  const featured = active.filter((g) => g.featured);
  const rest = active.filter((g) => !g.featured);
  const coming = (games ?? []).filter((g) => !isActive(g));
  const spot = featured.find((g) => cleanArt(g));
  const more = featured.filter((g) => g !== spot);
  return (
    <div data-scroll className="h-full overflow-y-auto px-[2.5rem] pb-[2.5rem] pt-[0.5rem]">
      <div className="mb-[1.25rem] flex items-end gap-[1rem]">
        <div>
          <h1 className="text-title font-semibold">Sui Games</h1>
          <p className="text-small text-muted">Games built on Sui and their collections, from Rare.{games ? ` ${active.length} games.` : ""}</p>
        </div>
        <div className="ml-auto">
          <Button size="sm" onClick={() => void native.openWeb(`${RARE}/games`)}>
            Open on Rare
          </Button>
        </div>
      </div>
      {/* The spotlight's slot is always there, so nothing above the grid moves when the live catalog arrives. */}
      {spot ? (
        <div data-nav-group="suigames-spot" className="mb-[1rem]">
          <Spotlight game={spot} art={cleanArt(spot)!} />
        </div>
      ) : (
        <div className="mb-[1rem] h-[16rem]" />
      )}
      <div data-nav-group="suigames-featured" className="grid grid-cols-3 gap-[1rem]">
        {more.map((g) => (
          <SuiGameCard key={g.slug} game={g} wide />
        ))}
      </div>
      {rest.length > 0 && (
        <>
          <SectionHead title="All games" note={`${rest.length}`} className="mb-[0.9rem] mt-[2rem]" />
          <div data-nav-group="suigames-all" className="grid grid-cols-4 gap-[1rem]">
            {rest.map((g) => (
              <SuiGameCard key={g.slug} game={g} wide />
            ))}
          </div>
        </>
      )}
      {coming.length > 0 && (
        <>
          <SectionHead title="Coming" note={`${coming.length}`} className="mb-[0.4rem] mt-[2rem]" />
          <p className="mb-[0.9rem] text-small text-muted">Announced, paused or offline for now. They move up when they're live.</p>
          <div data-nav-group="suigames-coming" className="grid grid-cols-4 gap-[1rem]">
            {coming.map((g) => (
              <SuiGameCard key={g.slug} game={g} wide />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

export function SuiGameDetail({ slug }: { slug: string }) {
  useOwnedGames();
  useHints([exitHint()]);
  const [game, setGame] = useState<SuiGame | null>(() => gamesNow()?.find((x) => x.slug === slug) ?? null);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    void loadGames().then((r) => {
      const g = r.games.find((x) => x.slug === slug) ?? null;
      setGame(g);
      if (g?.token) loadTokens([g.token.coinType]);
    });
  }, [slug]);
  useEffect(() => {
    if (game) setTimeout(() => root.current && focusFirst(root.current), 20);
  }, [game]);
  if (!game) return null;
  return <SuiGameBody game={game} root={root} />;
}

/** A Sui game's page: Play (installed native build or web game), Get it, View on Rare. */
function SuiGameBody({ game, root }: { game: SuiGame; root: React.RefObject<HTMLDivElement | null> }) {
  const library = useApp((s) => s.library);
  const url = playUrl(game);
  const playable = !!url;
  const why = unavailable(game.slug);
  const inLibrary = library?.games.find((g) => g.id === `web:${game.slug}`) ?? null;
  const nat = profile(game.slug)?.native;
  const installed = nat || game.platforms.some((p) => p === "pc" || p === "suiplay") ? installedNative(game.slug, game.name, library) : null;
  const { launch, toast } = useApp.getState();
  const steamId = nat?.steamAppIds?.[0];
  const art = cleanArt(game);
  // Art with words baked in stays in the top half, above the lockup; clean art may run under it.
  const banner = !art && usableBanner(game) ? game.bannerUrl : null;
  return (
    // Starts under the launcher's top bar (TopBar is h-[4.25rem]), so the tabs, profile and clock stay visible.
    <div ref={root} data-nav-scope className="rise fixed inset-x-0 bottom-0 top-[4.25rem] z-30 overflow-hidden bg-bg">
      {art && (
        <div className="absolute inset-y-0 left-[30%] right-0">
          <ArtImg src={art} minWidth={MIN_BANNER_WIDTH} className="absolute inset-0 h-full w-full object-[80%_30%] mix-blend-lighten" />
          <div className="absolute inset-y-0 left-0 w-1/2 bg-gradient-to-r from-bg to-transparent" />
          <div className="absolute inset-x-0 top-0 h-[3rem] bg-gradient-to-b from-bg to-transparent" />
          <div className="absolute inset-x-0 bottom-0 h-[60%] bg-gradient-to-t from-bg via-bg/85 to-transparent" />
        </div>
      )}
      {banner && (
        <div className="absolute inset-x-0 top-0 h-[52%]">
          <ArtImg src={banner} minWidth={MIN_BANNER_WIDTH} className="absolute inset-0 h-full w-full object-[center_30%]" />
          <div className="absolute inset-x-0 top-0 h-[3rem] bg-gradient-to-b from-bg/70 to-transparent" />
          <div className="absolute inset-x-0 bottom-0 h-[70%] bg-gradient-to-t from-bg to-transparent" />
        </div>
      )}
      <div className="relative flex h-full flex-col justify-end gap-[1rem] px-[2.5rem] pb-[4.5rem]">
        <div className="flex items-center gap-[1rem]">
          <GameLogo game={game} size="4.5rem" />
          <div className="flex flex-col gap-[0.25rem]">
            <div className="text-small font-medium text-fg/70">{[game.studio, game.genre].filter(Boolean).join(" · ")}</div>
            <h1 className="text-[3rem] font-semibold leading-none tracking-[-0.035em]">{game.name}</h1>
          </div>
        </div>
        {game.description && <p className="max-w-[44rem] text-body leading-relaxed text-fg/80">{game.description}</p>}
        <div className="flex flex-wrap items-center gap-[1.25rem] text-small text-fg/75">
          {game.status && (
            <span className="flex items-center gap-[0.45rem]">
              {game.status === "live" && <Dot tone="primary" />}
              {STATUS[game.status] ?? game.status}
            </span>
          )}
          {game.platforms.length > 0 && <span>{game.platforms.map((p) => PLATFORM[p] ?? p).join(", ")}</span>}
          <TokenLine game={game} />
          <OwnedLine slug={game.slug} />
          {game.collections.length > 0 && <span>{game.collections.map((c) => c.name).join(", ")}</span>}
        </div>
        {why && <p className="max-w-[44rem] text-small text-fg/80">{why}</p>}
        {!installed && nat && <p className="max-w-[44rem] text-small text-fg/80">{nat.how}</p>}
        <div className="flex gap-[0.75rem]">
          {installed && (
            <Button variant="primary" def onClick={() => void launch(installed)}>
              Play
            </Button>
          )}
          {!installed && url && (
            <>
              <Button
                variant="primary"
                def
                onClick={() =>
                  void (inLibrary
                    ? launch(inLibrary)
                    : launchAllowed() &&
                      native.webgameOpen(game.slug, game.name, url).then((browser) => (browser && toast(`Opening in ${browser}`), exitTipOnce())).catch((e) => toast(failed("Couldn't open the game.", e), "error")))
                }
              >
                Play now
              </Button>
              {!inLibrary && (
                <Button
                  onClick={() =>
                    void addWebGame({ id: game.slug, title: game.name, url, cover: game.logoUrl || null, hero: game.bannerUrl || null })
                      .then(() => toast(`${game.name} is in your library`))
                      .catch((e) => toast(failed("Couldn't add the game to your library.", e), "error"))
                  }
                >
                  Add to library
                </Button>
              )}
            </>
          )}
          {!installed && nat && (
            <Button
              variant={url ? "secondary" : "primary"}
              def={!url}
              onClick={() =>
                void (steamId ? native.steamStore(steamId) : native.webgameOpenBrowser(`store-${game.slug}`, nat.storeUrl).then((b) => toast(`Opening ${nat.store} in ${b}`))).catch((e) =>
                  toast(failed("Couldn't open the store.", e), "error"),
                )
              }
            >
              Get it on {nat.store}
            </Button>
          )}
          <Button variant={playable || installed || nat ? "secondary" : "primary"} def={!playable && !installed && !nat} onClick={() => void native.openWeb(`${RARE}/games/${game.slug}`)}>
            View on Rare
          </Button>
          <OwnedActions game={game} />
          {game.collections.length > 0 && <Button onClick={() => void native.openWeb(`${RARE}/games/${game.slug}`)}>Collections</Button>}
        </div>
      </div>
    </div>
  );
}
