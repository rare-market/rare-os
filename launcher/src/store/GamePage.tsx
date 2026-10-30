// A Store game's page (overlay "storegame"), in the Sui Games page's layout. Link listings open
// their play, Steam, Epic or website link the way Sui Games does. Uploaded games show the price
// and Buy; once owned: Install, Update, Play, Refund while refundable, Make tradable and List
// when the studio allows resale.

import { useEffect, useRef, useState } from "react";
import { native } from "../lib/bridge";
import { exitTipOnce, launchAllowed, useApp } from "../lib/store";
import { focusFirst } from "../nav/focus";
import { askPrice } from "../overlay/price";
import { cleanArt, GameLogo } from "../screens/SuiGames";
import { Button, Dot, LoadBar, StatLine } from "../ui";
import { canSignWithWallet, SignRejected } from "../wallet/market";
import { install, listLicence, makeTradable, plain, refund, buy } from "./actions";
import { api, canResell, canSell } from "./api";
import { readGame, releaseFor } from "./chain";
import { platformText, priceNow, refundable, sizeText, steamAppId, timeLeft, tradableNow, updateDecision, usdText } from "./logic";
import { BuySheet } from "./BuySheet";
import { linkWhere, logoOf, PriceText, useLoad } from "./parts";
import { useStore, useStoreFresh } from "./state";
import type { ChainRelease, Licence, StoreGameDetail } from "./types";
import { failed } from "../lib/fail";

const STATUS: Record<string, string> = { live: "Live", beta: "Beta" };

export function StoreGamePage({ slug }: { slug: string }) {
  useStoreFresh();
  const d = useLoad(`game:${slug}`, () => api.game(slug), 30_000);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (d.data) setTimeout(() => root.current && focusFirst(root.current), 20);
  }, [!!d.data]); // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <div ref={root} data-nav-scope className="rise fixed inset-0 z-30 bg-bg">
      <LoadBar on={d.loading} />
      {d.data ? (
        <Body game={d.data} />
      ) : (
        d.error && (
          <div className="flex h-full flex-col justify-end gap-[0.5rem] px-[2.5rem] pb-[4.5rem]">
            <h1 className="text-title font-semibold">Couldn't load this game</h1>
            <p className="text-small text-muted">Check the connection and try again.</p>
          </div>
        )
      )}
    </div>
  );
}

function Body({ game }: { game: StoreGameDetail }) {
  const art = game.kind === "link" ? cleanArt({ slug: game.slug, bannerUrl: game.images.banner ?? "" } as Parameters<typeof cleanArt>[0]) : game.images.banner;
  const about = game.page?.about || game.description;
  const licence = useStore((s) => s.licences.find((l) => l.slug === game.slug) ?? null);
  return (
    <>
      {art && (
        <div className="absolute inset-y-0 left-[30%] right-0">
          <img src={art} alt="" draggable={false} className="absolute inset-0 h-full w-full object-cover object-[80%_30%] mix-blend-lighten" />
          <div className="absolute inset-y-0 left-0 w-1/2 bg-gradient-to-r from-bg to-transparent" />
          <div className="absolute inset-x-0 bottom-0 h-[60%] bg-gradient-to-t from-bg via-bg/85 to-transparent" />
        </div>
      )}
      <div className="relative flex h-full flex-col justify-end gap-[1rem] px-[2.5rem] pb-[4.5rem]">
        <div className="flex items-center gap-[1rem]">
          <GameLogo game={logoOf(game)} size="4.5rem" />
          <div className="flex flex-col gap-[0.25rem]">
            <div className="text-small font-medium text-fg/70">{[game.studio.name, game.genre].filter(Boolean).join(" · ")}</div>
            <h1 className="text-[3rem] font-semibold leading-none tracking-[-0.035em]">{game.name}</h1>
          </div>
        </div>
        {about && <p className="line-clamp-3 max-w-[44rem] text-body leading-relaxed text-fg/80">{about}</p>}
        <div className="flex flex-wrap items-center gap-[1.25rem] text-small text-fg/75">
          {game.status && (
            <span className="flex items-center gap-[0.45rem]">
              {game.status === "live" && <Dot tone="primary" />}
              {STATUS[game.status] ?? game.status}
            </span>
          )}
          {game.kind === "link" ? <span>{linkWhere(game)}</span> : game.platforms.length > 0 && <span>{platformText(game.platforms)}</span>}
          {game.page?.tags?.length ? <span>{game.page.tags.join(", ")}</span> : null}
          {licence && <span>In your library</span>}
        </div>
        {game.kind === "link" ? <LinkActions game={game} /> : <Owned game={game} licence={licence} />}
      </div>
    </>
  );
}

function LinkActions({ game }: { game: StoreGameDetail }) {
  const toast = useApp((s) => s.toast);
  const library = useApp((s) => s.library);
  const inLibrary = library?.games.find((g) => g.id === `web:${game.slug}`) ?? null;
  const steam = steamAppId(game.links.steam);
  const fail = (e: unknown) => toast(failed("Couldn't open that.", e), "error");
  const actions: Array<{ label: string; run: () => void }> = [];
  if (game.links.play) {
    const url = game.links.play;
    actions.push({
      label: "Play now",
      run: () =>
        void (inLibrary ? useApp.getState().launch(inLibrary) : !launchAllowed() ? Promise.resolve() : native.webgameOpen(game.slug, game.name, url).then((b) => (b && toast(`Opening in ${b}`), exitTipOnce()))).catch(fail),
    });
  }
  if (game.links.steam) {
    const url = game.links.steam;
    actions.push({ label: "Get it on Steam", run: () => void (steam ? native.steamStore(steam) : native.webgameOpenBrowser(`store-${game.slug}`, url).then((b) => toast(`Opening Steam in ${b}`))).catch(fail) });
  }
  if (game.links.epic) {
    const url = game.links.epic;
    actions.push({ label: "Get it on Epic Games", run: () => void native.webgameOpenBrowser(`store-${game.slug}`, url).then((b) => toast(`Opening Epic Games in ${b}`)).catch(fail) });
  }
  if (game.links.website) {
    const url = game.links.website;
    actions.push({ label: "Website", run: () => void native.webgameOpenBrowser(`site-${game.slug}`, url).then((b) => toast(`Opening in ${b}`)).catch(fail) });
  }
  if (!actions.length) return <p className="text-small text-muted">This listing has no links yet.</p>;
  return (
    <div data-nav-group="storegame-actions" className="flex gap-[0.75rem]">
      {actions.map((a, i) => (
        <Button key={a.label} variant={i === 0 ? "primary" : "secondary"} def={i === 0} onClick={a.run}>
          {a.label}
        </Button>
      ))}
    </div>
  );
}

type Note = { text: string; tone?: "error" } | null;

function Owned({ game, licence }: { game: StoreGameDetail; licence: Licence | null }) {
  const config = useStore((s) => s.config);
  const account = useStore((s) => s.account);
  const installed = useStore((s) => s.installed.find((i) => i.slug === game.slug) ?? null);
  const progress = useStore((s) => s.progress[game.slug]);
  const busy = useStore((s) => s.busy[game.slug]);
  const libGame = useApp((s) => s.library?.games.find((g) => g.id === `rare:${game.slug}`) ?? null);
  const running = useApp((s) => s.running.includes(`rare:${game.slug}`));
  const [buying, setBuying] = useState(false);
  const [note, setNote] = useState<Note>(null);
  const [working, setWorking] = useState<string | null>(null);
  // The release this licence may download, read from the chain: drives Install and Update.
  const [release, setRelease] = useState<ChainRelease | null | undefined>(undefined);
  useEffect(() => {
    if (!licence) return setRelease(undefined);
    let live = true;
    void readGame(licence.game_id)
      .then((g) => releaseFor(g, licence.edition))
      .then((r) => live && setRelease(r), () => live && setRelease(undefined));
    return () => {
      live = false;
    };
  }, [licence?.game_id, licence?.edition]); // eslint-disable-line react-hooks/exhaustive-deps

  const canSign = canSignWithWallet(account);
  const price = priceNow(game);
  const selling = canSell(config) && game.listed && !game.blocked && !!game.game_id;
  const decision = licence ? updateDecision(installed, release) : null;
  const now = Date.now();

  const run = async (label: string, f: () => Promise<unknown>, done?: string) => {
    if (working || busy) return;
    if (!account || !canSign) {
      setNote({ text: account ? "This account is watch only. Switch to a Rare Wallet account in Wallet." : "Add a Rare Wallet account in Wallet first." });
      return;
    }
    setWorking(label);
    setNote(null);
    try {
      await f();
      if (done) setNote({ text: done });
    } catch (e) {
      if (!(e instanceof SignRejected)) setNote({ text: plain(e), tone: "error" });
    } finally {
      setWorking(null);
    }
  };

  const doInstall = () => run("install", () => install(game, licence!, account!), `${game.name} is installed.`);
  const play = () => libGame && void useApp.getState().launch(libGame);
  const size = release?.builds.find((b) => b.platform === "linux-x86_64")?.size ?? release?.builds[0]?.size;

  const buttons: Array<{ label: string; run: () => void; variant?: "primary" | "secondary" | "ghost"; disabled?: boolean }> = [];
  if (!licence) {
    if (!selling) buttons.push({ label: "Coming soon", run: () => undefined, variant: "primary", disabled: true });
    else if (price === 0n) buttons.push({ label: "Get for free", variant: "primary", run: () => void run("buy", () => buy(game, "free", account!), `${game.name} is in your library.`) });
    else if (price != null) buttons.push({ label: `Buy for ${usdText(price)}`, variant: "primary", run: () => (account && canSign ? setBuying(true) : void run("buy", async () => undefined)) });
  } else {
    if (installed && libGame) buttons.push({ label: running ? "Resume" : "Play", variant: decision === "update" ? "secondary" : "primary", run: play });
    if (decision === "update") buttons.unshift({ label: "Update", variant: "primary", run: doInstall });
    if (!installed && decision !== "unavailable")
      buttons.push({ label: `Install${size ? ` (${sizeText(size)})` : ""}`, variant: "primary", run: doInstall });
    if (refundable(licence, now) && canSell(config)) buttons.push({ label: "Refund", run: () => void run("refund", () => refund(game, licence, account!), "Refunded. The payment is back in your wallet.") });
    if (licence.type === "licence" && game.resale?.enabled && tradableNow(licence, now) && canResell(config))
      buttons.push({ label: "Make tradable", run: () => void run("trade", () => makeTradable(game, licence, account!), "Tradable. It's in your kiosk, ready to list.") });
    if (licence.type === "resale")
      buttons.push({
        label: "List for sale",
        run: () =>
          void run("list", async () => {
            const p = await askPrice(0n);
            if (p != null) await listLicence(game, licence, p, account!);
          }),
      });
    if (installed) buttons.push({ label: "Uninstall", variant: "ghost", run: () => void run("uninstall", () => import("./native").then(({ storeNative }) => storeNative.uninstall(game.slug)).then(() => useStore.getState().refreshInstalled()).then(() => useApp.getState().scan({ quiet: true }))) });
  }

  const status =
    progress && progress.phase === "download"
      ? `Downloading ${Math.floor((progress.done / Math.max(1, progress.total)) * 100)}%, ${sizeText(progress.done)} of ${sizeText(progress.total)}`
      : progress?.phase === "verify"
        ? "Checking the download against the chain"
        : progress?.phase === "unpack"
          ? "Installing"
          : busy
            ? busy
            : working === "buy" || working === "refund" || working === "trade" || working === "list"
              ? "Review and approve in Rare Wallet."
              : null;

  // Facts in one stat line; anything unknown is left out.
  const facts = [
    { label: "Price", value: price != null ? <PriceText game={game} /> : null },
    { label: "Version", value: (release ?? game.latest)?.label ? <span className="num">{(release ?? game.latest)!.label}</span> : null },
    { label: "Size", value: size ? sizeText(size) : game.latest?.builds[0] ? sizeText(BigInt(game.latest.builds[0].size)) : null },
    { label: "Your licence", value: licence ? `#${licence.number}` : null },
    { label: "Refund", value: licence && refundable(licence, now) ? `${timeLeft(licence.refundable_until_ms! - now)} left` : null },
    { label: "Resale", value: game.resale?.enabled ? `${game.resale.royalty_bps / 100}% royalty` : null },
    { label: "Sales", value: game.sales ? game.sales.toLocaleString("en-US") : null },
  ];

  const hold = licence && licence.type === "licence" && game.resale?.enabled && licence.tradable_from_ms && now < licence.tradable_from_ms ? `Tradable from ${new Date(licence.tradable_from_ms).toLocaleDateString([], { month: "short", day: "numeric" })}.` : null;
  const notes = [
    !licence && selling && price != null && price > 0n ? "Refundable in full for 48 hours." : null,
    !licence && selling && game.price?.accept_sui ? "Pay in USDC or SUI." : null,
    hold,
    licence && refundable(licence, now) && game.resale?.enabled ? "Making it tradable ends the refund window." : null,
    licence && game.launch_check ? "Checks your licence before each launch. Works offline for 7 days." : null,
    licence && decision === "unavailable" ? "No build for your edition yet." : null,
  ].filter(Boolean);

  return (
    <>
      <div data-nav-group="storegame-actions" className="mt-[0.25rem] flex gap-[0.75rem]">
        {buttons.map((b, i) => (
          <Button key={b.label} variant={b.variant ?? "secondary"} def={i === 0} disabled={b.disabled || !!working || (!!busy && b.label !== "Play")} onClick={b.run}>
            {b.label}
          </Button>
        ))}
      </div>
      {(note || status) && (
        <p role="status" className={`max-w-[44rem] text-small tabular-nums ${note?.tone === "error" ? "text-negative" : "text-fg/80"}`}>
          {status ?? note?.text}
        </p>
      )}
      {notes.length > 0 && <p className="max-w-[44rem] text-small text-muted">{notes.join(" ")}</p>}
      <div className="mt-[0.4rem] max-w-[60rem] border-t border-hair pt-[1rem]">
        <StatLine items={facts} size="lg" />
      </div>
      {buying && account && (
        <BuySheet
          game={game}
          account={account}
          onClose={() => setBuying(false)}
          onBought={() => {
            setBuying(false);
            setNote({ text: `${game.name} is in your library. Install it from here.` });
          }}
        />
      )}
    </>
  );
}
