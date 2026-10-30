// The Switcher: hold the Rare button (or View + Menu) for a full-screen row of cards, the Xbox task
// view. Home first, then everything running,
// most recently used first, each as the still frame gamescope took when you left it; then up to
// four recent games that aren't running, smaller. A switches, X closes (X again confirms), B goes
// back to where you were. A game you switched away from is paused (freeze.rs) and its card says so.
//
// Over a game it is the native overlay (the session's frame behind it); over the launcher it is the
// launcher's own sheet. Also here: "Close a paused game?", when leaving a game would make three.

import { useEffect, useMemo, useRef, useState } from "react";
import { fileSrc } from "../lib/bridge";
import { useApp } from "../lib/store";
import type { Game } from "../lib/types";
import { Art } from "../components/Art";
import { AppIcon } from "../apps/tiles";
import { quitItem, resumeItem, useRunningItems, type RunningItem } from "../apps/sheets";
import { focusFirst } from "../nav/focus";
import { feedback } from "../nav/feedback";
import { usePadHandler } from "../nav/input";
import { Sheet } from "../trade/parts";
import { Button, Glyph, Hint } from "../ui";
import { overlayNative, onFreezeFull, type FreezeFull, type SwitcherState } from "./native";
import { catalogFor } from "./game";
import { useGames } from "./panels";
import { inOrder, recentNotRunning } from "./menuModel";
import { closeOverlay, type Session } from "./session";

function useSwitcherState(): SwitcherState | null {
  const running = useApp((s) => s.running);
  const [st, setSt] = useState<SwitcherState | null>(null);
  useEffect(() => {
    let live = true;
    void overlayNative.switcherState().then((s) => live && setSt(s), () => live && setSt({ cards: [], from: null, maxPaused: 2 }));
    return () => {
      live = false;
    };
  }, [running]);
  return st;
}

/** `session` is the native overlay over a game; null is the launcher's own Switcher. */
export function Switcher({ session, onClose }: { session: Session | null; onClose?: () => void }) {
  const library = useApp((s) => s.library);
  const running = useApp((s) => s.running);
  const sui = useGames();
  const st = useSwitcherState();
  const all = useRunningItems((rid) => catalogFor(rid, library, sui)?.name ?? null);
  const items = useMemo(() => inOrder(all, st?.cards.map((c) => c.rid) ?? []), [all, st]);
  const recent = useMemo(() => recentNotRunning(library?.games ?? [], running), [library, running]);
  const [armed, setArmed] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const from = session?.gameId ?? null;

  useEffect(() => {
    feedback("open");
  }, []);
  // Focus lands on the game you were in (one press of A is "back"), else on Home.
  const ready = !!st;
  useEffect(() => {
    if (!ready) return;
    const t = setTimeout(() => {
      const el = (from && root.current?.querySelector<HTMLElement>(`[data-switch="${CSS.escape(from)}"]`)) || null;
      if (el) el.focus();
      else if (root.current) focusFirst(root.current);
    }, 40);
    return () => clearTimeout(t);
  }, [ready]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(null), 4000);
    return () => clearTimeout(t);
  }, [armed]);

  const back = () => {
    feedback("back");
    if (session) void closeOverlay();
    else onClose?.();
  };
  /** A card picked: null is Home. */
  const pick = async (it: RunningItem | null) => {
    feedback("select");
    if (it && it.rid === from) return back();
    if (session) await overlayNative.switch(it?.rid ?? null).catch(() => {});
    else onClose?.();
    if (it) resumeItem(it);
    else useApp.getState().setTab("home");
  };
  const launch = async (g: Game) => {
    feedback("select");
    if (session) await overlayNative.switch(null).catch(() => {});
    else onClose?.();
    void useApp.getState().launch(g);
  };

  usePadHandler((b) => {
    switch (b) {
      case "b":
        back();
        return true;
      case "x": {
        const rid = (document.activeElement as HTMLElement | null)?.dataset.switch;
        const it = rid ? items.find((x) => x.rid === rid) : null;
        if (!it) return true;
        if (armed !== it.rid) {
          setArmed(it.rid);
          feedback("select");
          return true;
        }
        setArmed(null);
        quitItem(it);
        feedback("back");
        return true;
      }
      case "lb":
      case "rb":
      case "lt":
      case "rt":
      case "y":
      case "select":
      case "start":
      case "exithold":
      case "guidehold":
        return true;
      default:
        return false;
    }
  });

  const card = (rid: string) => st?.cards.find((c) => c.rid === rid) ?? null;
  const backdrop = session?.frame ? fileSrc(session.frame) : null;

  return (
    <div className="fixed inset-0 z-40 flex flex-col">
      <div className="absolute inset-0 overflow-hidden bg-bg" aria-hidden>
        {backdrop && <img src={backdrop} alt="" className="fade-in absolute inset-0 size-full object-cover" />}
        <div className="absolute inset-0 bg-black/80" />
      </div>
      <div ref={root} data-nav-scope className="fade-in relative flex min-h-0 flex-1 flex-col justify-center gap-[2.25rem] px-[2.5rem]">
        <div data-nav-group="switcher-running" className="flex items-start gap-[1.25rem] overflow-x-auto py-[0.75rem]">
          <SwitchCard title="Home" status="" onClick={() => void pick(null)} rid={null}>
            <span className="absolute inset-0 flex items-center justify-center bg-l2 text-title font-semibold text-fg/90">Home</span>
          </SwitchCard>
          {items.map((it) => {
            const c = card(it.rid);
            const status = armed === it.rid ? "Press X again to close" : c?.paused ? "Paused" : it.app ? "Open" : "Running";
            return (
              <SwitchCard key={it.rid} rid={it.rid} title={it.name} status={status} onClick={() => void pick(it)}>
                <CardArt it={it} frame={c?.frame ?? null} />
              </SwitchCard>
            );
          })}
        </div>
        {recent.length > 0 && (
          <section className="flex flex-col gap-[0.6rem]">
            <div className="text-label font-medium text-secondary">Recent</div>
            <div data-nav-group="switcher-recent" className="flex gap-[1rem]">
              {recent.map((g) => (
                <button key={g.id} data-nav data-hint-a="Play" onClick={() => void launch(g)} className="tile relative aspect-[2/3] w-[7.5rem] shrink-0 overflow-hidden rounded-card bg-l3">
                  <Art sources={g.art.cover} title={g.title} variant="cover" className="absolute inset-0 size-full object-cover" />
                </button>
              ))}
            </div>
          </section>
        )}
      </div>
      <footer className="relative flex h-[3rem] shrink-0 items-center gap-[1.25rem] border-t border-hair bg-bg/60 px-[2.5rem] text-small text-fg/75">
        <span className="flex items-center gap-[0.35rem]">
          <Glyph b="rare" size="sm" />+<Glyph b="y" size="sm" /> Screenshot
        </span>
        <span className="flex items-center gap-[0.35rem]">
          <Glyph b="rare" size="sm" />+<Glyph b="select" size="sm" /> Save clip
        </span>
        <span className="ml-auto flex items-center gap-[1.25rem]">
          <Hint b="a">Switch</Hint>
          <Hint b="x">Close</Hint>
          <Hint b="b">Back</Hint>
        </span>
      </footer>
    </div>
  );
}

function SwitchCard({ rid, title, status, onClick, children }: { rid: string | null; title: string; status: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <div className="w-[22rem] shrink-0">
      <button data-nav data-switch={rid ?? "home"} data-hint-a={rid ? "Switch" : "Home"} data-hint-x={rid ? "Close" : undefined} onClick={onClick} className="tile tile-wide relative block aspect-[16/10] w-full overflow-hidden rounded-card bg-l3">
        {children}
      </button>
      <div className="mt-[0.7rem] px-[0.1rem]">
        <span className="focus-text block truncate text-body font-medium">{title}</span>
        {status && <span className="block truncate text-small text-secondary">{status}</span>}
      </div>
    </div>
  );
}

/** The still frame from when you left it; else the game's art, or the app's icon. */
function CardArt({ it, frame }: { it: RunningItem; frame: string | null }) {
  if (frame) return <img src={fileSrc(frame)} alt="" className="absolute inset-0 size-full object-cover" />;
  if (it.app)
    return (
      <span className="absolute inset-0 flex items-center justify-center bg-l2">
        <AppIcon app={it.app} className="h-[4.5rem] w-[4.5rem]" />
      </span>
    );
  const src = it.stream?.app.art ? [it.stream.app.art] : it.game ? [...it.game.art.hero, ...it.game.art.header, ...it.game.art.cover] : [];
  return <Art sources={src} title={it.name} variant="hero" className="absolute inset-0 size-full object-cover" />;
}

/** Leaving a game would make three paused: close one, or keep the new one running. */
export function PausedFull() {
  const [ask, setAsk] = useState<FreezeFull | null>(null);
  const all = useRunningItems();
  const running = useApp((s) => s.running);
  const box = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    let un: (() => void) | null = null;
    void onFreezeFull(setAsk).then((u) => (un = u));
    return () => un?.();
  }, []);
  // Done once one of them has gone (the native side pauses the waiting game then).
  useEffect(() => {
    if (ask && ask.frozen.some((r) => !running.includes(r))) setAsk(null);
  }, [ask, running]);
  useEffect(() => {
    if (!ask) return;
    const t = setTimeout(() => box.current && focusFirst(box.current), 30);
    return () => clearTimeout(t);
  }, [ask]);
  const keep = () => {
    feedback("back");
    void overlayNative.keepPending();
    setAsk(null);
  };
  usePadHandler(
    (b) => {
      if (b === "b") {
        keep();
        return true;
      }
      return b === "lb" || b === "rb" || b === "y" || b === "select" || b === "start";
    },
    !!ask,
  );
  if (!ask) return null;
  const name = (rid: string) => all.find((x) => x.rid === rid)?.name ?? "A game";
  return (
    <Sheet
      onRef={(el) => (box.current = el)}
      title="Close a paused game?"
      sub={`Two games are paused. Close one and ${name(ask.pending)} pauses too, or keep it running.`}
      footer={
        <Button variant="secondary" hint="Keep it running" onClick={keep}>
          Keep {name(ask.pending)} running
        </Button>
      }
    >
      <div data-nav-group="paused-full" className="flex flex-col">
        {ask.frozen.map((rid) => {
          const it = all.find((x) => x.rid === rid);
          return (
            <div key={rid} className="flex items-center gap-[0.9rem] border-b border-hair py-[0.7rem] last:border-b-0">
              <span className="min-w-0 flex-1">
                <span className="block truncate text-body font-medium">{name(rid)}</span>
                <span className="block text-small text-secondary">Paused</span>
              </span>
              <Button size="sm" variant="secondary" hint={it?.app ? "Close app" : "Quit game"} disabled={!it} onClick={() => it && quitItem(it)}>
                {it?.app ? "Close" : "Quit game"}
              </Button>
            </div>
          );
        })}
      </div>
    </Sheet>
  );
}
