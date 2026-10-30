// The Trophies shelf on the profile: the gamerscore card, then the games with the most recent
// unlocks. A opens the trophy case (at that game).

import { useApp } from "../../lib/store";
import { Dot, Rail } from "../../ui";
import { TIER_NAME, type Tier } from "../types";
import { GameCard } from "./bits";
import { cardNote, useCaseData } from "./useCase";

const BACK = { kind: "profile" } as const;

export function ProfileTrophies() {
  const data = useCaseData("recent");
  const s = data.summary;
  if (!data.games.length) return null;
  const open = (appid?: number) => useApp.getState().open({ kind: "trophies", appid, from: BACK });
  const big = s.points != null ? s.points.toLocaleString("en-US") : s.steamUnlocked.toLocaleString("en-US");
  return (
    <Rail id="profile-trophies" title="Trophies" note={s.points != null ? `${s.trophies.toLocaleString("en-US")} minted` : "Steam achievements on this device"}>
      <button data-nav data-hint-a="Trophy case" onClick={() => open()} className="tile tile-wide flex w-[17rem] shrink-0 flex-col justify-between self-stretch rounded-card bg-l1 px-[1.1rem] py-[1rem] text-left">
        <span>
          <span className="block text-label text-secondary">{s.points != null ? "Points" : "Steam achievements"}</span>
          <span className="block tabular-nums text-display font-semibold">{big}</span>
        </span>
        <span className="flex flex-wrap gap-x-[0.9rem] gap-y-[0.3rem] text-meta text-muted">
          {s.points != null ? (
            ([3, 2, 1, 0] as Tier[]).map((t) => (
              <span key={t} className="inline-flex items-center gap-[0.35rem] tabular-nums">
                <Dot tone={t === 3 ? "primary" : "muted"} />
                {s.tiers[t]} {TIER_NAME[t]}
              </span>
            ))
          ) : (
            <span className="tabular-nums">
              {s.steamUnlocked} of {s.steamTotal} in {s.games} {s.games === 1 ? "game" : "games"}
            </span>
          )}
        </span>
      </button>
      {data.games.slice(0, 10).map((g) => (
        <GameCard key={g.appid} className="w-[17rem] shrink-0" appid={g.appid} name={g.name} sum={g.sum} note={cardNote(g)} onClick={() => open(g.appid)} />
      ))}
    </Rail>
  );
}
