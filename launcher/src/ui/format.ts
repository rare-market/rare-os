// Number rules for Rare OS.
//
//   money   your money: full, cents dimmed            $4,151.49
//   pnl     profit and loss: money, always signed     +$4,151.49
//   market  market figures: compact, 4 digits at most $10.91M, $247K
//   price   3 significant digits, subscript zeros     $0.0109, $0.0₅6093
//   change  sign and colour, 2 decimals under 100%    +4.15%, −9.46%
//   count   exact under 1,000, then compact           901, 23.4K
//   share   whole percent or 1 decimal                36%, 4.6%
//   amount  a coin quantity                           12.5, 1.25M
//
// An unknown value has no parts: the caller leaves it out.

export type NumKind =
  | "money"
  | "pnl"
  | "market"
  | "price"
  | "change"
  | "count"
  | "share"
  | "amount";

/** up and down color the number; flat is a signed kind sitting at zero. */
export type NumTone = "up" | "down" | "flat" | null;

export type NumOptions = {
  /** money, pnl: drop the cents ("+$412" in a headline). */
  cents?: boolean;
  /** money, pnl: compact like a market figure ("+$35.6K") for totals that are
   * not one person's balance. */
  compact?: boolean;
  /** share: 0 in a sentence ("36%"), 1 in a table ("4.6%"), 2 for a rate
   * ("5.27%" APY). Default 1. */
  decimals?: 0 | 1 | 2;
};

export type NumParts = {
  /** "+", "−" (a real minus sign) or "". */
  sign: "" | "+" | "−";
  /** The value itself, without the sign. */
  body: string;
  /** Cents, shown dimmed: ".49". Empty when there are none. */
  dim: string;
  tone: NumTone;
};

export const MINUS = "−";

function known(value: number | null | undefined): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

const UNITS: [number, string][] = [
  [1e15, "Q"],
  [1e12, "T"],
  [1e9, "B"],
  [1e6, "M"],
  [1e3, "K"],
];

/** 1,000 and up as at most four digits and a unit, never padded with
 * zeros: 1.98M, 10.91M, 247.3M, 123M, 6.3K, 81K.
 * `wideK` keeps one decimal on hundreds of thousands (counts: 317.3K);
 * money drops it ($247K). */
function compactUnit(abs: number, wideK: boolean): string {
  for (let i = 0; i < UNITS.length; i++) {
    const [size, unit] = UNITS[i];
    if (abs < size) continue;
    const n = abs / size;
    const decimals =
      unit === "K" ? (n < 100 || wideK ? 1 : 0) : n < 100 ? 2 : 1;
    const text = n.toFixed(decimals);
    // 999.96K rounds up to 1000K: say 1M instead.
    if (Number(text) >= 1000 && i > 0) return `1${UNITS[i - 1][1]}`;
    // Never padded: 123M, 94.05M, 1.5M, 81K.
    return `${trimZeros(text)}${unit}`;
  }
  return "";
}

/** A market figure without its currency sign. */
export function compactFigure(abs: number): string {
  if (abs >= 999.5) return compactUnit(Math.max(abs, 1000), false);
  if (abs >= 100) return abs.toFixed(0);
  if (abs >= 1 || abs === 0) return abs === 0 ? "0" : abs.toFixed(2);
  if (abs >= 0.01) return abs.toFixed(2);
  return Number(abs.toPrecision(2)).toString();
}

function fullMoney(abs: number, cents: boolean): { body: string; dim: string } {
  if (!cents)
    return { body: `$${Math.round(abs).toLocaleString("en-US")}`, dim: "" };
  if (abs > 0 && abs < 0.005) return { body: "<$0", dim: ".01" };
  const [whole, frac] = abs
    .toLocaleString("en-US", {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    })
    .split(".");
  return { body: `$${whole}`, dim: `.${frac}` };
}

function moneyParts(value: number, signed: boolean, o: NumOptions): NumParts {
  const abs = Math.abs(value);
  const { body, dim } = o.compact
    ? { body: `$${compactFigure(abs)}`, dim: "" }
    : fullMoney(abs, o.cents !== false);
  // What is shown decides the sign: −$0.001 reads as $0.00, not −$0.00.
  const zero = !o.compact && o.cents !== false ? abs < 0.005 : abs === 0;
  const sign = value < 0 && !zero ? MINUS : signed && !zero ? "+" : "";
  const tone: NumTone = !signed
    ? null
    : zero
      ? "flat"
      : value > 0
        ? "up"
        : "down";
  // A balance of dust says "<$0.01"; a result of dust is simply flat.
  if (zero && (signed || abs === 0) && !o.compact && o.cents !== false)
    return { sign: "", body: "$0", dim: ".00", tone };
  return { sign, body, dim, tone };
}

function changeBody(abs: number): string {
  if (abs < 100) return abs.toFixed(2);
  if (abs < 1000) return abs.toFixed(1);
  if (abs < 100_000) return Math.round(abs).toLocaleString("en-US");
  return compactUnit(abs, false);
}

function changeParts(value: number): NumParts {
  const body = changeBody(Math.abs(value));
  const zero = Number(body) === 0;
  return {
    sign: zero ? "" : value > 0 ? "+" : MINUS,
    body: `${body}%`,
    dim: "",
    tone: zero ? "flat" : value > 0 ? "up" : "down",
  };
}

function countBody(abs: number): string {
  return abs < 999.5
    ? String(Math.round(abs))
    : compactUnit(Math.max(abs, 1000), true);
}

function amountBody(abs: number): string {
  if (abs >= 1000) return compactUnit(abs, false);
  if (abs >= 1) return trimZeros(abs.toFixed(2));
  if (abs === 0) return "0";
  return Number(abs.toPrecision(4)).toString();
}

function trimZeros(text: string): string {
  return text.includes(".") ? text.replace(/\.?0+$/, "") : text;
}

/** The pieces of a formatted number, or null when the value is unknown. */
export function numParts(
  kind: NumKind,
  value: number | null | undefined,
  options: NumOptions = {}
): NumParts | null {
  if (!known(value)) return null;
  const abs = Math.abs(value);
  const minus = value < 0 ? MINUS : "";
  switch (kind) {
    case "money":
      return moneyParts(value, false, options);
    case "pnl":
      return moneyParts(value, true, options);
    case "market":
      return {
        sign: minus,
        body: `$${compactFigure(abs)}`,
        dim: "",
        tone: null,
      };
    case "price":
      // A price of zero or less is not a price we know.
      if (value <= 0) return null;
      return { sign: "", body: usdPrice(value), dim: "", tone: null };
    case "change":
      return changeParts(value);
    case "count":
      return { sign: minus, body: countBody(abs), dim: "", tone: null };
    case "share":
      return {
        sign: minus,
        body: `${abs.toFixed(options.decimals ?? 1)}%`,
        dim: "",
        tone: null,
      };
    case "amount":
      return { sign: minus, body: amountBody(abs), dim: "", tone: null };
  }
}

/** The same number as plain text ("" when unknown): for titles, aria labels
 * and sentences built outside JSX. */
export function numText(
  kind: NumKind,
  value: number | null | undefined,
  options: NumOptions = {}
): string {
  const p = numParts(kind, value, options);
  return p ? `${p.sign}${p.body}${p.dim}` : "";
}

/** Which characters of `next` differ from `prev`, compared from the right so
 * the cents line up when a digit is added on the left. Used to roll only the
 * digits that moved. */
export function changedChars(prev: string, next: string): boolean[] {
  const out: boolean[] = [];
  for (let i = 0; i < next.length; i++) {
    const fromEnd = next.length - 1 - i;
    const before = prev[prev.length - 1 - fromEnd];
    out.push(before !== next[i]);
  }
  return out;
}


/** the Trade API's price label: 3 significant digits, subscript zeros for tiny prices. */
export function usdPrice(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value) || value <= 0) return "";
  if (value >= 1000) return `$${value.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
  if (value >= 1) return `$${value.toFixed(value >= 100 ? 2 : 3)}`;
  if (value >= 0.001) return `$${value.toPrecision(3)}`;
  const zeros = Math.ceil(-Math.log10(value)) - 1;
  const digits = Math.round(value * 10 ** (zeros + 4));
  return `$0.0${subscript(zeros)}${String(digits).replace(/0+$/, "") || "0"}`;
}

const SUB = "₀₁₂₃₄₅₆₇₈₉";
function subscript(n: number): string {
  return String(n)
    .split("")
    .map((d) => SUB[Number(d)])
    .join("");
}
