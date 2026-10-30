// App tiles: the console media row. Same shape and focus as the launcher's other square tiles
// (collectibles), the app's own icon on the layer colour as the emulator tiles show theirs
// (components/ui AppArt), name and how it opens underneath like a game cover.

import { useEffect } from "react";
import { useApp } from "../lib/store";
import { feedback } from "../nav/feedback";
import { usePadHandler } from "../nav/input";
import { Dot, Rail } from "../ui";
import { APPS, runId, type AppEntry } from "./catalog";
import { useApps } from "./state";

/** The icon as the service ships it: an apple-touch icon, masked to the rounded square it was drawn for. */
export function AppIcon({ app, className = "h-[4.5rem] w-[4.5rem]" }: { app: AppEntry; className?: string }) {
  return <img src={app.icon} alt="" draggable={false} className={`shrink-0 rounded-[22%] object-contain ${className}`} />;
}

/** A thin determinate bar on the slider track of Quick settings (layer 04, brand fill). */
export function InstallBar({ progress, className = "" }: { progress: number | null; className?: string }) {
  return (
    <div className={`relative h-[0.35rem] overflow-hidden rounded-full bg-l4 ${className}`}>
      {progress == null ? (
        <div className="load-bar absolute inset-0" />
      ) : (
        <div className="absolute inset-y-0 left-0 rounded-full bg-fg transition-[width] duration-200" style={{ width: `${Math.round(progress * 100)}%` }} />
      )}
    </div>
  );
}

export function AppTile({ app, def, details }: { app: AppEntry; def?: boolean; details?: boolean }) {
  const running = useApp((s) => s.running.includes(runId(app.id)));
  const status = useApps((s) => s.status[app.id]);
  const installing = useApps((s) => s.installing[app.id]);
  const ready = status?.way != null;
  // Only a state worth reading: how an app opens (which browser) is detail for its page, not the tile.
  const sub = installing && !installing.error ? "Installing" : running ? "Running" : !ready && status ? "Not installed" : "";
  return (
    <div className="w-[9rem] shrink-0">
      <button
        data-nav
        data-nav-default={def ? "" : undefined}
        data-hint-a={running ? "Resume" : ready ? "Open" : "Get"}
        data-hint-x={details ? "Details" : undefined}
        data-app={app.id}
        onClick={() => useApps.getState().activate(app)}
        className="tile relative flex aspect-square w-full items-center justify-center overflow-hidden rounded-card bg-l2"
      >
        <AppIcon app={app} />
        {running && <Dot tone="primary" live className="absolute right-[0.6rem] top-[0.6rem]" />}
        {installing && !installing.error && <InstallBar progress={installing.progress} className="absolute inset-x-[0.75rem] bottom-[0.75rem]" />}
      </button>
      <div className="focus-text mt-[0.85rem] truncate px-[0.1rem] text-meta font-medium text-fg/85">{app.name}</div>
      <div className="truncate px-[0.1rem] text-label text-muted">{sub || "\u00a0"}</div>
    </div>
  );
}

function useStatus() {
  const refresh = useApps((s) => s.refresh);
  useEffect(() => {
    void refresh();
    // Back from Desktop mode or an install elsewhere: pick up what changed.
    const f = () => void refresh();
    window.addEventListener("focus", f);
    return () => window.removeEventListener("focus", f);
  }, [refresh]);
}

/** Home's Apps row. */
export function AppsRail() {
  useStatus();
  // X on a focused app: its page (Resume and Close, other ways to open it, the controls).
  usePadHandler((b) => {
    const id = b === "x" ? (document.activeElement as HTMLElement | null)?.dataset?.app : undefined;
    if (!id) return false;
    feedback("open");
    useApp.getState().open({ kind: "app", id });
    return true;
  });
  const n = useApp((s) => s.running.filter((r) => r.startsWith("app:")).length);
  return (
    <Rail id="media-apps" title="Apps" note={n ? `${n} running` : undefined}>
      {APPS.map((a) => (
        <AppTile key={a.id} app={a} details />
      ))}
    </Rail>
  );
}

/** Library > Apps. */
export function AppsGrid() {
  useStatus();
  return (
    <div data-nav-group="library-media" className="grid grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-x-[1.1rem] gap-y-[1.4rem]">
      {APPS.map((a, i) => (
        <AppTile key={a.id} app={a} def={i === 0} />
      ))}
    </div>
  );
}
