// Every money, price, percent and count on the shell goes through this:
// tabular figures, cents dimmed, P/L always signed, never wraps
// or truncates. An unknown value renders nothing. Green and red only for money direction.

import { useState } from "react";
import { changedChars, numParts, type NumKind, type NumOptions } from "./format";

export type NumProps = NumOptions & {
  kind: NumKind;
  value: number | null | undefined;
  className?: string;
  /** Balance privacy: dots instead of the value. */
  hidden?: boolean;
  /** The value updates on screen: moved digits roll (200 ms) and the value tints once (600 ms). */
  live?: boolean;
  /** pnl and change colour themselves. "none" keeps the surrounding colour. */
  tone?: "auto" | "none";
};

const TONE = { up: "text-positive", down: "text-negative", flat: "text-secondary" } as const;
const BASE = "tabular-nums shrink-0 whitespace-nowrap rounded-[0.25rem]";

export function Num({ kind, value, className = "", hidden = false, live = false, tone = "auto", cents, compact, decimals }: NumProps) {
  const parts = numParts(kind, value, { cents, compact, decimals });
  const text = parts ? `${parts.sign}${parts.body}` : "";
  const fx = useRoll(live && !hidden ? text + (parts?.dim ?? "") : null, value);
  if (hidden) return <span className={`${BASE} ${className}`}>••••</span>;
  if (!parts) return null;
  const color = tone === "auto" && parts.tone ? TONE[parts.tone] : "";
  const tint = fx ? (fx.up ? "tint-up" : "tint-down") : "";
  return (
    <span key={fx?.version} className={`${BASE} ${color} ${tint} ${className}`}>
      {fx ? <Rolled text={text} offset={0} fx={fx} /> : text}
      {parts.dim && <span className="opacity-50">{fx ? <Rolled text={parts.dim} offset={text.length} fx={fx} /> : parts.dim}</span>}
    </span>
  );
}

type Roll = { version: number; up: boolean; moved: boolean[] };

function useRoll(text: string | null, value: number | null | undefined): Roll | null {
  const [seen, setSeen] = useState({ text, value });
  const [roll, setRoll] = useState<Roll | null>(null);
  if (text !== seen.text) {
    setSeen({ text, value });
    if (text && seen.text && value != null && seen.value != null)
      setRoll({ version: (roll?.version ?? 0) + 1, up: value >= seen.value, moved: changedChars(seen.text, text) });
  }
  return text == null ? null : roll;
}

function Rolled({ text, offset, fx }: { text: string; offset: number; fx: Roll }) {
  return (
    <>
      {[...text].map((ch, i) =>
        fx.moved[offset + i] ? (
          <span key={i} className={fx.up ? "num-roll-up" : "num-roll-down"}>
            {ch}
          </span>
        ) : (
          ch
        ),
      )}
    </>
  );
}
