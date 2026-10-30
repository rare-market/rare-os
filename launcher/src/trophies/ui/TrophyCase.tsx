// The trophy case: gamerscore, every Steam game with achievements on this device, and a page per
// game with each trophy minted, unlocked (Claim) or locked. Steam claims are Self-reported; the
// header comment of attest.ts says why.

import { useEffect, useRef, useState } from "react";
import { type Overlay, useApp } from "../../lib/store";
import { focusEl, focusFirst } from "../../nav/focus";
import { feedback } from "../../nav/feedback";
import { useHints } from "../../nav/hints";
import { usePadHandler } from "../../nav/input";
import { Button, Dot, Empty, Glyph, HeadRow, LoadBar, SectionHead, Segmented, StatLine } from "../../ui";
import { ProfileAvatar } from "../../sui/ui/bits";
import { displayName, useProfile } from "../../sui/profile";
import { claimableKeys, sortRows, type RowFilter, type TrophyRow } from "../model";
import { useTrophies } from "../store";
import { SOURCE_NAME, TIER_NAME, type Tier } from "../types";
import { GameArt, GameCard, TierLabel, TrophyArt, fmtDay, fmtPct } from "./bits";
import { cardNote, useCaseData, type CaseGame, type GameSort } from "./useCase";

const SORTS: Array<{ value: GameSort; label: string }> = [
  { value: "recent", label: "Recent" },
  { value: "completion", label: "Completion" },
  { value: "name", label: "Name" },
];
const FILTERS: Array<{ value: RowFilter; label: string }> = [
  { value: "all", label: "All" },
  { value: "unlocked", label: "Unlocked" },
  { value: "locked", label: "Locked" },
];

export function TrophyCase({ appid, from }: { appid?: number; from?: Overlay }) {
  const [view, setView] = useState<number | null>(appid ?? null);
  const [sort, setSort] = useState<GameSort>("recent");
  const [filter, setFilter] = useState<RowFilter>("all");
  const last = useRef<number | null>(appid ?? null);
  const root = useRef<HTMLDivElement>(null);
  const data = useCaseData(sort);
  const game = view != null ? data.games.find((g) => g.appid === view) ?? null : null;

  // Focus the screen's default on open and on every page change (the card you came from on the way back).
  useEffect(() => {
    const id = setTimeout(() => root.current && focusFirst(root.current), 40);
    return () => clearTimeout(id);
    // Once more when the game's chain data lands: the default moves to Claim.
  }, [view, data.games.length > 0, !!game?.chain]); // eslint-disable-line react-hooks/exhaustive-deps

  const next = <T,>(list: Array<{ value: T; label: string }>, cur: T) => list[(list.findIndex((x) => x.value === cur) + 1) % list.length];
  useHints([{ b: "x", label: view != null ? `Show ${next(FILTERS, filter).label.toLowerCase()}` : `Sort by ${next(SORTS, sort).label.toLowerCase()}` }]);
  usePadHandler((b) => {
    if (b === "b") {
      feedback("back");
      if (view != null && appid == null) {
        setView(null);
        setFilter("all");
      } else if (from) useApp.getState().open(from);
      else useApp.getState().close();
      return true;
    }
    if (b === "x") {
      feedback("move");
      if (view != null) setFilter(next(FILTERS, filter).value);
      else setSort(next(SORTS, sort).value);
      return true;
    }
    return false;
  });

  const open = (id: number) => {
    last.current = id;
    setView(id);
  };

  return (
    <div ref={root} data-nav-scope className="rise fixed inset-0 z-30 bg-bg">
      <LoadBar on={data.loading} />
      {view != null && game ? (
        <GamePage key={game.appid} g={game} filter={filter} />
      ) : view != null && !data.loading ? (
        <div className="flex h-full items-center px-[3rem]">
          <Empty title="No achievements for this game here" body="Steam keeps achievements on this device once the game has run here at least once." />
        </div>
      ) : view == null ? (
        <CasePage data={data} sort={sort} onOpen={open} lastAppid={last.current} />
      ) : null}
    </div>
  );
}

function CasePage({ data, sort, onOpen, lastAppid }: { data: ReturnType<typeof useCaseData>; sort: GameSort; onOpen: (appid: number) => void; lastAppid: number | null }) {
  const prof = useProfile();
  const s = data.summary;
  const tierItem = (t: Tier) => ({
    key: TIER_NAME[t],
    label: (
      <span className="inline-flex items-center gap-[0.35rem]">
        <Dot tone={t === 3 ? "primary" : "muted"} />
        {TIER_NAME[t]}
      </span>
    ),
    value: s.points != null ? s.tiers[t].toLocaleString("en-US") : null,
  });
  const defIdx = Math.max(0, data.games.findIndex((g) => g.appid === lastAppid));
  return (
    <div data-scroll className="h-full overflow-y-auto pb-[3rem]">
      <header className="flex items-end gap-[1.25rem] px-[2.5rem] pt-[2.25rem]">
        {prof.address && <ProfileAvatar size="4rem" />}
        <div className="min-w-0 flex-1">
          <div className="text-label text-secondary">Trophies</div>
          <h1 className="truncate text-title font-semibold tracking-[-0.02em]">{prof.address ? displayName(prof) : "Your trophies"}</h1>
        </div>
      </header>

      <StatLine
        size="lg"
        className="mx-[2.5rem] mt-[1.5rem] rounded-panel bg-l1 px-[1.25rem] py-[1rem]"
        items={[
          { key: "points", label: "Points", value: s.points != null ? s.points.toLocaleString("en-US") : null },
          tierItem(3),
          tierItem(2),
          tierItem(1),
          tierItem(0),
          { key: "self", label: "Self-reported", value: s.selfReportedPct != null ? `${s.selfReportedPct}%` : null },
          { key: "steam", label: "Steam achievements", value: data.games.length ? `${s.steamUnlocked.toLocaleString("en-US")} of ${s.steamTotal.toLocaleString("en-US")}` : null },
          { key: "games", label: "Games with unlocks", value: s.points == null && data.games.length ? String(s.games) : null },
          {
            key: "pct",
            label: "Completion",
            value: s.points == null && s.steamTotal ? `${Math.round((s.steamUnlocked / s.steamTotal) * 100)}%` : null,
          },
        ]}
      />
      <p className="mx-[2.5rem] mt-[0.75rem] max-w-[56rem] text-small text-muted">
        {data.chainOn
          ? "Steam achievements on this device can be claimed as trophies in your wallet. They are marked Self-reported, because Steam's files on this device are the only proof."
          : "Your Steam achievements on this device. Claiming them as trophies opens soon."}
      </p>

      <SectionHead
        className="mb-[0.9rem] mt-[2rem] px-[2.5rem]"
        title="Games"
        note={data.games.length ? `${data.games.length} with achievements` : undefined}
        right={
          data.games.length > 1 ? (
            <>
              <Glyph b="x" size="sm" />
              <Segmented items={SORTS} value={sort} />
            </>
          ) : undefined
        }
      />
      {data.games.length > 0 ? (
        <div data-nav-group="trophy-games" className="grid grid-cols-4 gap-[1.1rem] px-[2.5rem]">
          {data.games.map((g, i) => (
            <GameCard key={g.appid} appid={g.appid} name={g.name} sum={g.sum} note={cardNote(g)} def={i === defIdx} onClick={() => onOpen(g.appid)} />
          ))}
        </div>
      ) : !data.loading ? (
        <Empty
          className="mx-[2.5rem]"
          title="No Steam achievements on this device yet"
          body="Play a Steam game with achievements and they show up here. Rare reads them from Steam's own files, so nothing to sign in to."
        />
      ) : null}
    </div>
  );
}

const COLS = "grid-cols-[2.75rem_minmax(0,1fr)_8rem_6rem_12.5rem]";

function GamePage({ g, filter }: { g: CaseGame; filter: RowFilter }) {
  const claiming = useTrophies((s) => s.claiming[g.appid] ?? null);
  const rows = sortRows(g.rows, filter);
  const keys = claimableKeys(g.rows);
  const canClaim = !g.blocker && keys.length > 0;
  const claim = (k: string[]) => void useTrophies.getState().claim(g.appid, k);
  const firstFocus = useRef<HTMLDivElement>(null);

  // A finished claim moves focus off a row that changed: keep it on the list.
  useEffect(() => {
    if (claiming) return;
    const cur = document.activeElement as HTMLElement | null;
    if (cur && !cur.isConnected) focusEl(firstFocus.current?.querySelector<HTMLElement>("[data-nav]"));
  }, [claiming]);

  return (
    <div data-scroll className="h-full overflow-y-auto pb-[3rem]">
      <header className="flex items-end gap-[1.5rem] px-[2.5rem] pt-[2.25rem]">
        <div className="w-[17rem] shrink-0 overflow-hidden rounded-card">
          <GameArt appid={g.appid} title={g.name} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-label text-secondary">Steam{g.steam.source === "librarycache" ? " · from your Steam library" : ""}</div>
          <h1 className="truncate text-title font-semibold tracking-[-0.02em]">{g.name}</h1>
          <StatLine
            align="start"
            className="mt-[0.9rem]"
            items={[
              { key: "u", label: "Unlocked", value: `${g.sum.unlocked} of ${g.sum.total}` },
              { key: "c", label: "Completion", value: `${g.sum.pct}%` },
              { key: "m", label: "Minted", value: g.chain ? String(g.sum.minted) : null },
              { key: "p", label: "Points", value: g.chain && g.sum.points ? g.sum.points.toLocaleString("en-US") : null },
              { key: "r", label: "Players on Rare", value: g.chain && g.chain.players ? g.chain.players.toLocaleString("en-US") : null },
            ]}
          />
        </div>
        <div data-nav-group="trophy-actions" data-nav-remember className="flex shrink-0 flex-col items-end gap-[0.5rem]">
          {canClaim || claiming ? (
            <Button variant="primary" size="lg" def hint="Claim" disabled={!!claiming} onClick={() => claim(keys)}>
              {claiming ? "Claiming" : keys.length === 1 ? "Claim 1 trophy" : `Claim ${keys.length} trophies`}
            </Button>
          ) : g.sum.unminted > 0 && g.blocker ? (
            <Button size="lg" disabled>
              {g.blocker}
            </Button>
          ) : null}
        </div>
      </header>

      <SectionHead
        className="mb-[0.5rem] mt-[2rem] px-[2.5rem]"
        title="Trophies"
        note={`${rows.length} shown`}
        right={
          <>
            <Glyph b="x" size="sm" />
            <Segmented items={FILTERS} value={filter} />
          </>
        }
      />
      <div ref={firstFocus} className="px-[2.5rem]">
        <HeadRow cols={COLS}>
          <span />
          <span>Trophy</span>
          <span>Tier</span>
          <span className="text-right">Rarity</span>
          <span className="text-right">Status</span>
        </HeadRow>
        <div data-nav-group={`trophy-list-${filter}`}>
          {rows.map((r, i) => (
            <TrophyRowView key={r.key} r={r} def={!canClaim && i === 0} claimable={!g.blocker && r.state === "unlocked" && !!r.def && r.def.tier !== 3} busy={!!claiming?.includes(r.key)} onClaim={() => claim([r.key])} />
          ))}
          {!rows.length && <div className="px-[0.75rem] py-[1rem] text-small text-muted">{filter === "locked" ? "Nothing left to unlock." : "Nothing unlocked yet."}</div>}
        </div>
      </div>
    </div>
  );
}

function TrophyRowView({ r, def, claimable, busy, onClaim }: { r: TrophyRow; def: boolean; claimable: boolean; busy: boolean; onClaim: () => void }) {
  const locked = r.state === "locked";
  return (
    <button
      data-nav
      data-nav-default={def ? "" : undefined}
      data-hint-a={claimable && !busy ? "Claim" : ""}
      onClick={() => claimable && !busy && onClaim()}
      className={`grid h-[4.25rem] w-full items-center gap-x-[0.9rem] rounded-card px-[0.75rem] text-left ${COLS}`}
    >
      <TrophyArt src={r.iconUrl} gray={locked ? r.iconGrayUrl : undefined} masked={r.masked} label={r.name} />
      <span className="min-w-0">
        <span className={`block truncate text-body font-medium ${locked ? "text-fg/70" : "text-fg"}`}>{r.masked ? "Hidden trophy" : r.name}</span>
        <span className="block truncate text-small text-muted">{r.masked ? "Keep playing to reveal it." : r.description}</span>
      </span>
      <span>{r.tier != null && <TierLabel tier={r.tier} />}</span>
      <span className="text-right">
        {r.rarity != null && (
          <>
            <span className="block tabular-nums text-small text-fg/85">{fmtPct(r.rarity)}</span>
            <span className="block text-label text-muted">{r.rarityFrom === "rare" ? "on Rare" : "on Steam"}</span>
          </>
        )}
      </span>
      <span className="text-right">
        {r.state === "minted" && r.minted ? (
          <>
            <span className="block text-small font-medium text-fg">Minted</span>
            <span className="block truncate text-label text-muted">
              {SOURCE_NAME[r.minted.source]} · #{r.minted.number.toLocaleString("en-US")}
            </span>
          </>
        ) : r.state === "unlocked" ? (
          <>
            <span className={`block text-small font-medium ${busy ? "text-muted" : "text-fg"}`}>{busy ? "Claiming" : claimable ? "Ready to claim" : "Unlocked"}</span>
            {r.at && <span className="block text-label text-muted">{fmtDay(r.at)}</span>}
          </>
        ) : (
          <span className="text-small text-muted">Locked</span>
        )}
      </span>
    </button>
  );
}
