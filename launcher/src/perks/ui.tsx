// Rareling perks on screen: the profile badge, the trader list label, and the profile's perks panel
// (themes and early access for holders; "Hold a Rareling to unlock" for everyone else). The kit's
// pieces only: no pills, a Dot for status, the theme swatch is the accent itself.

import { useEffect, useState } from "react";
import { Check, ExternalLink } from "lucide-react";
import { loadGames, type SuiGame } from "../lib/rare";
import { native } from "../lib/bridge";
import { isActive } from "../games/catalog";
import { SuiGameCard } from "../screens/SuiGames";
import { rareSui, type Launch } from "../sui/data";
import { Button, Dot, MImg, Panel, SectionHead } from "../ui";
import { RARELINGS_URL, rarelingsOn } from "./config";
import { holdsRareling, usePerks } from "./store";
import { THEMES } from "./themes";

/** Profile header badge: shows only for a holder. */
export function RarelingBadge() {
  if (!rarelingsOn()) return null;
  const owned = usePerks((s) => holdsRareling(s.result));
  const count = usePerks((s) => (s.result?.status === "owned" ? s.result.count : 0));
  if (!owned) return null;
  return (
    <span className="inline-flex items-center gap-[0.45rem] text-meta font-semibold text-fg" title="Holds a Rareling">
      <RarelingMark />
      Rareling
      {count > 1 && <span className="font-normal text-muted">{count}</span>}
    </span>
  );
}

/** The trader list's word for a holder: muted text, never a pill. */
export function RarelingLabel() {
  if (!rarelingsOn()) return null;
  return (
    <span className="inline-flex items-center gap-[0.3rem] text-label text-secondary">
      <RarelingMark size="0.75rem" />
      Rareling
    </span>
  );
}

/** A small mark in the brand colour (it follows the theme). */
function RarelingMark({ size = "0.9rem" }: { size?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" aria-hidden className="shrink-0">
      <path d="M8 1.2 13.8 5 11.6 14H4.4L2.2 5z" fill="rgb(var(--brand))" />
      <path d="M2.2 5h11.6M8 1.2 5.6 5 8 14l2.4-9z" fill="none" stroke="rgb(255 255 255 / 0.55)" strokeWidth="0.9" strokeLinejoin="round" />
    </svg>
  );
}

const startsMs = (l: Launch) => {
  const t = l.startsAt ?? l.phases?.[0]?.startsAt ?? null;
  const n = typeof t === "string" ? Date.parse(t) : typeof t === "number" ? t : NaN;
  return Number.isFinite(n) ? n : null;
};

/** Launches that have not opened yet, soonest first. */
export function upcomingLaunches(list: Launch[], now = Date.now()): Launch[] {
  return list
    .map((l) => ({ l, at: startsMs(l) }))
    .filter((x): x is { l: Launch; at: number } => x.at != null && x.at > now)
    .sort((a, b) => a.at - b.at)
    .map((x) => x.l);
}

const when = (l: Launch) => {
  const at = startsMs(l);
  return at ? new Date(at).toLocaleDateString("en-US", { month: "short", day: "numeric" }) : "";
};

/** Profile panel: the perks, unlocked or not. */
export function PerksSection({ address }: { address: string | null }) {
  if (!rarelingsOn()) return null;
  return <PerksPanel address={address} />;
}

function PerksPanel({ address }: { address: string | null }) {
  const result = usePerks((s) => s.result);
  const theme = usePerks((s) => s.theme);
  const owned = holdsRareling(result);
  const [games, setGames] = useState<SuiGame[]>([]);
  const [drops, setDrops] = useState<Launch[]>([]);
  useEffect(() => {
    void usePerks.getState().check(address);
  }, [address]);
  useEffect(() => {
    if (!owned) return;
    // Upcoming only: paused or offline games are not "early access".
    void loadGames().then((r) => setGames(r.games.filter((g) => !isActive(g) && (g.status === "announced" || g.status === "coming"))));
    void rareSui
      .launches()
      .then((l) => setDrops(upcomingLaunches(l)))
      .catch(() => setDrops([]));
  }, [owned]);

  if (!owned) {
    return (
      <Panel className="mx-[2.5rem] flex items-center gap-[1.5rem] px-[1.5rem] py-[1.25rem]">
        <RarelingMark size="2.25rem" />
        <div className="min-w-0 flex-1">
          <div className="text-row font-semibold">Hold a Rareling to unlock</div>
          <div className="text-small text-muted">A Rareling badge on your profile, launcher themes, early access to new Sui games and drops, and a Rareling label in Top traders.</div>
        </div>
        <div data-nav-group="perks-locked">
          <Button icon={<ExternalLink size="1.05rem" />} onClick={() => void native.openWeb(RARELINGS_URL)}>
            See Rarelings
          </Button>
        </div>
      </Panel>
    );
  }

  const early = [...drops, ...games];

  return (
    <section className="flex flex-col gap-[1.25rem]">
      <SectionHead title="Rareling perks" note={<span className="inline-flex items-center gap-[0.4rem]"><Dot tone="primary" />Unlocked</span>} className="px-[2.5rem]" />
      <Panel className="mx-[2.5rem] px-[1.5rem] py-[1.25rem]">
        <div className="text-body font-semibold">Launcher theme</div>
        <div className="text-small text-muted">Accent colours from rare.market. Only Rareling holders see them.</div>
        <div data-nav-group="perks-themes" data-nav-remember className="mt-[0.9rem] flex flex-wrap gap-[0.75rem]">
          {THEMES.map((t) => {
            const on = t.id === theme;
            return (
              <button
                key={t.id}
                data-nav
                data-hint-a={on ? "In use" : "Use this theme"}
                aria-pressed={on}
                onClick={() => usePerks.getState().setTheme(t.id)}
                className={`flex h-[2.85rem] items-center gap-[0.6rem] rounded-chip px-[1rem] text-body font-medium ${on ? "bg-button-secondary-active text-fg" : "bg-button-secondary text-secondary"}`}
              >
                <span className="size-[1rem] shrink-0 rounded-full" style={{ background: `rgb(${t.swatch})` }} aria-hidden />
                {t.label}
                {on && <Check size="1rem" />}
              </button>
            );
          })}
        </div>
      </Panel>
      <div>
        <SectionHead title="Early access" note="New Sui games and drops before they open" className="mb-[0.9rem] px-[2.5rem]" />
        {early.length ? (
          <div data-rail data-nav-group="perks-early" className="flex gap-[1rem] overflow-x-auto px-[2.5rem]">
            {drops.slice(0, 6).map((d) => (
              <button key={`d:${d.slug}`} data-nav data-hint-a="Open" onClick={() => void native.openWeb(`https://rare.market/launchpad/${d.slug}`)} className="tile flex w-[20rem] shrink-0 items-center gap-[1rem] rounded-card border border-hair bg-l1 p-[1rem] text-left">
                <span className="relative h-[3.5rem] w-[3.5rem] shrink-0 overflow-hidden rounded-card bg-l3">
                  <MImg src={d.imageUrl ?? null} label={d.name} width={160} className="absolute inset-0 h-full w-full" />
                </span>
                <span className="min-w-0">
                  <span className="block truncate text-row font-semibold">{d.name}</span>
                  <span className="block truncate text-small text-muted">Drop, opens {when(d)}</span>
                </span>
              </button>
            ))}
            {games.slice(0, 8).map((g) => (
              <SuiGameCard key={g.slug} game={g} />
            ))}
          </div>
        ) : (
          <p className="px-[2.5rem] text-small text-muted">Nothing new on the way right now. New games and drops show here first.</p>
        )}
      </div>
    </section>
  );
}
