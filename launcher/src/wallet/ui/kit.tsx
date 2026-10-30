// Rare Wallet UI kit: rare.market's own pieces (web/components/ui/modal.tsx, controls.tsx) made
// controller-first, plus the Marketplace's copies (marketplace/parts.tsx) re-exported so every
// wallet page uses the same buttons, tabs and states as the rest of the launcher. Don't invent
// new looks here: no coloured icon chips, no gradients, green only for positive numbers.

import { X } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { askText } from "../../components/Osk";
import { focusFirst } from "../../nav/focus";
import { usePadHandler } from "../../nav/input";
import type { PadButton } from "../../lib/types";
import { useLandingFocus } from "../../marketplace/page";

export { Avatar, Btn, Delta, Empty, Segmented, Stat, Tabs, WalletAvatar } from "../../marketplace/parts";
export { Qr } from "../../marketplace/BuySheet";

export const cn = (...c: Array<string | false | null | undefined>) => c.filter(Boolean).join(" ");

/**
 * A wallet page: title row, optional subtitle and right side, scrolling body. `narrow` centres a
 * form column (flows like send, import); default is the full launcher width.
 */
export function Page({
  title,
  subtitle,
  right,
  children,
  active,
  ready,
  narrow = false,
  className,
}: {
  title?: ReactNode;
  subtitle?: ReactNode;
  right?: ReactNode;
  children: ReactNode;
  active: boolean;
  /** changes when the page's data arrives, so focus lands on content */
  ready?: unknown;
  narrow?: boolean;
  className?: string;
}) {
  const root = useRef<HTMLDivElement>(null);
  useLandingFocus(root, active, ready);
  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto px-[2.5rem] pb-[2.5rem] pt-[0.75rem]">
      <div className={cn(narrow ? "mx-auto max-w-[44rem]" : "", className)}>
        {(title || right) && (
          <div className="mb-6 flex items-end justify-between gap-6">
            <div className="min-w-0">
              {title && <h1 className="truncate text-2xl font-bold leading-[1.4] tracking-[-0.01em]">{title}</h1>}
              {subtitle && <p className="mt-1 text-sm text-secondary">{subtitle}</p>}
            </div>
            {right && <div className="flex shrink-0 items-center gap-2">{right}</div>}
          </div>
        )}
        {children}
      </div>
    </div>
  );
}

/** rare.market panel: layer-01 card with the primary border. */
export function Panel({ children, className, title, right }: { children: ReactNode; className?: string; title?: ReactNode; right?: ReactNode }) {
  return (
    <section className={cn("rounded-lg border border-border-primary bg-layer-01", className)}>
      {(title || right) && (
        <div className="flex items-center justify-between gap-4 px-5 pt-4">
          {title && <h2 className="text-base font-semibold">{title}</h2>}
          {right}
        </div>
      )}
      {children}
    </section>
  );
}

/** Label on the left, value on the right (review sheets, details). */
export function Row({ label, children, className, muted = true }: { label: ReactNode; children: ReactNode; className?: string; muted?: boolean }) {
  return (
    <div className={cn("flex items-baseline justify-between gap-6 text-sm", className)}>
      <span className={muted ? "shrink-0 text-secondary" : "shrink-0"}>{label}</span>
      <span className="min-w-0 text-right">{children}</span>
    </div>
  );
}

/** A focusable list row (menus, account lists, settings). */
export function ListButton({
  icon,
  title,
  body,
  right,
  onClick,
  def,
  className,
}: {
  icon?: ReactNode;
  title: ReactNode;
  body?: ReactNode;
  right?: ReactNode;
  onClick: () => void;
  def?: boolean;
  className?: string;
}) {
  return (
    <button
      data-nav
      data-nav-default={def ? "" : undefined}
      onClick={onClick}
      className={cn("flex w-full items-center gap-4 rounded px-4 py-3.5 text-left transition-colors", className)}
    >
      {icon && <span className="flex shrink-0 items-center justify-center text-icon-primary">{icon}</span>}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-base font-medium">{title}</span>
        {body && <span className="focus-text mt-0.5 block text-sm leading-snug text-secondary">{body}</span>}
      </span>
      {right && <span className="shrink-0 text-sm text-secondary">{right}</span>}
    </button>
  );
}

/**
 * A text field for the controller: shows like rare.market's Input; A opens the on-screen
 * keyboard. `secret` hides the value (passphrases, private keys).
 */
export function Field({
  label,
  value,
  onChange,
  placeholder,
  secret = false,
  hint,
  error,
  def,
  className,
  prompt,
}: {
  label?: ReactNode;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  secret?: boolean;
  hint?: ReactNode;
  error?: ReactNode;
  def?: boolean;
  className?: string;
  /** title shown on the keyboard (defaults to the label) */
  prompt?: string;
}) {
  const open = async () => {
    const v = await askText(prompt ?? (typeof label === "string" ? label : placeholder ?? ""), value, { placeholder, secret });
    if (v !== null) onChange(v);
  };
  return (
    <div className={className}>
      {label && <div className="mb-1.5 text-sm text-secondary">{label}</div>}
      <button
        data-nav
        data-nav-default={def ? "" : undefined}
        data-hint-a="Type"
        onClick={() => void open()}
        className={cn(
          "flex h-12 w-full min-w-0 items-center rounded border bg-transparent px-4 text-left text-base transition-colors",
          error ? "border-negative/60" : "border-border-light",
        )}
      >
        <span className={cn("min-w-0 flex-1 truncate", value ? "text-primary" : "text-secondary")}>
          {value ? (secret ? "•".repeat(Math.min(24, value.length)) : value) : placeholder}
        </span>
      </button>
      {error ? <div className="mt-1.5 text-sm text-negative">{error}</div> : hint ? <div className="mt-1.5 text-sm text-secondary">{hint}</div> : null}
    </div>
  );
}

/** rare.market's Toggle, as a focusable row with its label. */
export function ToggleRow({ label, body, checked, onChange }: { label: ReactNode; body?: ReactNode; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button data-nav role="switch" aria-checked={checked} onClick={() => onChange(!checked)} className="flex w-full items-center gap-4 rounded px-4 py-3.5 text-left">
      <span className="min-w-0 flex-1">
        <span className="block text-base font-medium">{label}</span>
        {body && <span className="focus-text mt-0.5 block text-sm text-secondary">{body}</span>}
      </span>
      <span className={cn("relative h-6 w-[2.6rem] shrink-0 rounded-full transition-colors", checked ? "bg-primary" : "bg-layer-04")}>
        <span className={cn("absolute top-1/2 h-[1.1rem] w-[1.1rem] -translate-y-1/2 rounded-full transition-[left]", checked ? "left-[1.3rem] bg-background" : "left-[0.2rem] bg-icon-secondary")} />
      </span>
    </button>
  );
}

/**
 * rare.market's modal (ui/modal.tsx) for the launcher: dimmed backdrop, layer border, header with
 * a close button, optional footer. Traps controller focus; B closes unless `onClose` is omitted.
 */
export function Sheet({
  title,
  subtitle,
  icon,
  children,
  footer,
  onClose,
  width = "52rem",
  ready,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  icon?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  onClose?: () => void;
  width?: string;
  ready?: unknown;
}) {
  const root = useRef<HTMLDivElement>(null);
  useLandingFocus(root, true, ready ?? 0);
  usePadHandler((b) => {
    if (b === "b") {
      onClose?.();
      return true;
    }
    return false;
  });
  return createPortal(
    <div className="fixed inset-x-0 bottom-[3rem] top-0 z-40 flex items-center justify-center p-6">
      <div className="absolute inset-0 bg-background/50 backdrop-blur-sm" aria-hidden />
      <div
        ref={root}
        data-nav-scope
        role="dialog"
        className="fade-in relative flex max-h-full w-full flex-col overflow-hidden rounded-modal border border-border-light bg-background"
        style={{ maxWidth: width }}
      >
        <div className="flex shrink-0 items-start justify-between gap-4 border-b border-border-primary px-8 pb-4 pt-6">
          <div className="flex min-w-0 items-center gap-3">
            {icon}
            <div className="min-w-0">
              <h2 className="truncate text-xl font-bold leading-[1.55]">{title}</h2>
              {subtitle && <div className="truncate text-sm text-secondary">{subtitle}</div>}
            </div>
          </div>
          {onClose && (
            <button data-nav onClick={onClose} aria-label="Close" className="-mr-1 mt-1 flex size-10 items-center justify-center rounded text-icon-primary">
              <X size="1.25rem" />
            </button>
          )}
        </div>
        <div data-scroll className="min-h-0 flex-1 overflow-y-auto px-8 py-6">
          {children}
        </div>
        {footer && <div className="flex shrink-0 gap-2 border-t border-border-primary px-8 py-4">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}

const PAD: string[] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "back", "0", "ok"];

/**
 * Number pad for PINs and amounts, made for the controller: the d-pad moves over the keys, A
 * presses, X deletes, Start confirms; digits on a keyboard work too. Dots show the PIN length.
 */
export function PinPad({
  length,
  value,
  onChange,
  onSubmit,
  minLength = 6,
  maxLength = 12,
  masked = true,
  disabled = false,
  okLabel = "OK",
}: {
  length?: number;
  value: string;
  onChange: (v: string) => void;
  onSubmit: (v: string) => void;
  minLength?: number;
  maxLength?: number;
  masked?: boolean;
  disabled?: boolean;
  okLabel?: string;
}) {
  const root = useRef<HTMLDivElement>(null);
  const v = useRef(value);
  v.current = value;
  const press = (k: string) => {
    if (disabled) return;
    if (k === "back") onChange(v.current.slice(0, -1));
    else if (k === "ok") {
      if (v.current.length >= minLength) onSubmit(v.current);
    } else if (v.current.length < (length ?? maxLength)) {
      const next = v.current + k;
      onChange(next);
      if (length && next.length === length) onSubmit(next);
    }
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (/^\d$/.test(e.key)) {
        e.preventDefault();
        e.stopPropagation();
        press(e.key);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });
  usePadHandler((b: PadButton) => {
    if (b === "x") {
      press("back");
      return true;
    }
    if (b === "start") {
      press("ok");
      return true;
    }
    return false;
  });
  useEffect(() => {
    const t = setTimeout(() => root.current && focusFirst(root.current), 50);
    return () => clearTimeout(t);
  }, []);
  const shown = length ?? Math.max(minLength, value.length);
  return (
    <div ref={root} className="flex flex-col items-center gap-6">
      <div className="flex h-6 items-center gap-3" aria-label={`${value.length} digits`}>
        {masked
          ? Array.from({ length: shown }, (_, i) => (
              <span key={i} className={cn("size-3.5 rounded-full transition-colors", i < value.length ? "bg-primary" : "bg-layer-04")} />
            ))
          : <span className="font-mono text-2xl tabular-nums">{value || " "}</span>}
      </div>
      <div className="grid grid-cols-3 gap-2">
        {PAD.map((k) => (
          <button
            key={k}
            data-nav
            data-nav-default={k === "5" ? "" : undefined}
            disabled={disabled || (k === "ok" && value.length < minLength)}
            onClick={() => press(k)}
            className={cn(
              "btn flex h-16 w-24 items-center justify-center rounded text-2xl font-medium tabular-nums disabled:opacity-40",
              k === "ok" ? "bg-button-primary text-base font-semibold" : "bg-button-secondary",
            )}
          >
            {k === "back" ? "Del" : k === "ok" ? okLabel : k}
          </button>
        ))}
      </div>
    </div>
  );
}

/** Copies text (address) and briefly says so. */
export function useCopy(): [boolean, (t: string) => void] {
  const [done, setDone] = useState(false);
  const copy = (t: string) => {
    void navigator.clipboard?.writeText(t).catch(() => {});
    setDone(true);
    setTimeout(() => setDone(false), 1500);
  };
  return [done, copy];
}

/** Address in mono, middle shortened. */
export function Addr({ address, full = false, className }: { address: string; full?: boolean; className?: string }) {
  return <span className={cn("font-mono tabular-nums [font-variant-ligatures:none]", full && "break-all", className)}>{full ? address : `${address.slice(0, 6)}...${address.slice(-4)}`}</span>;
}

/** Hints for the wallet tab's bottom bar while a page is on top. */
export { useWalletHints } from "./store";
