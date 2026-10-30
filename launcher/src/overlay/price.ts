// A listing price typed on the on-screen keyboard: SUI with up to 9 decimals, to MIST.

import { askText } from "../components/Osk";
import { fmtSui } from "../marketplace/format";

const MIST = 1_000_000_000n;

/** "12", "12.5", "0.25", "1,200" to MIST; null for anything else or zero. */
export function parseSuiInput(text: string): bigint | null {
  const t = text.trim().replace(/,/g, "").replace(/\s*sui$/i, "");
  const m = /^(\d*)(?:\.(\d{0,9}))?$/.exec(t);
  if (!m || (!m[1] && !m[2])) return null;
  const whole = BigInt(m[1] || "0");
  const frac = BigInt((m[2] ?? "").padEnd(9, "0") || "0");
  const v = whole * MIST + frac;
  return v > 0n ? v : null;
}

export async function askPrice(current: bigint): Promise<bigint | null> {
  const start = current > 0n ? fmtSui(current, { dp: 9, trim: true }).replace(/,/g, "") : "";
  for (;;) {
    const t = await askText("Price in SUI", start, { placeholder: "e.g. 12.5" });
    if (t === null) return null;
    const v = parseSuiInput(t);
    if (v !== null) return v;
  }
}
