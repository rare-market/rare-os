// Parts of a library game's page: the play row (Play, or Resume and Quit game while it runs),
// ES-DE's ROM systems, and a web game's controls and browser fallback.

import { useEffect, useState } from "react";
import { IconPlay } from "../components/icons";
import { Button, fmtAgo } from "../components/ui";
import { native, onProtonInstall, type ProtonStatus } from "../lib/bridge";
import { useApp } from "../lib/store";
import type { Emulation, Game } from "../lib/types";
import { profileForUrl } from "./catalog";
import { failed } from "../lib/fail";

const INPUT_LABEL = { gamepad: "Controller", keys: "Controller as keyboard", focus: "Controller picks buttons" } as const;

/** Play, or Resume and Quit game while the game runs. */
export function PlayButtons({ game }: { game: Game }) {
  const running = useApp((s) => s.running.includes(game.id));
  const launching = useApp((s) => s.launching === game.id);
  const { launch, resume, quit } = useApp.getState();
  if (running)
    return (
      <>
        <Button variant="primary" size="lg" def hint="Resume" icon={<IconPlay size="1.1rem" />} onClick={() => void resume(game)} className="min-w-[10rem]">
          Resume
        </Button>
        <Button size="lg" hint="Quit game" onClick={() => void quit(game)}>
          Quit game
        </Button>
      </>
    );
  return (
    <Button variant="primary" size="lg" def icon={<IconPlay size="1.1rem" />} onClick={() => void launch(game)} disabled={launching} className="min-w-[10rem]">
      {launching ? "Starting..." : game.source === "apps" ? "Open" : "Play"}
    </Button>
  );
}

const fmtHours = (s: number) => (s < 3600 ? `${Math.max(1, Math.round(s / 60))} min` : `${Math.round(s / 360) / 10} h`);

/** ES-DE's systems with game counts, and the ROMs played last. */
export function EmulationPanel({ emulation }: { emulation: Emulation }) {
  const total = emulation.systems.reduce((n, s) => n + s.count, 0);
  const sysName = (id: string) => emulation.systems.find((s) => s.id === id)?.name ?? id.toUpperCase();
  return (
    <div className="flex max-w-[75rem] gap-[1rem]">
      <div className="min-w-0 flex-[3] rounded-panel bg-l1 px-[1.25rem] py-[1rem]">
        <div className="mb-[0.6rem] flex items-baseline gap-[0.6rem]">
          <span className="text-small font-semibold">Systems</span>
          <span className="tabular-nums text-meta text-muted">{total} games</span>
        </div>
        <div className="grid grid-cols-3 gap-x-[1.2rem] gap-y-[0.35rem]">
          {emulation.systems.slice(0, 12).map((s) => (
            <div key={s.id} className="flex min-w-0 items-baseline justify-between gap-[0.5rem] text-small">
              <span className="truncate">{s.name}</span>
              <span className="tabular-nums shrink-0 text-muted">{s.count}</span>
            </div>
          ))}
        </div>
      </div>
      {emulation.recent.length > 0 && (
        <div className="min-w-0 flex-[2] rounded-panel bg-l1 px-[1.25rem] py-[1rem]">
          <div className="mb-[0.6rem] text-small font-semibold">Played last</div>
          <div className="flex flex-col gap-[0.35rem]">
            {emulation.recent.slice(0, 4).map((g) => (
              <div key={`${g.system}:${g.name}`} className="flex min-w-0 items-baseline gap-[0.6rem] text-small">
                <span className="min-w-0 flex-1 truncate">{g.name.replace(/\s*[[(].*$/, "") || g.name}</span>
                <span className="shrink-0 text-meta text-muted">
                  {sysName(g.system)} · {g.playtime ? fmtHours(g.playtime) : fmtAgo(g.lastPlayed)}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

/** How a web game takes the controller, and the Chrome fallback for pages WebKit can't run. */
export function WebGameInfo({ game, url }: { game: Game; url: string | null }) {
  const p = profileForUrl(game.id, url);
  const [browser, setBrowser] = useState<string | null>(null);
  useEffect(() => {
    void native.browser().then((b) => setBrowser(b?.name ?? null));
  }, []);
  const mode = p?.input ?? "keys";
  return (
    <div className="flex max-w-[60rem] items-end gap-[1.5rem]">
      <div className="min-w-0 flex-1">
        <div className="text-label font-medium text-secondary">{INPUT_LABEL[mode]}</div>
        <div className="mt-[0.25rem] text-small text-fg/80">
          {p?.controls ??
            (mode === "keys"
              ? "D-pad and left stick are the arrow keys, A Enter, B Escape, X Space. The right stick moves a pointer, RB clicks. View opens the keyboard."
              : "Guide returns to Rare with the game still running.")}
        </div>
      </div>
      {url && browser && (
        <Button
          size="sm"
          variant="ghost"
          hint={`Open in ${browser}`}
          onClick={() =>
            void native
              .webgameOpenBrowser(game.id, url)
              .then((name) => useApp.getState().toast(`Opening in ${name}`))
              .catch((e) => useApp.getState().toast(failed("Couldn't open the browser.", e), "error"))
          }
        >
          Open in {browser}
        </Button>
      )}
    </div>
  );
}

const PHASE = { lookup: "Looking up the release", download: "Downloading", verify: "Checking the checksum", unpack: "Unpacking" } as Record<string, string>;
const KIND_LABEL = { ge: "GE-Proton", cachyos: "Proton-CachyOS" } as const;

/** A Steam game's Proton: Valve's stable build by default, others on request. The choice is Steam's
 *  own per-game setting; changing it closes Steam for a moment (never while a game runs). */
export function ProtonPanel({ appid }: { appid: number }) {
  const [st, setSt] = useState<ProtonStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [prog, setProg] = useState<{ phase: string; done: number; total: number } | null>(null);
  const toast = useApp((s) => s.toast);
  const refresh = () => void native.protonStatus(appid).then(setSt).catch(() => {});
  useEffect(refresh, [appid]);
  useEffect(() => {
    let un = () => {};
    void onProtonInstall((p) => setProg(p.phase === "done" || p.phase === "failed" ? null : p)).then((u) => (un = u));
    return () => un();
  }, []);
  if (!st || !st.tools.length) return null;
  const current = st.game ?? st.global;
  const stable = st.tools.find((t) => t.kind === "valve");
  const run = async (key: string, f: () => Promise<void>, done: string) => {
    setBusy(key);
    try {
      await f();
      toast(done);
    } catch (e) {
      toast(failed("Couldn't finish that.", e), "error");
    } finally {
      setBusy(null);
      refresh();
    }
  };
  const choose = (name: string | null, label: string) => run(`set:${name}`, () => native.protonSetGame(appid, name), `Proton for this game: ${label}`);
  const install = (kind: "ge" | "cachyos") =>
    run(`install:${kind}`, async () => {
      const name = await native.protonInstall(kind);
      await native.protonSetGame(appid, name);
    }, `${KIND_LABEL[kind]} installed and selected`);
  const pick = (kinds: string[]) => st.tools.filter((t) => kinds.includes(t.kind) && !t.stale);
  const entries = [
    ...(stable ? [{ key: "stable", label: stable.label, name: null as string | null, on: current === stable.name && !st.game }] : []),
    ...pick(["experimental"]).map((t) => ({ key: t.name, label: "Experimental", name: t.name as string | null, on: st.game === t.name })),
    ...pick(["ge", "cachyos", "other"]).map((t) => ({ key: t.name, label: t.label, name: t.name as string | null, on: st.game === t.name })),
  ];
  const stale = st.tools.filter((t) => t.stale);
  return (
    <div className="flex max-w-[75rem] flex-wrap items-center gap-[0.5rem]">
      <span className="mr-[0.4rem] text-small font-semibold">Proton</span>
      {entries.map((e) => (
        <Button key={e.key} variant={e.on ? "primary" : "ghost"} disabled={busy !== null} onClick={() => !e.on && void choose(e.name, e.label)}>
          {e.label}
        </Button>
      ))}
      {st.installable.map((k) => (
        <Button key={k} variant="ghost" disabled={busy !== null} onClick={() => void install(k)}>
          Install {KIND_LABEL[k]}
        </Button>
      ))}
      {stale.map((t) => (
        <Button key={t.name} variant="ghost" disabled={busy !== null} onClick={() => void run(`rm:${t.name}`, () => native.protonRemove(t.name), `${t.label} removed`)}>
          Remove old {t.label}
        </Button>
      ))}
      {busy && <span className="text-meta text-muted">{prog ? `${PHASE[prog.phase] ?? ""}${prog.phase === "download" && prog.total ? ` ${Math.round((prog.done / prog.total) * 100)}%` : ""}` : "Restarting Steam"}</span>}
    </div>
  );
}
