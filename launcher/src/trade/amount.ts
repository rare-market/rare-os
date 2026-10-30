// Amounts for the controller: exact base-unit math, the preset rows, and the ladders LT and RT
// step through. Pure functions, unit tested.

export const SUI_TYPE = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
export const USDC_TYPE = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";

/** SUI kept back for gas when buying with "Max". */
export const GAS_RESERVE_MIST = 50_000_000n;

/** Buy amounts in SUI. */
export const BUY_PRESETS = [1, 5, 10, 25, 50];
export const SUI_LADDER = [0.1, 0.25, 0.5, 1, 2, 3, 5, 10, 15, 25, 50, 75, 100, 150, 250, 500, 1000, 2500, 5000, 10000];
/** Sell amounts as a share of the balance. */
export const SELL_PRESETS = [25, 50, 75, 100];
export const PCT_LADDER = [5, 10, 15, 20, 25, 33, 40, 50, 60, 66, 75, 80, 90, 100];
/** Perps margin in USD. */
export const MARGIN_PRESETS = [5, 10, 25, 50, 100];
export const USD_LADDER = [1, 2, 5, 10, 15, 20, 25, 50, 75, 100, 150, 250, 500, 1000, 2500, 5000, 10000];
export const LEVERAGE_LADDER = [1, 1.5, 2, 3, 4, 5, 6, 8, 10, 12, 15, 20, 25, 30, 40, 50, 75, 100];

export const isCoinType = (t: string) => /^0x[0-9a-fA-F]+::\w+::\w+$/.test(t);

/** "1.5" with 9 decimals -> 1500000000n. Digits past `decimals` are dropped, never rounded up. */
export function toBase(amount: string | number, decimals: number): bigint {
  const s = typeof amount === "number" ? numberString(amount) : amount.trim();
  if (!/^\d*\.?\d*$/.test(s) || s === "" || s === ".") return 0n;
  const [whole = "", frac = ""] = s.split(".");
  const digits = (whole || "0") + frac.slice(0, decimals).padEnd(decimals, "0");
  return BigInt(digits.replace(/^0+(?=\d)/, ""));
}

/** 1500000000n with 9 decimals -> "1.5". Exact, no trailing zeros. */
export function fromBase(raw: bigint, decimals: number): string {
  const neg = raw < 0n;
  const abs = neg ? -raw : raw;
  const s = abs.toString().padStart(decimals + 1, "0");
  const whole = s.slice(0, s.length - decimals);
  const frac = decimals ? s.slice(s.length - decimals).replace(/0+$/, "") : "";
  return `${neg ? "-" : ""}${whole}${frac ? `.${frac}` : ""}`;
}

export const baseToNumber = (raw: bigint, decimals: number) => Number(fromBase(raw, decimals));

/** A plain decimal string for a JS number (no exponent). */
function numberString(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0";
  return n.toLocaleString("en-US", { useGrouping: false, maximumFractionDigits: 20 });
}

/**
 * The next value on a ladder from `current`, up (+1) or down (-1). It never passes `max`: the
 * step that would cross it lands on `max` itself, so the top of the range is always reachable.
 * `min` works the same way at the bottom.
 */
export function step(ladder: number[], current: number, dir: 1 | -1, max = Infinity, min = 0): number {
  const top = Math.max(min, max);
  if (dir > 0) {
    const next = ladder.find((v) => v > current + 1e-12);
    if (next === undefined || next > top) return current < top && Number.isFinite(top) ? top : current;
    return next;
  }
  const prev = [...ladder].reverse().find((v) => v < current - 1e-12 && v >= min);
  if (prev === undefined) return current > min && min > 0 ? min : current;
  return Math.min(prev, top);
}

/** What can be spent of a balance: SUI keeps the gas reserve back. */
export function spendable(raw: bigint, coinType: string): bigint {
  if (coinType !== SUI_TYPE) return raw > 0n ? raw : 0n;
  const left = raw - GAS_RESERVE_MIST;
  return left > 0n ? left : 0n;
}

/** `pct` percent of `raw`, exact at 100. */
export function pctOf(raw: bigint, pct: number): bigint {
  if (pct >= 100) return raw;
  if (pct <= 0) return 0n;
  return (raw * BigInt(Math.round(pct * 100))) / 10_000n;
}

/** The leverage stops for a market whose maximum is `max`. */
export function leverageStops(max: number | null | undefined): number[] {
  const m = max && max > 0 ? max : 1;
  const stops = LEVERAGE_LADDER.filter((v) => v <= m);
  if (stops[stops.length - 1] !== m) stops.push(m);
  return stops;
}

/** "5x", "1.5x" */
export const fmtLeverage = (x: number) => `${Number(x.toFixed(1))}x`;

/** Slippage the swap is built with, in basis points. */
export const SLIPPAGE_BPS = 100;

/** The least the trader receives at a slippage, base units. */
export function minOut(amountOut: bigint, slippageBps = SLIPPAGE_BPS): bigint {
  return (amountOut * BigInt(10_000 - slippageBps)) / 10_000n;
}

/** Price of one `coinOut` in `coinIn` units implied by a quote, as a JS number. */
export function impliedPrice(amountIn: bigint, decIn: number, amountOut: bigint, decOut: number): number | null {
  if (amountIn <= 0n || amountOut <= 0n) return null;
  return baseToNumber(amountIn, decIn) / baseToNumber(amountOut, decOut);
}

export function validPair(coinIn: string, coinOut: string): boolean {
  return isCoinType(coinIn) && isCoinType(coinOut) && normalizeType(coinIn) !== normalizeType(coinOut);
}

/** 0x2::sui::SUI -> the long form, so types compare equal. */
export function normalizeType(t: string): string {
  const m = /^0x([0-9a-fA-F]+)(::.+)$/.exec(t.trim());
  if (!m) return t;
  return `0x${m[1].toLowerCase().padStart(64, "0")}${m[2]}`;
}
