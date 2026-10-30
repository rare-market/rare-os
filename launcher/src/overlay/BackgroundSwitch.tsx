// "Keep running in the background" on a game's page: quick resume pauses a game you switch away
// from (freeze.rs); this keeps one running instead. On by default for online games (a Sui game,
// anti-cheat) and apps; not offered where Rare never pauses (streams, web games in Rare's window).

import { useEffect, useState } from "react";
import { feedback } from "../nav/feedback";
import { Toggle } from "../ui";
import { overlayNative, type Background, type BackgroundWhy } from "./native";

/** The line under the switch: what happens, and why when it is the default. */
export function backgroundLine(b: Pick<Background, "pause" | "why">): string {
  if (b.pause) return "Pauses when you switch away, and picks up where you left it.";
  const why: Partial<Record<BackgroundWhy, string>> = {
    online: "On for online games, so you stay connected.",
    app: "On for apps, so calls, messages and music carry on.",
    webGame: "On for web games, so you stay connected.",
  };
  return why[b.why] ?? "Keeps running when you switch away.";
}

export function BackgroundSwitch({ id }: { id: string }) {
  const [b, setB] = useState<Background | null>(null);
  useEffect(() => {
    let live = true;
    void overlayNative.background(id).then((x) => live && setB(x), () => live && setB(null));
    return () => {
      live = false;
    };
  }, [id]);
  if (!b?.canChange) return null;
  const on = !b.pause;
  const flip = () => {
    feedback("toggle");
    void overlayNative.setBackground(id, !on).then(() => overlayNative.background(id).then(setB));
  };
  return (
    <div data-nav-group="game-background" className="flex">
      <button data-nav data-hint-a={on ? "Turn off" : "Turn on"} onClick={flip} className="flex items-center gap-[0.9rem] rounded-card px-[0.75rem] py-[0.5rem] text-left">
        <Toggle on={on} />
        <span className="min-w-0">
          <span className="block text-small font-medium text-fg">Keep running in the background</span>
          <span className="block text-label text-secondary">{backgroundLine(b)}</span>
        </span>
      </button>
    </div>
  );
}
