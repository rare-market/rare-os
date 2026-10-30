import { useEffect, useRef, useState } from "react";
import { Art } from "../components/Art";
import { Button, fmtAgo, fmtSize } from "../components/ui";
import { CardRow, Dot, InfoCard } from "../ui";
import { useFavourites } from "../lib/favourites";
import { usePadHandler } from "../nav/input";
import { exitHint, useHints } from "../nav/hints";
import { feedback } from "../nav/feedback";
import { EmulationPanel, PlayButtons, ProtonPanel, WebGameInfo } from "../games/GamePanels";
import { native } from "../lib/bridge";
import { gameById, useApp } from "../lib/store";
import { SOURCE_LABEL, prettyPath } from "../lib/types";
import { focusFirst } from "../nav/focus";
import ANTICHEAT from "../lib/anticheat.json";
import { removeWebGame } from "../lib/webgames";
import { BackgroundSwitch } from "../overlay/BackgroundSwitch";

const AC = ANTICHEAT as unknown as Record<string, [string, string[]]>;

/** Anti-cheat status on Linux from AreWeAntiCheatYet, for Steam games. */
function antiCheat(id: string): { tone: "ok" | "bad"; text: string } | null {
  if (!id.startsWith("steam:")) return null;
  const e = AC[id.slice(6)];
  if (!e) return null;
  const [status, list] = e;
  const names = list.join(", ");
  if (status === "Broken" || status === "Denied")
    return {
      tone: "bad",
      text:
        status === "Denied"
          ? `Online play won't run here: the publisher blocks Linux in its anti-cheat (${names}). Offline modes may still work.`
          : `Online play won't run here: its anti-cheat (${names}) doesn't support Linux yet. Offline modes may still work.`,
    };
  if (status === "Supported") return { tone: "ok", text: `${names} supports Linux and Proton.` };
  if (status === "Running") return { tone: "ok", text: `Runs with ${names} on Proton, unofficially.` };
  return null;
}

const fmtPlaytime = (m?: number | null) => (!m ? "" : m < 60 ? `${m} min` : `${Math.round(m / 6) / 10} h`);
import { HeroLogo } from "./Home";
import { failed } from "../lib/fail";

const PHASE_TEXT = { steam: "Starting Steam", game: "Starting the game" } as const;

/** A thin bar under the actions while a Steam game starts: Steam first (a cold start is about 6 s,
 *  a client update 30 s or more), then the game. Never a frozen page. */
function StartProgress({ id }: { id: string }) {
  const [phase, setPhase] = useState<"steam" | "game" | "playing" | "none">("none");
  const [secs, setSecs] = useState(0);
  useEffect(() => {
    let live = true;
    const t0 = Date.now();
    const tick = () => {
      void native.launchPhase(id).then((p) => live && setPhase(p)).catch(() => {});
      setSecs(Math.floor((Date.now() - t0) / 1000));
    };
    tick();
    const h = setInterval(tick, 700);
    return () => {
      live = false;
      clearInterval(h);
    };
  }, [id]);
  if (phase !== "steam" && phase !== "game") return null;
  return (
    <div className="max-w-[26rem]" role="progressbar" aria-label={PHASE_TEXT[phase]}>
      <div className="relative h-[0.2rem] overflow-hidden rounded-full bg-fg/15">
        <div className="absolute inset-y-0 w-[35%] rounded-full bg-fg/80" style={{ animation: "start-slide 1.3s ease-in-out infinite" }} />
      </div>
      <div className="mt-[0.35rem] text-small text-muted">
        {PHASE_TEXT[phase]}
        {phase === "steam" && secs >= 15 ? ", it is updating" : ""}
      </div>
      <style>{"@keyframes start-slide{0%{left:-35%}100%{left:100%}}"}</style>
    </div>
  );
}

export function GameDetail({ id }: { id: string }) {
  const game = useApp((s) => s.library?.games.find((g) => g.id === id) ?? gameById(id));
  const running = useApp((s) => s.running.includes(id));
  const emulation = useApp((s) => (s.library?.emulationApp === id ? s.library?.emulation ?? null : null));
  const toast = useApp((s) => s.toast);
  const root = useRef<HTMLDivElement>(null);
  const fav = useFavourites((s) => s.ids.includes(id));
  usePadHandler((b) => {
    if (b !== "x") return false;
    feedback("toggle");
    useFavourites.getState().toggle(id);
    useApp.getState().toast(fav ? "Removed from favourites" : "Added to favourites");
    return true;
  });
  useHints([{ b: "x", label: fav ? "Unfavourite" : "Favourite" }, { b: "start", label: "Options" }, exitHint()]);

  useEffect(() => {
    setTimeout(() => root.current && focusFirst(root.current), 20);
  }, []);

  // Selecting a Steam game starts Steam quietly (setting "When Steam starts"), so Play lands on a
  // warm client. A short dwell keeps a pass through the list from starting it.
  const steamGame = game?.source === "steam";
  useEffect(() => {
    if (!steamGame || running) return;
    const t = setTimeout(() => void native.steamPrewarm().catch(() => {}), 800);
    return () => clearTimeout(t);
  }, [steamGame, running]);

  if (!game) return null;
  const facts: Array<{ label: string; value: string | null; sub?: string }> = [
    { label: "Last played", value: game.lastPlayed ? fmtAgo(game.lastPlayed).replace(/^Played /, "") : "Not yet" },
    { label: "Playtime", value: fmtPlaytime(game.playtimeMinutes) || "None yet" },
    { label: "Size", value: fmtSize(game.sizeBytes) || (game.source === "web" ? "Streamed" : null), sub: game.source === "web" ? "Plays from the web" : undefined },
    { label: "Store", value: SOURCE_LABEL[game.source], sub: prettyPath(game.origin) !== SOURCE_LABEL[game.source] ? prettyPath(game.origin) : undefined },
  ];
  const ac = antiCheat(game.id);

  return (
    <div ref={root} data-nav-scope className="rise fixed inset-0 z-30 bg-bg">
      {/* Key art fills the top two thirds; plain scrims keep the lockup and cards legible. */}
      <Art sources={game.art.hero.length ? game.art.hero : game.art.header} title={game.title} variant="hero" className="absolute inset-x-0 top-0 h-[72%] w-full object-cover object-[center_25%] mix-blend-lighten" />
      <div className="absolute inset-x-0 top-[40%] h-[32.5%] bg-gradient-to-b from-transparent to-bg" />
      <div className="absolute inset-y-0 left-0 w-[60%] bg-gradient-to-r from-bg/90 via-bg/50 to-transparent" />
      <div className="relative flex h-full flex-col justify-end gap-[1rem] px-[2.5rem] pb-[4.75rem]">
        <div className="flex items-center gap-[0.5rem] text-small font-medium text-fg/75">
          {running && <Dot tone="primary" live />}
          {running ? "Now playing" : SOURCE_LABEL[game.source]}
          {fav && <span className="text-fg/50">· Favourite</span>}
        </div>
        <HeroLogo sources={game.art.logo} title={game.title} big />
        <div data-nav-group="game-actions" data-nav-remember className="mt-[0.4rem] flex gap-[0.75rem]">
          <PlayButtons game={game} />
          {game.foreign && (
            <Button
              size="lg"
              onClick={async () => {
                const lib = game.origin.replace(/^Steam library on /, "");
                try {
                  await native.steamAddLibrary(lib);
                  toast("Added to Steam");
                } catch (e) {
                  toast(failed("Couldn't add the library to Steam.", e), "error");
                }
              }}
            >
              Add library to Steam
            </Button>
          )}
          {game.source === "steam" && /^steam:\d+$/.test(game.id) && (
            <Button size="lg" variant="ghost" onClick={() => useApp.getState().open({ kind: "trophies", appid: Number(game.id.slice(6)), from: { kind: "game", id: game.id } })}>
              Trophies
            </Button>
          )}
          {game.source === "steam" && (
            <Button size="lg" variant="ghost" onClick={() => void native.steamBigPicture()}>
              Open in Steam
            </Button>
          )}
          {game.source === "web" && (
            <Button size="lg" variant="ghost" onClick={() => void removeWebGame(game.id).then(() => useApp.getState().close())}>
              Remove
            </Button>
          )}
        </div>
        {running && steamGame && <StartProgress id={id} />}
        {ac && (
          <div className="flex max-w-[52rem] items-start gap-[0.6rem] text-small text-fg/80">
            <Dot tone={ac.tone === "bad" ? "attention" : "muted"} className="mt-[0.45rem]" />
            {ac.text}
          </div>
        )}
        <BackgroundSwitch id={game.id} />
        {game.source === "steam" && !game.foreign && /^steam:\d+$/.test(game.id) && !running && <ProtonPanel appid={Number(game.id.slice(6))} />}
        {emulation && <EmulationPanel emulation={emulation} />}
        {game.source === "web" && <WebGameInfo game={game} url={game.launch?.url ?? null} />}
        {/* Activity cards: one fact per card, solid, under the actions. */}
        <CardRow id="game-cards" className="mt-[0.75rem]">
          {facts.map((f) => (f.value ? <InfoCard key={f.label} label={f.label} value={f.value} sub={f.sub} /> : null))}
        </CardRow>
      </div>
    </div>
  );
}
