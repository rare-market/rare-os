// The Trade tab's pieces on the Rare OS UI kit (../ui: Num, StatLine, LoadBar, Glyph, the type
// scale and radii): the Trade API's coin row and trending card, the order button, preset chips, the Buy/Sell
// switch and the sheet. Every interactive piece is a [data-nav] button; green is only for money up
// and Buy, red only for money down and Sell.

import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import { LoadBar, MImg, Num, StatLine, numParts } from "../ui";
import { useTradeLoading, change24, logoUrl, type TokenRow } from "./api";
import { age } from "./format";

export { Num, StatLine };

export const cn = (...c: Array<string | false | null | undefined>) => c.filter(Boolean).join(" ");

/** Thin loading bar at the top of the Trade tab while anything is fetching. No skeletons. */
export function TradeLoadBar() {
  const on = useTradeLoading((s) => s.n > 0);
  return <LoadBar on={on} />;
}

/** Round coin logo; the symbol's initial when there is no image or it fails. */
export function CoinLogo({ src, symbol, size = "2.5rem", className }: { src: string | null | undefined; symbol: string; size?: string; className?: string }) {
  return (
    <span className={cn("relative inline-block shrink-0 overflow-hidden rounded-full bg-l3 [&>div]:!text-[0.9em]", className)} style={{ width: size, height: size, fontSize: `calc(${size} * 0.45)` }}>
      <MImg src={logoUrl(src)} label={symbol || "?"} width={96} className="absolute inset-0 size-full" />
    </span>
  );
}

/** the Trade API's row: avatar, name over one meta line, price and change on the right. */
export function TokenListRow({ t, onOpen, meta, idx, def }: { t: TokenRow; onOpen: () => void; meta?: ReactNode; idx?: number; def?: boolean }) {
  return (
    <button data-nav data-idx={idx} data-nav-default={def ? "" : undefined} data-hint-a="Trade" onClick={onOpen} className="cv-row flex h-[3.75rem] w-full items-center gap-[0.75rem] rounded-card px-[0.75rem] text-left">
      <CoinLogo src={t.logo} symbol={t.symbol} />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-body font-semibold">{t.symbol}</span>
        <span className="block truncate text-label text-secondary">{meta ?? t.name}</span>
      </span>
      <span className="flex flex-col items-end">
        <Num kind="price" value={t.priceUsd} className="text-body font-medium" />
        <Num kind="change" value={change24(t)} className="text-label" />
      </span>
    </button>
  );
}

/** Market cap and age, the meta line the Trade API puts under a coin's name. */
export function tokenMeta(t: TokenRow, opts: { age?: boolean } = {}) {
  const mc = numParts("market", t.mcap);
  const parts = [mc ? `MC ${mc.sign}${mc.body}` : null, opts.age && t.createdMs ? `${age(t.createdMs)} old` : null, !mc && !opts.age ? t.name : null].filter(Boolean);
  return parts.join(", ");
}

/** A trending card for a rail. */
export function TokenCard({ t, onOpen, def }: { t: TokenRow; onOpen: () => void; def?: boolean }) {
  return (
    <button data-nav data-nav-default={def ? "" : undefined} data-hint-a="Trade" onClick={onOpen} className="tile w-[15rem] shrink-0 rounded-card border border-hair bg-l1 p-[1rem] text-left">
      <span className="flex items-center gap-[0.75rem]">
        <CoinLogo src={t.logo} symbol={t.symbol} />
        <span className="min-w-0">
          <span className="block truncate text-body font-semibold">{t.symbol}</span>
          <span className="block truncate text-label text-secondary">{t.name}</span>
        </span>
      </span>
      <span className="mt-[1rem] flex items-baseline justify-between gap-[0.5rem]">
        <Num kind="price" value={t.priceUsd} className="text-row font-semibold" />
        <Num kind="change" value={change24(t)} className="text-small" />
      </span>
      <span className="mt-[0.25rem] flex justify-between gap-[0.5rem] text-label text-secondary">
        <span>MC {numParts("market", t.mcap)?.body ?? ""}</span>
        <span>Vol {numParts("market", t.volume24h)?.body ?? ""}</span>
      </span>
    </button>
  );
}

/** A label and value on one line, hairlines between lines (no box inside a panel). */
export function Line({ label, children, strong }: { label: string; children: ReactNode; strong?: boolean }) {
  return (
    <div className={cn("flex items-baseline justify-between gap-[1rem] border-b border-hair py-[0.45rem] text-small last:border-b-0", strong && "font-semibold")}>
      <span className={strong ? "" : "text-secondary"}>{label}</span>
      <span className="text-right tabular-nums">{children}</span>
    </div>
  );
}

/** A full-screen sheet in rare.market's modal style; it traps focus. */
export function Sheet({ title, sub, children, footer, onRef, wide }: { title: ReactNode; sub?: ReactNode; children: ReactNode; footer?: ReactNode; onRef?: (el: HTMLDivElement | null) => void; wide?: boolean }) {
  return createPortal(
    <div className="fixed inset-x-0 bottom-[3rem] top-0 z-40 flex items-center justify-center p-[1.5rem]">
      <div className="absolute inset-0 bg-background/80" aria-hidden />
      <div ref={onRef} data-nav-scope className={cn("fade-in relative flex max-h-full w-full flex-col overflow-hidden rounded-sheet border border-border-light bg-background shadow-2xl shadow-black/40", wide ? "max-w-[60rem]" : "max-w-[44rem]")}>
        <div className="shrink-0 border-b border-hair px-[2rem] pb-[1rem] pt-[1.5rem]">
          <h2 className="text-row font-semibold">{title}</h2>
          {sub && <p className="text-small text-secondary">{sub}</p>}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-[2rem] py-[1.25rem]">{children}</div>
        {footer && <div className="flex shrink-0 gap-[0.5rem] border-t border-hair px-[2rem] py-[1rem]">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}

/** Calm empty or error sentence inside a panel. */
export function Quiet({ children, className }: { children: ReactNode; className?: string }) {
  return <p className={cn("py-[2rem] text-center text-small text-secondary", className)}>{children}</p>;
}

type Tone = "buy" | "sell" | "primary" | "secondary";
// ui Button's variants, plus Sell in the negative colour (red only for sell).
const TONES: Record<Tone, string> = {
  buy: "bg-[rgb(var(--button-buy))] text-[#0E0A14] font-semibold",
  sell: "bg-negative text-fg font-semibold",
  primary: "bg-brand text-fg font-semibold",
  secondary: "bg-btn-2 text-fg font-medium",
};

/**
 * The order button: ui Button's look at size lg. A button that cannot act yet stays focusable so
 * the hint bar can say why.
 */
export function Action({ children, onClick, tone = "primary", disabled, def, hint, className, size = "lg" }: { children: ReactNode; onClick?: () => void; tone?: Tone; disabled?: boolean; def?: boolean; hint?: string; className?: string; size?: "md" | "lg" }) {
  return (
    <button
      data-nav
      data-nav-default={def ? "" : undefined}
      data-hint-a={hint}
      aria-disabled={disabled || undefined}
      onClick={() => !disabled && onClick?.()}
      className={cn(
        "btn inline-flex shrink-0 items-center justify-center gap-[0.55rem] whitespace-nowrap rounded-chip",
        size === "lg" ? "h-[3.25rem] px-[1.75rem] text-row" : "h-[2.85rem] px-[1.35rem] text-body",
        TONES[tone],
        disabled && "opacity-40",
        className,
      )}
    >
      {children}
    </button>
  );
}

/** A preset in a row of amounts. The chosen one is a lighter fill, not a colour. */
export function Chip({ children, on, onClick, def }: { children: ReactNode; on?: boolean; onClick: () => void; def?: boolean }) {
  return (
    <button data-nav data-nav-default={def ? "" : undefined} onClick={onClick} className={cn("inline-flex h-[2.5rem] min-w-0 flex-1 items-center justify-center rounded-chip px-[0.5rem] text-meta font-medium tabular-nums", on ? "bg-button-secondary-active text-primary" : "bg-button-secondary text-secondary")}>
      {children}
    </button>
  );
}

/**
 * Buy/Sell or Long/Short: two buttons the D-pad reaches (A picks). X flips it from anywhere on the
 * page, and the hint bar says so.
 */
export function SideSwitch<T extends string>({ items, value, onChange, group = "tr-side" }: { items: Array<{ value: T; label: string; tone: "buy" | "sell" }>; value: T; onChange: (v: T) => void; group?: string }) {
  return (
    <div data-nav-group={group} data-nav-remember className="grid grid-cols-2 gap-[0.25rem] rounded-card bg-l3 p-[0.25rem]" role="radiogroup">
      {items.map((it) => {
        const on = it.value === value;
        return (
          <button
            key={it.value}
            data-nav
            data-hint-a={`Switch to ${it.label}`}
            data-hint-x={items.find((o) => o.value !== it.value)?.label}
            role="radio"
            aria-checked={on}
            onClick={() => onChange(it.value)}
            className={cn("flex h-[2.5rem] items-center justify-center rounded-chip text-body font-semibold transition-colors duration-200", on ? (it.tone === "buy" ? "bg-[rgb(var(--button-buy))] text-[#0E0A14]" : "bg-negative text-fg") : "text-secondary")}
          >
            {it.label}
          </button>
        );
      })}
    </div>
  );
}

/** Timeframe chips: buttons the D-pad reaches, same look as the kit's Segmented. */
export function TfChips<T extends string>({ items, value, onChange, group }: { items: Array<{ value: T; label: string }>; value: T; onChange: (v: T) => void; group: string }) {
  return (
    <div data-nav-group={group} data-nav-remember role="radiogroup" className="inline-flex gap-px">
      {items.map((it, i) => (
        <button
          key={it.value}
          data-nav
          data-hint-a={`Show ${it.label}`}
          role="radio"
          aria-checked={it.value === value}
          onClick={() => onChange(it.value)}
          className={cn(
            "inline-flex h-[2.25rem] min-w-[2.6rem] items-center justify-center px-[0.7rem] text-meta font-medium",
            i === 0 && "rounded-l-chip",
            i === items.length - 1 && "rounded-r-chip",
            it.value === value ? "bg-button-secondary-active text-primary" : "bg-button-secondary text-secondary",
          )}
        >
          {it.label}
        </button>
      ))}
    </div>
  );
}

/** A minus and a plus around a value (amount, leverage): visible buttons for what LT and RT do. */
export function Stepper({ onStep, children, label, group }: { onStep: (dir: 1 | -1) => void; children: ReactNode; label: string; group: string }) {
  const btn = "inline-flex h-[2.5rem] w-[3rem] shrink-0 items-center justify-center rounded-chip bg-button-secondary text-row font-semibold text-secondary";
  return (
    <div data-nav-group={group} data-nav-remember className="flex items-center gap-[0.75rem]">
      <button data-nav data-hint-a={`Less ${label}`} aria-label={`Less ${label}`} onClick={() => onStep(-1)} className={btn}>
        &minus;
      </button>
      <div className="min-w-0 flex-1">{children}</div>
      <button data-nav data-hint-a={`More ${label}`} aria-label={`More ${label}`} onClick={() => onStep(1)} className={btn}>
        +
      </button>
    </div>
  );
}
