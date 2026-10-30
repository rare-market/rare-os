// An app's page (Open or Resume and Close, other ways to open it, Get with progress, the
// controller layout) and the running list View + Menu (or Guide) held brings up. Both are the launcher's
// standard sheet (trade/parts Sheet: rare.market's modal).

import { useEffect, useRef, useState } from "react";
import { native } from "../lib/bridge";
import { gameById, useApp } from "../lib/store";
import type { Game } from "../lib/types";
import { focusFirst } from "../nav/focus";
import { exitHint, useHints } from "../nav/hints";
import { isGuest, rareWay } from "../lib/guest";
import { Sheet } from "../trade/parts";
import { Button, Dot, Glyph } from "../ui";
import { Art } from "../components/Art";
import { appById, appByRunId, controlsFor, runId, type AppEntry, type ControlRow } from "./catalog";
import { useApps } from "./state";
import { streamOf, streamTitle, useStream } from "../stream/state";
import { AppIcon, InstallBar } from "./tiles";
import { failed } from "../lib/fail";

function useAutoFocus() {
  const ref = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const t = setTimeout(() => ref.current && focusFirst(ref.current), 20);
    return () => clearTimeout(t);
  }, []);
  return (el: HTMLDivElement | null) => {
    ref.current = el;
  };
}

function Controls({ rows }: { rows: ControlRow[] }) {
  return (
    <div className="grid grid-cols-2 gap-x-[2rem]">
      {rows.map((r) => (
        <div key={r.b + r.does} className="flex h-[2.5rem] items-center gap-[0.75rem] border-b border-hair text-small">
          <span className="flex w-[6.25rem] shrink-0 items-center text-secondary">{r.b.includes(" ") || r.b.includes("-") ? r.b : <Glyph b={r.b as never} size="sm" />}</span>
          <span className="truncate">{r.does}</span>
        </div>
      ))}
    </div>
  );
}

export function AppSheet({ id }: { id: string }) {
  const app = appById(id);
  const onRef = useAutoFocus();
  const status = useApps((s) => s.status[id]);
  const installing = useApps((s) => s.installing[id]);
  const running = useApp((s) => s.running.includes(runId(id)));
  useHints([exitHint()]);
  if (!app) return null;
  const { open, resume, close, install } = useApps.getState();
  const done = () => useApp.getState().close();
  const ready = status?.way != null;
  const offer = status?.offer ?? null;
  const busy = !!installing && !installing.error;
  const way = ready ? app.ways[status!.way!] : app.ways[0];
  const controller = status?.controller ?? way.controller ?? "pointer";
  const line = running ? "Running" : ready ? `Opens in ${status!.opensIn}` : offer ? `Needs ${offer.name}` : "Can't open on this device";

  return (
    <Sheet
      onRef={onRef}
      title={
        <span className="flex items-center gap-[0.9rem]">
          <AppIcon app={app} className="h-[2.75rem] w-[2.75rem]" />
          {app.name}
        </span>
      }
      sub={app.about}
      footer={
        <>
          {running ? (
            <>
              <Button variant="primary" def hint="Resume" onClick={() => (done(), void resume(app))}>
                Resume
              </Button>
              <Button hint="Close app" onClick={() => (done(), void close(app))}>
                Close
              </Button>
            </>
          ) : ready ? (
            <>
              <Button variant="primary" def hint="Open" onClick={() => (done(), void open(app))}>
                Open
              </Button>
              {status!.otherWays.map((i) => (
                <Button key={i} onClick={() => (done(), void open(app, i))}>
                  Open in {app.ways[i].label}
                </Button>
              ))}
            </>
          ) : null}
          {offer && (
            <Button variant={ready || running ? "secondary" : "primary"} def={!ready && !running} disabled={busy} hint={`Get ${offer.name}`} onClick={() => void install(app)}>
              {busy ? "Installing" : `Get ${offer.name}`}
            </Button>
          )}
        </>
      }
    >
      <div className="flex items-center gap-[0.5rem] text-small text-fg/80">
        <Dot tone={running ? "brand" : "muted"} live={running} />
        {line}
      </div>

      {offer && (
        <div className="mt-[1.1rem] flex flex-col gap-[0.6rem]">
          <p className="text-small text-secondary">
            {ready
              ? `${offer.name} is the full desktop app. It installs from Flathub for you only, no password needed.`
              : `${app.name} opens in ${offer.name}. It installs from Flathub for you only, no password needed.`}
          </p>
          {busy && (
            <div className="flex items-center gap-[0.9rem]">
              <InstallBar progress={installing!.progress} className="flex-1" />
              <span className="w-[3rem] text-right text-small tabular-nums text-fg">{installing!.progress == null ? "" : `${Math.round(installing!.progress * 100)}%`}</span>
            </div>
          )}
          {installing?.error && <p className="text-small text-negative">{installing.error}</p>}
          <div className="text-label text-secondary">Or in Desktop mode, in a terminal:</div>
          <code className="num block select-all break-all rounded-card bg-l2 px-[0.9rem] py-[0.6rem] text-meta text-fg">{offer.command}</code>
        </div>
      )}

      <div className="mt-[1.4rem]">
        <div className="mb-[0.35rem] text-body font-semibold">Controls</div>
        <Controls rows={controlsFor(controller, way)} />
        <p className="mt-[0.8rem] text-small text-secondary">{rareWay()} to come back to Rare, then Resume or Close it.</p>
      </div>
    </Sheet>
  );
}

export interface RunningItem {
  rid: string;
  name: string;
  app: AppEntry | null;
  game: Game | null;
  stream: ReturnType<typeof streamOf>;
}

/** Everything running, newest last. `title` names what the library doesn't (a Sui Games web game). */
export function useRunningItems(title?: (rid: string) => string | null): RunningItem[] {
  const running = useApp((s) => s.running);
  useStream((s) => s.apps);
  return running.map((rid) => {
    const app = appByRunId(rid);
    if (app) return { rid, name: app.name, app, game: null, stream: null };
    const stream = streamOf(rid);
    if (stream || rid.startsWith("stream:")) return { rid, name: streamTitle(rid) ?? "PC game", app: null, game: null, stream };
    const game = gameById(rid) ?? null;
    return { rid, name: game?.title ?? title?.(rid) ?? "Game", app: null, game, stream: null };
  });
}

/** Puts a running game or app back on screen. */
export function resumeItem(it: RunningItem) {
  if (it.app) void useApps.getState().resume(it.app);
  else if (it.stream) void useStream.getState().resume(it.stream.host, it.stream.app);
  else if (it.game) void useApp.getState().resume(it.game);
  else void native.resume(it.rid).catch((e) => useApp.getState().toast(failed("Couldn't switch back.", e), "error"));
}

/** Ends a running game or app. */
export function quitItem(it: RunningItem) {
  if (it.app) void useApps.getState().close(it.app);
  else if (it.stream) void useStream.getState().quit(it.stream.host);
  else if (it.game) void useApp.getState().quit(it.game);
  else void native.quit(it.rid).catch((e) => useApp.getState().toast(failed("Couldn't close it.", e), "error"));
}

/**
 * Resume and Quit for each running game or app. Quit asks once more (A again within a few
 * seconds), since unsaved progress goes with it.
 */
export function RunningList({ items, onResume, onQuit, quitting }: { items: RunningItem[]; onResume: (it: RunningItem) => void; onQuit: (it: RunningItem) => void; quitting?: string | null }) {
  const [armed, setArmed] = useState<string | null>(null);
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(null), 4000);
    return () => clearTimeout(t);
  }, [armed]);
  return (
    <div className="flex flex-col">
      {items.map((it, i) => (
        <div key={it.rid} className="flex items-center gap-[0.9rem] border-b border-hair py-[0.7rem] last:border-b-0">
          {it.app ? (
            <AppIcon app={it.app} className="h-[2.5rem] w-[2.5rem]" />
          ) : it.stream ? (
            <span className="relative h-[2.5rem] w-[2.5rem] shrink-0 overflow-hidden rounded-chip bg-l3">
              <Art sources={it.stream.app.art ? [it.stream.app.art] : []} title={it.name} variant="cover" className="absolute inset-0 h-full w-full object-cover" />
            </span>
          ) : (
            <span className="relative h-[2.5rem] w-[2.5rem] shrink-0 overflow-hidden rounded-chip bg-l3">
              <Art sources={it.game?.art.icon?.length ? it.game.art.icon : it.game?.art.cover ?? []} title={it.name} variant="cover" className="absolute inset-0 h-full w-full object-cover" />
            </span>
          )}
          <span className="min-w-0 flex-1">
            <span className="block truncate text-body font-medium">{it.name}</span>
            {quitting === it.rid ? (
              <span className="block text-small text-secondary">Quitting...</span>
            ) : armed === it.rid ? (
              <span className="block text-small text-secondary">Unsaved progress is lost. Press A again to quit.</span>
            ) : null}
          </span>
          <Button size="sm" variant="primary" def={i === 0} hint="Resume" disabled={quitting === it.rid} onClick={() => onResume(it)}>
            Resume
          </Button>
          <Button
            size="sm"
            variant={armed === it.rid ? "primary" : "secondary"}
            hint={armed === it.rid ? "Quit now" : it.app ? "Close app" : "Quit game"}
            disabled={quitting === it.rid}
            onClick={() => {
              if (armed !== it.rid) {
                setArmed(it.rid);
                return;
              }
              setArmed(null);
              onQuit(it);
            }}
          >
            {armed === it.rid ? "Quit now" : it.app ? "Close" : "Quit game"}
          </Button>
        </div>
      ))}
    </div>
  );
}

/** Holding Guide, or View + Menu: everything running, each with Resume and Quit. */
export function RunningSheet() {
  const onRef = useAutoFocus();
  const items = useRunningItems();
  const closeOverlay = useApp((s) => s.close);
  return (
    <Sheet onRef={onRef} title="Running" sub={items.length ? "Resume one, or quit it." : undefined}>
      {items.length === 0 && <p className="text-small text-secondary">Nothing is running.</p>}
      <RunningList
        items={items}
        onResume={(it) => {
          closeOverlay();
          resumeItem(it);
        }}
        onQuit={quitItem}
      />
      <p className="mt-[0.9rem] flex items-center gap-[0.45rem] text-small text-secondary">
        {isGuest() ? <>Hold <Glyph b="select" size="sm" /> in any game to come back here.</> : <>Press <Glyph b="rare" size="sm" /> in any game to come back here.</>}
      </p>
    </Sheet>
  );
}
