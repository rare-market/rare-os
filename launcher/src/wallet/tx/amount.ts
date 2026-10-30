// Amount parsing and formatting for the send screens and the review sheet. Base units are
// bigint (or decimal strings); nothing here goes through floats except USD display.

/** Base units -> "1,234.5678" with at most `maxFrac` fraction digits, trailing zeros dropped. */
export function formatUnits(base: string | bigint, decimals: number, maxFrac = 4): string {
  const v = typeof base === "bigint" ? base : BigInt(base || "0");
  const neg = v < 0n;
  const abs = neg ? -v : v;
  const s = abs.toString().padStart(decimals + 1, "0");
  const whole = s.slice(0, s.length - decimals);
  const frac = s.slice(s.length - decimals).slice(0, maxFrac).replace(/0+$/, "");
  const w = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return `${neg ? "-" : ""}${w}${frac ? "." + frac : ""}`;
}

/**
 * Display amount: 4 fraction digits for whole amounts, more for small ones so a fee like
 * 0.00199 SUI does not read as 0.0019 or 0.
 */
export function formatAmount(base: string | bigint, decimals: number): string {
  const v = typeof base === "bigint" ? base : BigInt(base || "0");
  const abs = v < 0n ? -v : v;
  const unit = 10n ** BigInt(decimals);
  if (abs === 0n || abs >= unit) return formatUnits(v, decimals, 4);
  // first significant digit position, then 3 more digits
  const frac = abs.toString().padStart(decimals, "0");
  const lead = frac.search(/[1-9]/);
  return formatUnits(v, decimals, Math.min(decimals, lead + 4));
}

/** "1.5" -> base units string. Returns null for empty or invalid input. */
export function parseUnits(input: string, decimals: number): string | null {
  const t = input.trim();
  if (!/^\d*\.?\d*$/.test(t) || t === "" || t === ".") return null;
  const [w, f = ""] = t.split(".");
  const frac = f.slice(0, decimals).padEnd(decimals, "0");
  return BigInt((w || "0") + frac).toString();
}

export const toNumber = (base: string | bigint, decimals: number): number => Number(formatUnits(base, decimals, decimals).replace(/,/g, ""));

export function usd(n: number): string {
  if (!isFinite(n)) return "$0.00";
  return n.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 2 });
}

export function price(n: number): string {
  if (!n) return "$0.00";
  const digits = n < 0.01 ? 6 : n < 1 ? 4 : 2;
  return n.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits: digits });
}

/** Leave a little SUI behind when sending most of it, so the transfer can still pay gas. */
export const SUI_GAS_RESERVE = 10_000_000n;

export type Chip = 25 | 50 | 100;

/** Base units for a quick chip. 25% / 50% never eat into the SUI gas reserve. Max on SUI uses buildCoinSend's max mode instead. */
export function chipAmount(available: bigint, pct: Chip, isSui: boolean): bigint {
  const cap = isSui ? (available > SUI_GAS_RESERVE ? available - SUI_GAS_RESERVE : 0n) : available;
  if (pct === 100) return cap;
  const part = (available * BigInt(pct)) / 100n;
  return part < cap ? part : cap;
}

/** Base units -> plain decimal text for the input (no thousands separators, full precision). */
export const toInput = (base: bigint | string, decimals: number) => formatUnits(base, decimals, decimals).replace(/,/g, "");

/** Accept what a number field should: digits and one dot (comma typed as a dot). */
export function cleanAmount(v: string): string | null {
  const s = v.replace(",", ".");
  return /^\d*\.?\d*$/.test(s) ? s : null;
}

/** USD text -> token base units at `price`, rounded down. null when empty, invalid or no price. */
export function usdToBase(usdText: string, priceUsd: number | null | undefined, decimals: number): string | null {
  const n = Number(usdText);
  if (!usdText || !isFinite(n) || !priceUsd) return null;
  // toFixed keeps it out of exponent form; trim to the coin's precision, rounding down
  return parseUnits((n / priceUsd).toFixed(Math.min(decimals, 20)), decimals);
}

/** Token base units -> USD number. */
export const baseToUsd = (base: string | bigint, decimals: number, priceUsd: number | null | undefined) =>
  priceUsd ? Number(toInput(base, decimals)) * priceUsd : null;
