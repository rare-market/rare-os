// The two button sheets of the UX model: Menu opens Options for the focused item, View opens the
// screen's view. Both are one list in the launcher's standard sheet. B, Menu or View close it, and
// every other press is swallowed so nothing moves behind it.

import { useEffect, useRef } from "react";
import { create } from "zustand";
import { feedback } from "../nav/feedback";
import { focusFirst } from "../nav/focus";
import { useFavourites } from "../lib/favourites";
import { removeWebGame } from "../lib/webgames";
import { native } from "../lib/bridge";
import { useApp } from "../lib/store";
import type { Game } from "../lib/types";
import { usePadHandler } from "../nav/input";
import { useHints } from "../nav/hints";
import { Sheet } from "../trade/parts";
import { Check } from "lucide-react";
import { Button } from "../ui";

export interface OptionItem {
  label: string;
  /** Marks the current choice in a View list. */
  on?: boolean;
  onSelect: () => void;
}
interface SheetSpec {
  title: string;
  sub?: string;
  items: OptionItem[];
}

const useSheet = create<{ spec: SheetSpec | null }>(() => ({ spec: null }));
const close = () => useSheet.setState({ spec: null });

export function openOptionsSheet(spec: SheetSpec) {
  feedback("open");
  useSheet.setState({ spec });
}

/** Menu: Options for a game or app. Play, Details, Favourite, Open in Steam and Remove. */
export function gameOptions(game: Game): SheetSpec {
  const s = useApp.getState();
  const running = s.running.includes(game.id);
  const fav = useFavourites.getState().ids.includes(game.id);
  const items: OptionItem[] = [
    { label: running ? "Resume" : game.source === "apps" ? "Open" : "Play", onSelect: () => void s.launch(game) },
    { label: "Details", onSelect: () => s.open({ kind: "game", id: game.id }) },
    {
      label: fav ? "Remove from favourites" : "Add to favourites",
      onSelect: () => {
        useFavourites.getState().toggle(game.id);
        s.toast(fav ? "Removed from favourites" : "Added to favourites");
      },
    },
  ];
  if (game.source === "steam") items.push({ label: "Open in Steam", onSelect: () => void native.steamBigPicture() });
  if (game.source === "web") items.push({ label: "Remove", onSelect: () => void removeWebGame(game.id) });
  return { title: game.title, sub: "Options", items };
}

/** The game the focused element belongs to: a shelf tile, a grid tile or the stage's buttons. */
export function focusedGame(): Game | null {
  const el = document.activeElement as HTMLElement | null;
  const id = el?.closest<HTMLElement>("[data-game]")?.dataset.game;
  return id ? useApp.getState().library?.games.find((g) => g.id === id) ?? null : null;
}

export function OptionsHost() {
  const spec = useSheet((s) => s.spec);
  const ref = useRef<HTMLDivElement | null>(null);
  usePadHandler((b) => {
    if (b === "b" || b === "start" || b === "select") {
      feedback("back");
      close();
      return true;
    }
    return b === "lb" || b === "rb" || b === "lt" || b === "rt" || b === "x" || b === "y";
  }, !!spec);
  useHints([{ b: "b", label: "Close" }], !!spec);
  useEffect(() => {
    if (!spec) return;
    const t = setTimeout(() => ref.current && focusFirst(ref.current), 20);
    return () => clearTimeout(t);
  }, [spec]);
  if (!spec) return null;
  return (
    <Sheet onRef={(el) => (ref.current = el)} title={spec.title} sub={spec.sub}>
      <div data-nav-group="options" className="flex flex-col gap-[0.5rem]">
        {spec.items.map((it, i) => (
          <Button
            key={it.label}
            size="lg"
            def={spec.items.some((x) => x.on) ? !!it.on : i === 0}
            variant="secondary"
            className="w-full justify-start gap-[0.75rem]"
            icon={it.on !== undefined ? <Check size="1.1rem" strokeWidth={2} className={it.on ? "" : "opacity-0"} /> : undefined}
            onClick={() => {
              close();
              it.onSelect();
            }}
          >
            {it.label}
          </Button>
        ))}
      </div>
    </Sheet>
  );
}
