// Before Steam's own UI takes the screen (Big Picture, a store page), the launcher says how to come
// back, for as long as the native side waits before handing over (apps::steam, STEAM_HINT). Steam
// is an app in Rare: Home always brings Rare back, and the Running list has Resume and Close.

import { useEffect, useState } from "react";
import { onSteamHandoff } from "../lib/bridge";

export function SteamHandoff() {
  const [until, setUntil] = useState(0);
  useEffect(() => {
    let off: (() => void) | undefined;
    void onSteamHandoff((ms) => setUntil(Date.now() + ms + 400)).then((u) => (off = u));
    return () => off?.();
  }, []);
  useEffect(() => {
    if (!until) return;
    const t = setTimeout(() => setUntil(0), Math.max(0, until - Date.now()));
    return () => clearTimeout(t);
  }, [until]);
  if (!until) return null;
  return (
    <div className="pointer-events-none fixed inset-0 z-[60] flex items-center justify-center bg-black/55">
      <div className="pop flex w-[30rem] flex-col items-center gap-[0.9rem] rounded-card border border-border-light bg-l2 px-[2rem] py-[1.8rem] text-center shadow-xl shadow-black/40">
        <img src="apps/steam.png" alt="" draggable={false} className="h-[3.5rem] w-[3.5rem] rounded-[22%]" />
        <div className="text-row font-semibold">Opening Steam</div>
        <div className="text-body text-secondary">Press the Rare button to come back to Rare. Steam stays open in Running until you close it.</div>
      </div>
    </div>
  );
}
