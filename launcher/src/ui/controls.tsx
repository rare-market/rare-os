// Controls for the controller shell. Focus comes from [data-nav] (src/nav/focus.ts) and
// the .btn / .tile focus rules in styles/app.css. Every interactive piece is a [data-nav] button.

import type { ReactNode } from "react";
import type { PadButton } from "../lib/types";

const cn = (...c: Array<string | false | null | undefined>) => c.filter(Boolean).join(" ");

/** Controller button glyph. */
export function Glyph({ b, size = "md" }: { b: PadButton; size?: "sm" | "md" }) {
  const d = size === "sm" ? "h-[1.2rem] w-[1.2rem] text-[0.62rem]" : "h-[1.45rem] w-[1.45rem] text-[0.72rem]";
  const round = `inline-flex shrink-0 items-center justify-center rounded-full font-bold ${d}`;
  switch (b) {
    case "a":
      return <span className={`${round} bg-fg text-bg`}>A</span>;
    case "b":
    case "x":
    case "y":
      return <span className={`${round} bg-fg/85 text-bg`}>{b.toUpperCase()}</span>;
    case "lb":
    case "rb":
    case "lt":
    case "rt":
      return (
        <span className={`inline-flex shrink-0 items-center rounded-[0.4rem] bg-fg/12 px-[0.4rem] font-bold tracking-wide text-fg/90 ${size === "sm" ? "h-[1.1rem] text-[0.58rem]" : "h-[1.3rem] text-[0.66rem]"}`}>{b.toUpperCase()}</span>
      );
    case "start":
      return (
        <span className={`${round} border border-fg/40 text-fg`}>
          <svg viewBox="0 0 12 12" width="0.62rem" height="0.62rem">
            <path d="M2 3h8M2 6h8M2 9h8" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
          </svg>
        </span>
      );
    case "select":
      return (
        <span className={`${round} border border-fg/40 text-fg`}>
          <svg viewBox="0 0 12 12" width="0.62rem" height="0.62rem">
            <rect x="1.5" y="1.5" width="6" height="6" rx="1" fill="none" stroke="currentColor" strokeWidth="1.3" />
            <rect x="4.5" y="4.5" width="6" height="6" rx="1" fill="currentColor" />
          </svg>
        </span>
      );
    case "rare":
    case "guide":
      // The Rare button: a round key with the gem.
      return (
        <span className={`${round} border border-fg/70 text-fg`} title="Rare button">
          <svg viewBox="0 0 12 12" width="0.72rem" height="0.72rem">
            <path d="M6 1.2 10.2 4.6 6 10.8 1.8 4.6Z" fill="currentColor" />
            <path d="M1.8 4.6h8.4M4.2 4.6 6 1.2l1.8 3.4" fill="none" stroke="rgb(var(--background))" strokeWidth="0.7" strokeLinejoin="round" />
          </svg>
        </span>
      );
    case "quick":
    case "qam":
      // The Quick button: a round key with three dots.
      return (
        <span className={`${round} border border-fg/70 text-fg`} title="Quick button">
          <svg viewBox="0 0 12 12" width="0.72rem" height="0.72rem">
            <circle cx="2.4" cy="6" r="1.05" fill="currentColor" />
            <circle cx="6" cy="6" r="1.05" fill="currentColor" />
            <circle cx="9.6" cy="6" r="1.05" fill="currentColor" />
          </svg>
        </span>
      );
    case "lc":
    case "rc": {
      // The small keys above each shoulder: a short tab with the outer corner cut.
      const left = b === "lc";
      const w = size === "sm" ? "1.55rem" : "1.85rem";
      return (
        <span className="inline-flex shrink-0" title={left ? "LC key" : "RC key"}>
          <svg viewBox="0 0 30 18" width={w} height={`calc(${w} * 0.6)`}>
            <path d={left ? "M9 1.2h19a1.3 1.3 0 0 1 1.3 1.3v13a1.3 1.3 0 0 1-1.3 1.3H2.2Z" : "M21 1.2H2a1.3 1.3 0 0 0-1.3 1.3v13A1.3 1.3 0 0 0 2 16.8h25.8Z"} fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" className="text-fg/70" />
            <text x={left ? 17 : 13} y="12.2" textAnchor="middle" fontSize="8.5" fontWeight="700" fill="currentColor" className="text-fg">{left ? "LC" : "RC"}</text>
          </svg>
        </span>
      );
    }
    case "lbrb":
    case "ltrt":
      return (
        <span className="inline-flex shrink-0 items-center gap-[0.2rem]">
          <Glyph b={b === "lbrb" ? "lb" : "lt"} size={size} />
          <Glyph b={b === "lbrb" ? "rb" : "rt"} size={size} />
        </span>
      );
    case "exithold":
      // The way out of a game: View and Menu held together.
      return (
        <span className="inline-flex shrink-0 items-center gap-[0.2rem]">
          <Glyph b="select" size={size} />
          <Glyph b="start" size={size} />
        </span>
      );
    default:
      return <span className={`${round} border border-fg/40 text-[0.6rem] text-fg`}>{b.slice(0, 1).toUpperCase()}</span>;
  }
}

/** A glyph and its label, as in the hint bar. */
export function Hint({ b, children }: { b: PadButton; children: ReactNode }) {
  return (
    <span className="flex items-center gap-[0.45rem] whitespace-nowrap">
      <Glyph b={b} />
      {children}
    </span>
  );
}

export type ButtonVariant = "primary" | "secondary" | "ghost" | "dimmed" | "outline" | "buy";
const VARIANTS: Record<ButtonVariant, string> = {
  primary: "bg-brand text-fg font-semibold",
  secondary: "bg-btn-2 text-fg font-medium",
  dimmed: "bg-button-dimmed text-muted font-medium",
  ghost: "bg-transparent text-muted font-medium",
  outline: "border border-border-light bg-transparent text-fg font-medium",
  // Green only for buy.
  buy: "bg-[rgb(var(--button-buy))] text-[#0E0A14] font-semibold",
};
const SIZES = {
  sm: "h-[2.25rem] px-[0.9rem] text-small gap-[0.4rem]",
  md: "h-[2.85rem] px-[1.35rem] text-body gap-[0.55rem]",
  lg: "h-[3.25rem] px-[1.75rem] text-row gap-[0.6rem]",
};

/** rare.market's button, sized for the couch. Focus fills it white (.btn in app.css). */
export function Button({
  children,
  onClick,
  variant = "secondary",
  size = "md",
  icon,
  className = "",
  def,
  disabled,
  hint,
}: {
  children?: ReactNode;
  onClick?: () => void;
  variant?: ButtonVariant;
  size?: keyof typeof SIZES;
  icon?: ReactNode;
  className?: string;
  def?: boolean;
  disabled?: boolean;
  /** Label for A in the hint bar while this button has focus. */
  hint?: string;
}) {
  return (
    <button
      data-nav
      data-hint-a={hint}
      data-nav-default={def ? "" : undefined}
      disabled={disabled}
      onClick={onClick}
      className={cn("btn inline-flex shrink-0 items-center justify-center whitespace-nowrap rounded-chip disabled:opacity-40", VARIANTS[variant], SIZES[size], className)}
    >
      {icon}
      {children}
    </button>
  );
}

/** rare.market's switch. Display only: the row around it takes focus and A. */
export function Toggle({ on }: { on: boolean }) {
  return (
    <span aria-hidden className={cn("relative inline-block h-[1.5rem] w-[2.6rem] shrink-0 rounded-full transition-colors duration-150", on ? "bg-fg" : "bg-l4")}>
      <span
        className={cn("absolute top-1/2 h-[1.1rem] w-[1.1rem] -translate-y-1/2 rounded-full transition-transform duration-150", on ? "translate-x-[1.3rem] bg-bg" : "translate-x-[0.2rem] bg-icon-2")}
      />
    </span>
  );
}

/**
 * rare.market's underline tabs (a 3px brand bar under the current one). Display only: LB/RB or X
 * switch them, so they never take focus.
 */
export function Tabs<T extends string>({ items, value, className = "", size = "md" }: { items: Array<{ value: T; label: ReactNode; count?: number }>; value: T; className?: string; size?: "sm" | "md" }) {
  return (
    <div role="tablist" className={cn("flex items-stretch gap-[1.25rem]", className)}>
      {items.map((t) => {
        const on = t.value === value;
        return (
          <span
            key={t.value}
            role="tab"
            aria-selected={on}
            className={cn(
              "flex shrink-0 items-center whitespace-nowrap font-medium transition-colors duration-200",
              size === "md" ? "text-body" : "text-small",
              on ? "text-primary" : "text-secondary",
            )}
          >
            {/* The underline hugs the text (a few px below the baseline), not the full tab height. */}
            <span className="relative flex items-center gap-[0.4rem]">
              {t.label}
              {t.count != null && <span className={cn("tabular-nums text-meta", on ? "text-secondary" : "text-secondary/70")}>{t.count}</span>}
              {on && <span className="absolute inset-x-0 -bottom-[0.3rem] h-[2px] rounded-full bg-fg" />}
            </span>
          </span>
        );
      })}
    </div>
  );
}

/** rare.market's segmented control, display only (a time range, a filter). */
export function Segmented<T extends string>({ items, value, className = "" }: { items: Array<{ value: T; label: ReactNode }>; value: T; className?: string }) {
  return (
    <div role="radiogroup" className={cn("inline-flex gap-px", className)}>
      {items.map((it, i) => (
        <span
          key={it.value}
          role="radio"
          aria-checked={it.value === value}
          className={cn(
            "inline-flex h-[1.9rem] min-w-[2.4rem] items-center justify-center px-[0.6rem] text-meta font-medium",
            i === 0 && "rounded-l-chip",
            i === items.length - 1 && "rounded-r-chip",
            it.value === value ? "bg-button-secondary-active text-primary" : "bg-button-secondary text-secondary",
          )}
        >
          {it.label}
        </span>
      ))}
    </div>
  );
}

/** A status dot: the only coloured marker allowed (no pills). */
export function Dot({ tone = "muted", live = false, className = "" }: { tone?: "muted" | "brand" | "positive" | "negative" | "attention" | "primary"; live?: boolean; className?: string }) {
  // "brand" stays for callers but renders white: accent blue is for primary actions and focus only.
  const c = { muted: "bg-icon-2", brand: "bg-fg", positive: "bg-positive", negative: "bg-negative", attention: "bg-attention", primary: "bg-fg" }[tone];
  return <span aria-hidden className={cn("inline-block h-[0.45rem] w-[0.45rem] shrink-0 rounded-full", c, live && "breathe", className)} />;
}

/** Thin loading bar (rare.market's). Place it in a relative box; no skeletons anywhere. */
export function LoadBar({ on, className = "top-0" }: { on: boolean; className?: string }) {
  return <div aria-hidden className={cn("pointer-events-none absolute inset-x-0 z-30 h-[2px] transition-opacity duration-200", on ? "load-bar opacity-100" : "opacity-0", className)} />;
}

/** Calm empty state: a sentence, an optional line, one action. No illustration. */
export function Empty({ title, body, children, className = "" }: { title: string; body?: ReactNode; children?: ReactNode; className?: string }) {
  return (
    <div className={cn("flex flex-col items-start gap-[0.5rem] rounded-card border border-hair bg-l1 px-[1.5rem] py-[1.3rem]", className)}>
      <div className="text-row font-semibold">{title}</div>
      {body && <div className="max-w-[34rem] text-small text-muted">{body}</div>}
      {children && <div className="mt-[0.5rem] flex gap-[0.75rem]">{children}</div>}
    </div>
  );
}

/** rare.market's home table card: rounded-2xl layer-01, no border. */
export function Panel({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <div className={cn("rounded-panel bg-l1", className)}>{children}</div>;
}

/** Section title with a muted note or a hint on the right. */
export function SectionHead({ title, note, right, className = "" }: { title: ReactNode; note?: ReactNode; right?: ReactNode; className?: string }) {
  return (
    <div className={cn("flex items-center gap-[0.75rem]", className)}>
      <h2 className="text-row font-semibold tracking-[-0.01em]">{title}</h2>
      {note && <span className="text-small text-muted">{note}</span>}
      {right && <div className="ml-auto flex items-center gap-[0.75rem] text-small text-muted">{right}</div>}
    </div>
  );
}

/** A horizontal shelf of tiles. Focus scrolls it; [data-rail] keeps room for the focus ring. */
export function Rail({ id, title, note, right, children, className = "" }: { id: string; title?: ReactNode; note?: ReactNode; right?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cn("relative", className)}>
      {title && <SectionHead title={title} note={note} right={right} className="mb-[0.9rem] px-[2.5rem]" />}
      <div data-rail data-nav-group={id} className="relative flex items-start gap-[1rem] overflow-x-auto px-[2.5rem]">
        {children}
      </div>
    </section>
  );
}

/**
 * A focusable table row (hairline rows, numbers right aligned, 3.25rem high).
 * `cols` is a grid-template-columns class shared with the head row.
 */
export function Row({ cols, children, onClick, hint, idx, className = "" }: { cols: string; children: ReactNode; onClick?: () => void; hint?: string; idx?: number; className?: string }) {
  return (
    <button data-nav data-idx={idx} data-hint-a={hint} onClick={onClick} className={cn("trow grid h-[3.25rem] w-full items-center gap-x-[0.75rem] rounded-card px-[0.75rem] text-left text-small", cols, className)}>
      {children}
    </button>
  );
}

export function HeadRow({ cols, children, className = "" }: { cols: string; children: ReactNode; className?: string }) {
  return <div className={cn("trow grid h-[2rem] items-center gap-x-[0.75rem] px-[0.75rem] text-label font-medium text-secondary", cols, className)}>{children}</div>;
}

/**
 * An activity card: a solid card with a small label, one big value and a muted
 * line. Display only unless it gets onClick, then it is a focusable tile. Use in a CardRow.
 */
export function InfoCard({ label, value, sub, onClick, hint, className = "" }: { label: ReactNode; value: ReactNode; sub?: ReactNode; onClick?: () => void; hint?: string; className?: string }) {
  const body = (
    <>
      <span className="block text-label font-medium text-secondary">{label}</span>
      <span className="mt-auto block truncate text-title font-semibold tabular-nums text-fg">{value}</span>
      {sub && <span className="mt-[0.15rem] block truncate text-meta text-muted">{sub}</span>}
    </>
  );
  const cls = cn("flex h-[6rem] w-[14rem] shrink-0 flex-col rounded-card bg-l1 px-[1rem] py-[0.875rem] text-left", className);
  return onClick ? (
    <button data-nav data-hint-a={hint} onClick={onClick} className={cn("tile tile-wide", cls)}>
      {body}
    </button>
  ) : (
    <div className={cls}>{body}</div>
  );
}

/** A row of InfoCards that scrolls sideways under focus (a rail without a title). */
export function CardRow({ id, children, className = "" }: { id: string; children: ReactNode; className?: string }) {
  return (
    <div data-rail data-nav-group={id} className={cn("flex gap-[1rem] overflow-x-auto", className)}>
      {children}
    </div>
  );
}
