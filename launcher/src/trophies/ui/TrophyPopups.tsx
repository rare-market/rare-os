// Console-style unlock pop-ups, in the toast column under the top bar (Chrome.tsx), one at a time: a new local Steam
// unlock, or a trophy minted by a claim. Held while a game runs, shown when you are back.

import { useEffect } from "react";
import { useApp } from "../../lib/store";
import { feedback } from "../../nav/feedback";
import { useTrophies } from "../store";
import { SOURCE_NAME, TIER_NAME } from "../types";
import { TrophyArt } from "./bits";

const SHOW_MS = 4800;

export function TrophyPopups() {
  const p = useTrophies((s) => s.popups[0] ?? null);
  const gaming = useApp((s) => s.running.length > 0);
  const id = p && !gaming ? p.id : null;

  useEffect(() => {
    if (id == null) return;
    feedback("notify");
    const t = setTimeout(() => useTrophies.getState().dropPopup(id), SHOW_MS);
    return () => clearTimeout(t);
  }, [id]);

  if (!p || gaming) return null;
  const label = p.kind === "minted" ? "Trophy minted" : p.tier != null ? "Trophy unlocked" : "Achievement unlocked";
  const meta = [p.game, p.tier != null ? TIER_NAME[p.tier] : null, p.points != null ? `${p.points} points` : null, p.source != null ? SOURCE_NAME[p.source] : null].filter(Boolean).join(" · ");
  return (
    <div key={p.id} role="status" className="pop flex w-[28rem] items-center gap-[0.85rem] rounded-card border border-border-light bg-l3 py-[0.7rem] pl-[0.7rem] pr-[1rem] shadow-xl shadow-black/30">
      <TrophyArt src={p.iconUrl} label={p.name} size="3rem" />
      <div className="min-w-0 flex-1">
        <div className="text-label text-secondary">{label}</div>
        <div className="truncate text-body font-semibold text-fg">{p.name}</div>
        <div className="truncate text-meta text-muted">
          {meta}
          {p.more > 0 && ` and ${p.more} more`}
        </div>
      </div>
    </div>
  );
}
