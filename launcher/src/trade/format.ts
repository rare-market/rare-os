// Plain-text formatters on the same rules as ./num.
import { compactFigure } from "../ui/format";

/* Plain-text formatters. They follow the same rules as <Num> (see
 * ./format-num) and return "" for an unknown value: a missing figure is
 * left out, never shown as dashes. */

/** A market figure as text: $1.23M, $45.6K, $50K, $812. */
export function usdCompact(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return "";
  return `${value < 0 ? "\u2212" : ""}$${compactFigure(Math.abs(value))}`;
}

/** Prices span twelve orders of magnitude on a memecoin terminal, so tiny ones
 * use subscript-zero notation: $0.0₅4123 means five zeros after the point. */
export function usdPrice(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value) || value <= 0) return "";
  if (value >= 1000)
    return `$${value.toLocaleString("en-US", {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    })}`;
  if (value >= 1) return `$${value.toFixed(value >= 100 ? 2 : 3)}`;
  if (value >= 0.001) return `$${value.toPrecision(3)}`;
  const zeros = Math.ceil(-Math.log10(value)) - 1;
  const digits = Math.round(value * 10 ** (zeros + 4));
  return `$0.0${subscript(zeros)}${String(digits).replace(/0+$/, "") || "0"}`;
}

/** usdPrice with the digits a per-second chart needs: moves that the
 * everyday label rounds away stay visible. */
export function usdPriceFine(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value) || value <= 0) return "";
  if (value >= 1000) return usdPrice(value);
  if (value >= 100) return `$${value.toFixed(3)}`;
  if (value >= 1) return `$${value.toFixed(4)}`;
  if (value >= 0.001) return `$${value.toPrecision(4)}`;
  return usdPrice(value);
}

const SUB = "₀₁₂₃₄₅₆₇₈₉";
function subscript(n: number): string {
  return String(n)
    .split("")
    .map((d) => SUB[Number(d)])
    .join("");
}

/** A percentage without its sign, for places that draw the sign
 * themselves. Prefer <Num kind="change">. */
export function pct(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return "";
  const abs = Math.abs(value);
  const body =
    abs >= 1000
      ? abs.toLocaleString("en-US", { maximumFractionDigits: 0 })
      : abs.toFixed(2);
  return `${body}%`;
}

/** Exact under 1,000, then compact: 901, 23.4K. */
export function count(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return "";
  const abs = Math.abs(value);
  const body =
    abs < 999.5
      ? String(Math.round(abs))
      : abs < 999_950
        ? `${(abs / 1000).toFixed(1).replace(/\.0$/, "")}K`
        : compactFigure(abs);
  return `${value < 0 ? "\u2212" : ""}${body}`;
}

/** 3s, 14m, 5h, 2d, 3mo */
export function age(ms: number | null | undefined, now = Date.now()): string {
  if (!ms) return "";
  const s = Math.max(0, (now - ms) / 1000);
  if (s < 60) return `${Math.floor(s)}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  if (s < 86400 * 60) return `${Math.floor(s / 86400)}d`;
  if (s < 86400 * 365) return `${Math.floor(s / (86400 * 30))}mo`;
  return `${Math.floor(s / (86400 * 365))}y`;
}

export function shortAddr(addr: string | null | undefined): string {
  if (!addr) return "";
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`;
}

/** A name for running text: a bare address (or coin type) standing in for
 * a name becomes 0x1234…abcd so it never runs off the screen. */
export function shortName(name: string): string {
  const m = /^(0x[0-9a-f]{16,})(::.*)?$/i.exec(name.trim());
  const short = m ? shortAddr(m[1]) : name;
  // Word joiners keep the line from breaking at the ellipsis.
  return short.replace(/(0x[0-9a-f]+)…([0-9a-f]+)/gi, "$1\u2060…\u2060$2");
}
