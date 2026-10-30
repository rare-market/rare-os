// The Rare menu's Game tab, over a game or app, top to bottom:
// the game card, Resume, your items (a Sui game), Screenshot and Save clip with their chords, this
// game's performance, the Steam row (a Steam game), Game details, and Quit game, which asks once
// more. Resume is focused when the menu opens: Rare then A is always back to the game.

import { useEffect, useState } from "react";
import { native } from "../lib/bridge";
import { gameById, useApp } from "../lib/store";
import type { GamePowerView, Perf } from "../lib/types";
import { appByRunId } from "../apps/catalog";
import { quitItem, useRunningItems } from "../apps/sheets";
import { feedback } from "../nav/feedback";
import { saveClip, useShare } from "../share";
import { Button, Glyph, MImg, Row } from "../ui";
import { playingFor, startedAt } from "./background";
import type { OverlayItem } from "./data";
import { gameKind, itemsLine } from "./menuModel";
import type { OverlayGame } from "./game";
import { overlayNative } from "./native";
import { emuActions, emuDo, type EmuAction } from "./emuActions";
import { isTauri } from "../lib/bridge";
import { useQuitThenClose } from "./panels";
import { closeOverlay, leaveForHome, type Session } from "./session";

const ROW = "grid-cols-[minmax(0,1fr)_auto]";

/** Steam's own UI as an app (apps/mod.rs STEAM_RUN). */
const STEAM_UI = "app:steam";

const STEAM_PAGES: Array<{ page: string; title: string }> = [
  { page: "library", title: "Library" },
  { page: "store", title: "Store" },
  { page: "downloads", title: "Downloads" },
  { page: "settings", title: "Steam settings" },
];

/** Over Steam's own UI: its pages (steam://open/...), in the Steam client that is up. */
function SteamPages() {
  const toast = useApp((s) => s.toast);
  return (
    <>
      {STEAM_PAGES.map((p) => (
        <Row key={p.page} cols={ROW} hint="Open in Steam" onClick={() => void overlayNative.steamPage(p.page).catch((e) => toast(String(e), "error"))}>
          <Line title={p.title} />
        </Row>
      ))}
    </>
  );
}

const PROFILE: Record<string, string> = { quiet: "Quiet", balanced: "Balanced", performance: "Performance", custom: "Custom" };

function Chord({ b }: { b: "y" | "select" }) {
  return (
    <span className="flex items-center gap-[0.25rem] text-label text-secondary">
      <Glyph b="rare" size="sm" />+<Glyph b={b} size="sm" />
    </span>
  );
}

function Line({ title, sub }: { title: React.ReactNode; sub?: React.ReactNode }) {
  return (
    <span className="min-w-0">
      <span className="block truncate font-medium">{title}</span>
      {sub && <span className="block truncate text-label text-secondary">{sub}</span>}
    </span>
  );
}

export function GameTab({ session, game, items, onItems, onPerformance }: { session: Session; game: OverlayGame; items: OverlayItem[] | null; onItems: () => void; onPerformance: () => void }) {
  const rid = session.gameId!;
  const kind = gameKind(rid);
  const toast = useApp((s) => s.toast);
  const [perf, setPerf] = useState<Perf | null>(null);
  const [power, setPower] = useState<GamePowerView | null>(null);
  const taking = useShare((s) => s.taking);
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    void native.perf().then(setPerf).catch(() => {});
    void native.gamePower().then(setPower).catch(() => setPower(null));
    const t = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(t);
  }, []);

  const app = appByRunId(rid);
  const lib = gameById(rid) ?? null;
  const playing = playingFor(startedAt(rid), now);
  const art = game.banner;

  const screenshot = async () => {
    // A screenshot of the game, not of the menu: back to it first.
    await closeOverlay();
    await new Promise((r) => setTimeout(r, 350));
    await useShare.getState().takeScreenshot();
  };

  return (
    <div className="flex flex-col gap-[0.6rem] px-[0.1rem]">
      {/* The game card. */}
      <div className="flex items-center gap-[0.9rem] px-[0.65rem] pt-[0.3rem]">
        {art ? (
          <span className="relative h-[4.25rem] w-[9rem] shrink-0 overflow-hidden rounded-card bg-l3">
            <MImg src={art} label={game.title} width={360} className="absolute inset-0 size-full object-cover" />
          </span>
        ) : (
          game.logo && <img src={game.logo} alt="" className="size-[3.5rem] shrink-0 rounded-[22%] object-contain" />
        )}
        <span className="min-w-0">
          <span className="block truncate text-row font-semibold">{game.title}</span>
          {playing && <span className="block truncate text-small text-secondary">{playing}</span>}
        </span>
      </div>

      <div data-nav-group="menu-resume" className="px-[0.65rem]">
        <Button variant="primary" size="md" def className="w-full" hint="Resume" onClick={() => (feedback("back"), void closeOverlay())}>
          Resume
        </Button>
      </div>

      <div data-nav-group="menu-game" className="flex flex-col">
        {items && items.length > 0 && (
          <Row cols={ROW} hint="Open" onClick={onItems}>
            <Line title="Your items" sub={itemsLine(items)} />
          </Row>
        )}
        <Row cols={ROW} hint="Take a screenshot" onClick={() => void screenshot()}>
          <Line title={taking === "screenshot" ? "Saving..." : "Screenshot"} />
          <Chord b="y" />
        </Row>
        <Row cols={ROW} hint="Save the last 30 seconds" onClick={() => void saveClip()}>
          <Line title={taking === "clip" ? "Saving..." : "Save clip"} sub="The last 30 seconds" />
          <Chord b="select" />
        </Row>
        <Row cols={ROW} hint="Change" onClick={onPerformance}>
          <Line
            title="Performance"
            sub={[perf ? `${PROFILE[perf.profile] ?? "Balanced"}${perf.tdp ? ` · ${Math.round(perf.tdp)} W` : ""}` : null, power?.id === rid && power.saved ? `Used for ${game.title}` : null].filter(Boolean).join(" · ") || undefined}
          />
        </Row>
        {(kind === "steam" || rid.startsWith("shortcut:")) && (
          <Row cols={ROW} hint="Open Steam's overlay" onClick={() => void native.steamMenu().catch((e) => toast(String(e), "error"))}>
            <Line title="Steam" sub="Steam's overlay: friends, guides, controller layout" />
          </Row>
        )}
        {rid === STEAM_UI && <SteamPages />}
        <EmuRows rid={rid} />
        {(lib || app) && (
          <Row
            cols={ROW}
            hint="Open"
            onClick={() =>
              void leaveForHome().then(() => {
                if (app) useApp.getState().open({ kind: "app", id: app.id });
                else if (lib) useApp.getState().open({ kind: "game", id: lib.id });
              })
            }
          >
            <Line title={app ? "App details" : "Game details"} />
          </Row>
        )}
      </div>

      <QuitRow rid={rid} title={game.title} kind={kind} />
    </div>
  );
}

/** An emulator's rows (emu/actions.rs): Save state, Load state, Emulator menu. */
function EmuRows({ rid }: { rid: string }) {
  const toast = useApp((s) => s.toast);
  const [rows, setRows] = useState<EmuAction[]>([]);
  useEffect(() => {
    if (!isTauri) return;
    let live = true;
    void emuActions(rid).then((r) => live && setRows(r), () => {});
    return () => {
      live = false;
    };
  }, [rid]);
  return (
    <>
      {rows.map((a) => (
        <Row
          key={a.id}
          cols={ROW}
          hint={a.ready ? a.label : undefined}
          onClick={() => {
            if (!a.ready) return;
            feedback("select");
            void emuDo(a).then(
              () => a.id !== "menu" && toast(a.id === "save" ? "State saved" : "State loaded"),
              (e) => toast(String(e), "error"),
            );
          }}
        >
          <Line title={a.label} sub={a.ready ? undefined : a.note ?? undefined} />
        </Row>
      ))}
    </>
  );
}

/** Quit game, which asks once more (A again within 4 s); then Rare has the screen. */
function QuitRow({ rid, title, kind }: { rid: string; title: string; kind: ReturnType<typeof gameKind> }) {
  const items = useRunningItems();
  const it = items.find((x) => x.rid === rid) ?? null;
  const { quitting, setQuitting } = useQuitThenClose();
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(false), 4000);
    return () => clearTimeout(t);
  }, [armed]);
  if (!it) return null;
  const label = rid === STEAM_UI ? "Close Steam" : kind === "app" ? "Close app" : "Quit game";
  const what = rid === STEAM_UI ? "Steam closes, with any game it runs, and Rare has the screen." : kind === "stream" ? `Ends ${title} on your PC and goes back to Rare.` : `Ends ${title} and goes back to Rare.`;
  return (
    <div data-nav-group="menu-quit" className="mt-[0.2rem] border-t border-hair pt-[0.35rem]">
      <Row
        cols={ROW}
        hint={armed ? "Quit now" : label}
        onClick={() => {
          if (quitting) return;
          if (!armed) return setArmed(true);
          setArmed(false);
          quitItem(it);
          setQuitting(it.rid);
        }}
      >
        <Line title={quitting ? "Quitting..." : armed ? "Quit now" : label} sub={armed ? "Unsaved progress is lost. Press A again to quit." : what} />
      </Row>
    </div>
  );
}
