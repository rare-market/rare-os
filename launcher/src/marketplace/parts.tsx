// rare.market's UI pieces for the native Marketplace, copied from market/web/components (ui/display,
// ui/button, ui/controls, ui/states, home/movers, home/collections-table, collection/item-card,
// collection/holders-tab). Changes are for the controller only: every interactive piece is a
// [data-nav] button with a focus state, and hit areas are a step larger for couch distance.

import { History } from "lucide-react";
import type { ReactNode } from "react";
import { Glyph } from "../ui/controls";
import { SuiLogo } from "../ui/sui";
import type { PadButton } from "../lib/types";
import { useLoading, type CollectionRow, type Holder, type ItemCard, type MarketEvent } from "./api";
import { DASH, EVENT, TIER, VENUES, fmtInt, fmtPct, fmtSui, mistToSui, shortAddr, shortName, timeAgo } from "./format";
import { avatarUri } from "./identicon";
import { MImg } from "./MImg";

const cn = (...c: Array<string | false | null | undefined>) => c.filter(Boolean).join(" ");

/** Thin loading bar at the top of the marketplace while anything is fetching. No skeletons. */
export function MarketLoadBar() {
  const on = useLoading((s) => s.n > 0);
  return <div className={`pointer-events-none absolute left-0 right-0 top-0 z-30 h-[2px] ${on ? "load-bar" : ""}`} />;
}

// ---------- ui/display ----------

/** A SUI amount: value in primary, unit muted at the same size. */
export function Sui({ mist, dp = 3, compact = false, unit = true, className }: { mist: string | bigint | null | undefined; dp?: number; compact?: boolean; unit?: boolean; className?: string }) {
  const none = mistToSui(mist) === null;
  return (
    <span className={cn("whitespace-nowrap tabular-nums", none && "text-secondary", className)}>
      <span>{fmtSui(mist, { dp, compact })}</span>
      {unit && !none && <span className="ml-1 font-normal text-secondary">SUI</span>}
    </span>
  );
}

/** Percent change with a small triangle. Green only when positive. */
export function Delta({ value, dp = 1, className, triangle = true }: { value: number | null | undefined; dp?: number; className?: string; triangle?: boolean }) {
  // Unknown is left out: an empty cell, never a dash.
  if (value === null || value === undefined || !Number.isFinite(value)) return <span className={className} />;
  const up = value > 0;
  const down = value < 0;
  return (
    <span className={cn("whitespace-nowrap tabular-nums", up ? "text-positive" : down ? "text-negative" : "text-secondary", className)}>
      {triangle && (up || down) && (
        <svg width="0.5rem" height="0.5rem" viewBox="0 0 8 8" aria-hidden className={cn("mr-0.5 inline", down && "rotate-180")}>
          <path d="M4 1 7.5 7h-7z" fill="currentColor" />
        </svg>
      )}
      {fmtPct(value, dp)}
    </span>
  );
}

export function Verified({ size = "1rem", className }: { size?: string; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" className={cn("inline-block shrink-0", className)} role="img" aria-label="Verified">
      <path fill="rgb(var(--brand))" d="M8 .8l1.7 1.3 2.1-.2.8 2 2 .8-.2 2.1L15.2 8l-1.3 1.7.2 2.1-2 .8-.8 2-2.1-.2L8 15.2l-1.7-1.3-2.1.2-.8-2-2-.8.2-2.1L.8 8l1.3-1.7-.2-2.1 2-.8.8-2 2.1.2z" />
      <path d="M5.2 8.2 7.1 10l3.8-4" fill="none" stroke="#fff" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

/** Real marketplace icons (web/public/venues). */
export function VenueMark({ venue, size = "0.875rem" }: { venue: string; size?: string }) {
  const v = VENUES[venue];
  if (!v) return null;
  return (
    <span className="inline-flex shrink-0 items-center justify-center overflow-hidden rounded-[3px]" style={{ width: size, height: size }} title={`Listed on ${v.label}`}>
      <img src={v.icon} alt="" draggable={false} className="size-full object-contain" />
    </span>
  );
}

export function RarityBadge({ rarity, size = "md", className }: { rarity: ItemCard["rarity"]; size?: "sm" | "md"; className?: string }) {
  if (!rarity) return null;
  return (
    <span title={`Rank ${rarity.rank}, ${TIER[rarity.tier] ?? rarity.tier}`} className={cn("inline-flex items-center rounded-sm bg-labels-neutral/90 font-medium text-label-neutral-text tabular-nums", size === "sm" ? "h-5 px-1 text-[0.6875rem]" : "h-6 px-1.5 text-xs", className)}>
      {rarity.rank}
    </span>
  );
}

/** Collection logo: round, layer-03 behind, initial when the image fails. */
export function Avatar({ src, label, size = "2.5rem", square = false, className }: { src: string | null | undefined; label: string; size?: string; square?: boolean; className?: string }) {
  // SUI has no icon in its on-chain metadata: the droplet, never an "S".
  if (!src && label === "SUI")
    return (
      <span className={cn("inline-flex shrink-0 items-center justify-center rounded-full bg-layer-03", className)} style={{ width: size, height: size }}>
        <SuiLogo size={`calc(${size} * 0.55)`} />
      </span>
    );
  return (
    <span className={cn("relative inline-block shrink-0 overflow-hidden bg-layer-03", square ? "rounded-md" : "rounded-full", className)} style={{ width: size, height: size }}>
      <MImg src={src} label={label} width={120} className="absolute inset-0 size-full" />
    </span>
  );
}

/** Wallet picture: the generated gem pattern. */
export function WalletAvatar({ address, size = "1.5rem" }: { address: string; size?: string }) {
  return <span aria-hidden className="inline-block shrink-0 rounded-full bg-cover bg-center" style={{ width: size, height: size, backgroundImage: `url("${avatarUri(address)}")` }} />;
}

// ---------- ui/states ----------

/** Centred secondary text, no illustration. Used wherever a list is empty. */
export function Empty({ title, children, action, compact = false }: { title: ReactNode; children?: ReactNode; action?: ReactNode; compact?: boolean }) {
  return (
    <div className={cn("flex flex-col items-center justify-center gap-3 px-6 text-center", compact ? "py-10" : "py-20")}>
      <p className="text-sm text-secondary">{title}</p>
      {children && <div className="max-w-sm text-xs text-secondary">{children}</div>}
      {action}
    </div>
  );
}

// ---------- ui/button and ui/controls ----------

type Variant = "primary" | "secondary" | "dimmed" | "ghost" | "outline";
const VARIANTS: Record<Variant, string> = {
  primary: "bg-button-primary text-on-brand font-semibold focus-visible:bg-button-primary-hover",
  secondary: "bg-button-secondary text-primary focus-visible:bg-button-secondary-active",
  dimmed: "bg-button-dimmed text-secondary focus-visible:text-primary",
  ghost: "bg-transparent text-secondary focus-visible:text-primary",
  outline: "border border-border-light bg-transparent text-primary",
};
const SIZES = { sm: "h-8 px-3 text-sm gap-1.5", md: "h-10 px-4 text-sm gap-2", lg: "h-12 px-6 text-base gap-2" };

export function Btn({ children, onClick, variant = "secondary", size = "lg", round = false, def, className }: { children: ReactNode; onClick?: () => void; variant?: Variant; size?: keyof typeof SIZES; round?: boolean; def?: boolean; className?: string }) {
  return (
    <button
      data-nav
      data-nav-default={def ? "" : undefined}
      onClick={onClick}
      className={cn("btn relative inline-flex shrink-0 items-center justify-center whitespace-nowrap font-medium transition-colors", round ? "rounded-full" : "rounded", VARIANTS[variant], SIZES[size], className)}
    >
      {children}
    </button>
  );
}

/** Underline tabs: 16/500, active primary with a 3px brand bar. LT/RT switch them. */
export function Tabs<T extends string>({ items, value, onChange, className }: { items: Array<{ value: T; label: ReactNode }>; value: T; onChange: (v: T) => void; className?: string }) {
  return (
    <div role="tablist" className={cn("flex items-end gap-8", className)}>
      <span className="pb-2.5 opacity-70"><Glyph b={"lt" as PadButton} /></span>
      {items.map((t) => {
        const active = t.value === value;
        return (
          <button key={t.value} data-nav data-tab={t.value} role="tab" aria-selected={active} onClick={() => onChange(t.value)} className={cn("relative flex shrink-0 items-center whitespace-nowrap rounded px-1 pb-2.5 pt-1 text-base font-medium transition-colors", active ? "text-primary" : "text-secondary focus-visible:text-primary")}>
            {t.label}
            {active && <span className="absolute inset-x-0 bottom-0 h-[2px] rounded-full bg-fg" />}
          </button>
        );
      })}
      <span className="pb-2.5 opacity-70"><Glyph b={"rt" as PadButton} /></span>
    </div>
  );
}

export function Segmented<T extends string>({ items, value, onChange, className }: { items: Array<{ value: T; label: ReactNode }>; value: T; onChange: (v: T) => void; className?: string }) {
  return (
    <div role="radiogroup" className={cn("inline-flex gap-px", className)}>
      {items.map((it, i) => {
        const active = it.value === value;
        return (
          <button
            key={it.value}
            data-nav
            role="radio"
            aria-checked={active}
            onClick={() => onChange(it.value)}
            className={cn("inline-flex h-9 min-w-11 items-center justify-center px-3 text-sm transition-colors", i === 0 && "rounded-l", i === items.length - 1 && "rounded-r", active ? "bg-button-secondary-active text-primary" : "bg-button-secondary text-secondary focus-visible:text-primary")}
          >
            {it.label}
          </button>
        );
      })}
    </div>
  );
}

/** Header stat (collection/header.tsx StatStrip). */
export function Stat({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="shrink-0">
      <p className="text-xs text-secondary">{label}</p>
      <div className="mt-0.5 flex items-baseline gap-1 text-base font-semibold tabular-nums">{children}</div>
    </div>
  );
}

// ---------- home/movers: the trending card ----------

export function MoverCard({ c, onOpen, def }: { c: CollectionRow; onOpen: () => void; def?: boolean }) {
  return (
    <button data-nav data-nav-default={def ? "" : undefined} onClick={onOpen} className="tile group w-[20rem] shrink-0 overflow-hidden rounded-lg border border-border-primary bg-layer-01 text-left focus-visible:border-border-interactive-hover">
      <div className="relative w-full bg-layer-03" style={{ aspectRatio: "276 / 160" }}>
        <MImg src={c.imageUrl} label={c.name} width={560} className="absolute inset-0 size-full" />
      </div>
      <div className="p-4">
        <p className="flex items-center gap-1.5">
          <span className="truncate text-lg font-bold">{c.name}</span>
          {c.verified && <Verified />}
        </p>
        <div className="mt-3 grid grid-cols-3 gap-2">
          <div>
            <p className="text-xxs uppercase text-secondary">Floor price</p>
            <Sui mist={c.floorMist} dp={2} className="text-sm font-semibold" />
          </div>
          <div>
            <p className="text-xxs uppercase text-secondary">Volume 1d</p>
            <Sui mist={c.volumeMist} dp={1} compact className="text-sm font-semibold" />
          </div>
          <div>
            <p className="text-xxs uppercase text-secondary">Vol change 1d</p>
            <Delta value={c.volumeChangePct} dp={1} className="text-sm font-semibold" />
          </div>
        </div>
      </div>
    </button>
  );
}

// ---------- home/collections-table: rows as focusable buttons ----------

export const TABLE_COLS = "grid-cols-[minmax(8rem,3fr)_minmax(4rem,1.4fr)_minmax(4rem,1.4fr)_minmax(3.5rem,1fr)_minmax(4rem,1.4fr)_minmax(3rem,0.8fr)_minmax(4rem,1.2fr)]";

export function CollectionHead({ win }: { win: string }) {
  return (
    <div className={cn("trow grid h-[3.25rem] items-center px-3 text-right text-sm text-secondary", TABLE_COLS)}>
      <span className="text-left font-medium">
        <span className="mr-5">#</span> Collection
      </span>
      <span className="px-1 font-medium">Floor</span>
      <span className="px-1 font-medium">Top offer</span>
      <span className="whitespace-nowrap px-1 font-medium">Floor {win}</span>
      <span className="px-1 font-medium">Volume</span>
      <span className="px-1 font-medium">Sales</span>
      <span className="px-1 pr-2 font-medium">Listed</span>
    </div>
  );
}

export function CollectionTableRow({ r, onOpen, idx }: { r: CollectionRow; onOpen: () => void; idx?: number }) {
  return (
    <button data-nav data-idx={idx} onClick={onOpen} className={cn("trow cv-row grid h-16 w-full items-center rounded-lg px-3 text-right text-sm transition-colors focus-visible:bg-layer-03", TABLE_COLS)}>
      <span className="flex min-w-0 items-center gap-3 text-left">
        <span className="w-6 shrink-0 text-sm font-semibold tabular-nums">{r.rank}</span>
        <Avatar src={r.imageUrl} label={r.name} />
        <span className="flex min-w-0 items-center gap-1.5">
          <span className="truncate text-sm font-semibold">{r.name}</span>
          {r.verified && <Verified size="0.95rem" />}
        </span>
      </span>
      <span className="px-1">
        <Sui mist={r.floorMist} dp={2} className="font-semibold" />
      </span>
      <span className="px-1">
        <Sui mist={r.topOfferMist} dp={2} className="font-semibold" />
      </span>
      <span className="px-1 font-semibold">
        <Delta value={r.floorChangePct} />
      </span>
      <span className="px-1">
        <Sui mist={r.volumeMist} dp={2} compact className="font-semibold" />
      </span>
      <span className="px-1 font-semibold tabular-nums">{fmtInt(r.sales)}</span>
      <span className="px-1 pr-2 tabular-nums">
        <span className="block truncate font-semibold">{r.supply ? fmtPct((r.listedCount / r.supply) * 100, 1) : DASH}</span>
        <span className="block truncate text-xs text-secondary">
          {fmtInt(r.listedCount)} / {fmtInt(r.supply, { compact: true })}
        </span>
      </span>
    </button>
  );
}

// ---------- collection/item-card ----------

export function ItemCardView({ card, onOpen, idx, def, showCollection, className }: { card: ItemCard; onOpen: () => void; idx?: number; def?: boolean; showCollection?: boolean; className?: string }) {
  const listed = !!card.listing;
  return (
    <button
      data-nav
      data-idx={idx}
      data-nav-default={def ? "" : undefined}
      onClick={onOpen}
      className={cn("tile group relative flex flex-col overflow-hidden rounded-lg border border-transparent bg-layer-01 text-left transition-colors focus-visible:border-border-interactive-hover focus-visible:bg-layer-02", className)}
    >
      <span className="relative block aspect-square w-full bg-layer-03">
        <MImg src={card.imageUrl} label={card.collection?.name ?? card.name} width={400} className="absolute inset-0 size-full" />
        {card.rarity && (
          <span className="absolute bottom-1.5 right-1.5">
            <RarityBadge rarity={card.rarity} />
          </span>
        )}
      </span>
      <span className="flex w-full flex-col gap-y-2 p-3">
        <span className="truncate text-sm font-semibold">{shortName(card)}</span>
        {showCollection && <span className="-mt-1 truncate text-xs text-secondary">{card.collection?.name}</span>}
        <span className="flex h-5 items-center gap-1 text-sm">
          {listed ? (
            <>
              <Sui mist={card.listing!.allInMist ?? card.listing!.priceMist} dp={3} className="font-semibold" />
              <VenueMark venue={card.listing!.venue} />
            </>
          ) : (
            <span className="text-secondary">{card.staked ? "Staked" : "Not listed"}</span>
          )}
        </span>
        <span className="flex h-6 items-center gap-1 text-sm text-secondary">
          <History size="0.875rem" aria-hidden />
          <span className="tabular-nums">{card.lastSaleMist ? fmtSui(card.lastSaleMist, { dp: 3, trim: true }) : DASH}</span>
        </span>
      </span>
    </button>
  );
}

// ---------- activity table rows (item/item-view ActivityTab) ----------

export const EVENT_COLS = "grid-cols-[5rem_minmax(2.25rem,1fr)_6rem_6.25rem_6.25rem_4rem]";

export function EventHead({ item = true }: { item?: boolean }) {
  return (
    <div className={cn("trow grid h-8 items-center px-2 text-meta text-secondary [&>span]:font-medium", EVENT_COLS)}>
      <span>Type</span>
      <span>{item ? "Item" : "Venue"}</span>
      <span className="text-right">Price</span>
      <span>From</span>
      <span>To</span>
      <span className="text-right">Time</span>
    </div>
  );
}

export function EventRow({ e, onOpen, item = true, idx }: { e: MarketEvent; onOpen?: () => void; item?: boolean; idx?: number }) {
  return (
    <button data-nav data-idx={idx} onClick={onOpen} className={cn("trow grid h-14 w-full items-center border-t border-border-primary px-2 text-left text-meta transition-colors focus-visible:bg-layer-03", EVENT_COLS)}>
      <span className="text-secondary">{EVENT[e.kind] ?? e.kind}</span>
      {item ? (
        <span className="flex min-w-0 items-center gap-2">
          {e.item && <Avatar src={e.item.imageUrl} label={e.item.name} size="2rem" square />}
          <span className="truncate">{e.item ? shortName({ name: e.item.name, collection: e.collection }) : DASH}</span>
        </span>
      ) : (
        <span className="flex min-w-0 items-center gap-1.5" title={VENUES[e.venue ?? ""]?.label}>
          {e.venue && <VenueMark venue={e.venue} />}
          <span className="min-w-0 truncate">{VENUES[e.venue ?? ""]?.label ?? DASH}</span>
        </span>
      )}
      <span className="text-right tabular-nums">{e.priceMist ? fmtSui(e.priceMist, { dp: 3 }) : DASH}</span>
      <span className="truncate" title={e.from ?? undefined}>{e.fromName ?? shortAddr(e.from)}</span>
      <span className="truncate" title={e.to ?? undefined}>{e.toName ?? shortAddr(e.to)}</span>
      <span className="text-right text-secondary">{timeAgo(e.at)} ago</span>
    </button>
  );
}

// ---------- collection/holders-tab ----------

export const HOLDER_COLS = "grid-cols-[3rem_minmax(0,1fr)_6rem_14rem_5rem]";

export function HolderRow({ r, onOpen }: { r: Holder; onOpen: () => void }) {
  const w = Math.min(100, Math.max(r.pctOfSupply > 0 ? 2 : 0, r.pctOfSupply));
  return (
    <button data-nav onClick={onOpen} className={cn("trow grid h-12 w-full items-center border-t border-border-primary px-4 text-left text-sm transition-colors focus-visible:bg-layer-03", HOLDER_COLS)}>
      <span className="font-mono text-secondary tabular-nums">{r.rank}</span>
      <span className="flex min-w-0 items-center gap-2">
        <WalletAvatar address={r.address} />
        <span className={cn("min-w-0 truncate", !r.name && "font-mono")}>{r.name ?? shortAddr(r.address)}</span>
      </span>
      <span className="text-right font-mono tabular-nums">{fmtInt(r.count)}</span>
      <span className="flex items-center gap-2">
        <span className="h-1 flex-1 overflow-hidden rounded-full bg-layer-03" aria-hidden>
          <span className="block h-full rounded-full bg-fg/45" style={{ width: `${w}%` }} />
        </span>
        <span className="w-14 text-right font-mono tabular-nums text-secondary">{fmtPct(r.pctOfSupply, r.pctOfSupply < 1 ? 2 : 1)}</span>
      </span>
      <span className="text-right font-mono tabular-nums">{r.listed ? fmtInt(r.listed) : <span className="text-secondary">0</span>}</span>
    </button>
  );
}

export function SectionTitle({ children, right }: { children: ReactNode; right?: ReactNode }) {
  return (
    <div className="mb-4 flex items-center gap-3">
      <h2 className="text-xl font-bold">{children}</h2>
      {right && <div className="ml-auto flex items-center gap-2 text-sm text-secondary">{right}</div>}
    </div>
  );
}

/** Calls `more` when focus lands within the last two rows of a grid. */
export function nearEnd(count: number, cols: number, more: () => void) {
  return (e: React.FocusEvent<HTMLElement>) => {
    const idx = Number((e.target as HTMLElement).dataset.idx);
    if (Number.isFinite(idx) && idx >= count - cols * 2) more();
  };
}
