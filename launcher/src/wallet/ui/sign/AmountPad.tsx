// Number pad for amounts: the kit PinPad's look (same keys, same sizes) with a decimal point
// instead of OK. The d-pad moves over the keys, A presses, X deletes, Start submits; digits and
// "." on a keyboard work too. Turn `active` off while a sheet is open so keys go to the sheet.

import { useEffect, useRef } from "react";
import { usePadHandler } from "../../../nav/input";
import { cn } from "../kit";
import { padPress } from "./padKeys";

const KEYS = ["1", "2", "3", "4", "5", "6", "7", "8", "9", ".", "0", "back"];

export function AmountPad({
  value,
  onChange,
  decimals,
  onSubmit,
  active = true,
}: {
  value: string;
  onChange: (v: string) => void;
  decimals: number;
  onSubmit?: () => void;
  active?: boolean;
}) {
  const v = useRef(value);
  v.current = value;
  const press = (k: string) => {
    if (!active) return;
    const next = padPress(v.current, k, decimals);
    if (next !== v.current) onChange(next);
  };
  useEffect(() => {
    if (!active) return;
    const onKey = (e: KeyboardEvent) => {
      const k = e.key === "," ? "." : e.key;
      if (/^[\d.]$/.test(k)) {
        e.preventDefault();
        e.stopPropagation();
        press(k);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });
  usePadHandler((b) => {
    if (b === "x") {
      press("back");
      return true;
    }
    if (b === "start") {
      onSubmit?.();
      return !!onSubmit;
    }
    return false;
  }, active);
  return (
    <div className="grid grid-cols-3 gap-2" role="group" aria-label="Amount keys">
      {KEYS.map((k) => (
        <button
          key={k}
          data-nav
          data-nav-default={k === "5" ? "" : undefined}
          disabled={!active}
          onClick={() => press(k)}
          className={cn("btn flex h-14 items-center justify-center rounded bg-button-secondary text-2xl font-medium tabular-nums disabled:opacity-40", k === "back" && "text-base")}
        >
          {k === "back" ? "Del" : k}
        </button>
      ))}
    </div>
  );
}
