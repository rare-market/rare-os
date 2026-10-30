import type { ReactNode } from "react";

// A stat line: label over value, a hairline between items.
// Replaces every grid of stat tiles. Items with nothing to show are left out, so it never shows a dash.

export type StatItem = { label: ReactNode; value: ReactNode; key?: string };

export function StatLine({ items, align = "fill", size = "md", className = "" }: { items: readonly StatItem[]; align?: "fill" | "start"; size?: "md" | "lg"; className?: string }) {
  const shown = items.filter((i) => i.value != null && i.value !== false && i.value !== "");
  if (!shown.length) return null;
  return (
    <dl className={`flex ${className}`}>
      {shown.map((item, i) => (
        <div
          key={item.key ?? i}
          className={`flex min-w-0 shrink-0 flex-col border-hair ${align === "fill" ? "flex-1" : ""} ${i ? "border-l pl-[1rem]" : ""} ${i < shown.length - 1 ? "pr-[1rem]" : ""}`}
        >
          <dt className="whitespace-nowrap text-label text-secondary">{item.label}</dt>
          <dd className={`tabular-nums truncate whitespace-nowrap font-medium text-primary ${size === "lg" ? "text-row" : "text-body"}`}>{item.value}</dd>
        </div>
      ))}
    </dl>
  );
}
