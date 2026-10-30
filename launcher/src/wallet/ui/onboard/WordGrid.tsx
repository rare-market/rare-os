// A numbered grid of recovery words. Read-only (show a new phrase) or as buttons (import).

import { cn } from "../kit";

export function WordGrid({
  words,
  cols,
  onPick,
  active,
  bad,
  defIndex,
}: {
  words: string[];
  cols?: number;
  /** when given, each cell is a focusable button (import) */
  onPick?: (i: number) => void;
  active?: number;
  bad?: Set<number>;
  defIndex?: number;
}) {
  const c = cols ?? (words.length > 12 ? 6 : 4);
  return (
    <ol className="grid gap-2" style={{ gridTemplateColumns: `repeat(${c}, minmax(0, 1fr))` }}>
      {words.map((w, i) => {
        const cell = (
          <>
            <span className="w-6 shrink-0 text-right text-xs tabular-nums text-secondary">{i + 1}</span>
            <span className={cn("min-w-0 truncate font-mono text-base", !w && "text-secondary")}>{w || "word"}</span>
          </>
        );
        const cls = cn(
          "flex h-11 items-center gap-2.5 rounded border px-3 text-left",
          bad?.has(i) ? "border-negative/60" : active === i ? "border-border-light" : "border-border-primary",
          onPick ? "bg-layer-01" : "bg-layer-01",
        );
        return (
          <li key={i} className="min-w-0">
            {onPick ? (
              <button data-nav data-nav-default={defIndex === i ? "" : undefined} onClick={() => onPick(i)} className={cn(cls, "w-full")}>
                {cell}
              </button>
            ) : (
              <div className={cls}>{cell}</div>
            )}
          </li>
        );
      })}
    </ol>
  );
}
