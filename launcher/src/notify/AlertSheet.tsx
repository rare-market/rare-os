// "Price alert" on a coin's page in Trade: pick a USD target above or below the price now (presets,
// a minus and plus, LT/RT, or type one), and rare.market sends one notice when it is crossed.

import { useRef, useState } from "react";
import { askText } from "../components/Osk";
import { useApp } from "../lib/store";
import { usePadHandler } from "../nav/input";
import { Num, numText, usdPrice } from "../ui";
import { useLandingFocus, useSheetPad } from "../trade/nav";
import { Action, Chip, Line, Sheet, Stepper } from "../trade/parts";
import { useAlerts } from "./alerts";

const PRESETS = [-10, -5, 5, 10, 25];

export function AlertSheet({ coinType, symbol, priceUsd, addr, onClose }: { coinType: string; symbol: string; priceUsd: number; addr?: string | null; onClose: () => void }) {
  const root = useRef<HTMLDivElement | null>(null);
  const [target, setTarget] = useState(() => round(priceUsd * 1.1));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const add = useAlerts((s) => s.add);
  const dir: "above" | "below" = target >= priceUsd ? "above" : "below";
  const pct = ((target - priceUsd) / priceUsd) * 100;
  const step = (d: 1 | -1) => setTarget((t) => round(Math.max(priceUsd * 0.01, t + d * priceUsd * 0.01)));

  useLandingFocus(root, true, null);
  useSheetPad(onClose, [{ b: "a", label: "Select" }, { b: "lt", label: "Lower" }, { b: "rt", label: "Higher" }, { b: "b", label: "Cancel" }]);
  // After the sheet's own handler, so LT/RT reach the target instead of being held.
  usePadHandler((b) => {
    if (b === "lt" || b === "rt") {
      step(b === "rt" ? 1 : -1);
      return true;
    }
    return false;
  });

  const save = async () => {
    if (busy || !(target > 0)) return;
    setBusy(true);
    setError(null);
    try {
      await add({ coinType, symbol, dir, target, addr: addr ?? undefined });
      useApp.getState().toast(`Price alert set: ${symbol} ${dir} ${usdPrice(target)}`);
      onClose();
    } catch {
      setError("Could not set the alert. Try again in a moment.");
    } finally {
      setBusy(false);
    }
  };

  const type = async () => {
    const v = await askText(`Alert price for ${symbol} in USD`, String(target), { placeholder: usdPrice(priceUsd) });
    const n = v == null ? NaN : Number(v.replace(/[$,\s]/g, ""));
    if (Number.isFinite(n) && n > 0) setTarget(round(n));
  };

  return (
    <Sheet
      title={`Price alert for ${symbol}`}
      sub="One notice when the price crosses your target."
      onRef={(el) => void (root.current = el)}
      footer={
        <>
          <Action def tone="primary" className="flex-1" disabled={busy} onClick={() => void save()} hint="Set alert">
            Set alert
          </Action>
          <Action tone="secondary" onClick={() => void type()} hint="Type a price">
            Type a price
          </Action>
          <Action tone="secondary" onClick={onClose}>
            Cancel
          </Action>
        </>
      }
    >
      <div className="flex flex-col gap-[1rem]">
        <Stepper label="price" group="alert-step" onStep={step}>
          <div className="flex flex-col items-center">
            <Num kind="price" value={target} className="text-display font-semibold" />
            <span className="text-small text-secondary tabular-nums">{`${pct >= 0 ? "+" : "\u2212"}${numText("amount", Math.abs(pct))}% from now`}</span>
          </div>
        </Stepper>
        <div className="flex gap-[0.4rem]" data-nav-group="alert-presets">
          {PRESETS.map((p) => {
            const t = round(priceUsd * (1 + p / 100));
            return (
              <Chip key={p} on={t === target} onClick={() => setTarget(t)}>
                {p > 0 ? `+${p}%` : `\u2212${-p}%`}
              </Chip>
            );
          })}
        </div>
        <div>
          <Line label="Price now">{usdPrice(priceUsd)}</Line>
          <Line label="Alert when" strong>{`${symbol} goes ${dir} ${usdPrice(target)}`}</Line>
        </div>
        {error && <p className="text-small text-negative">{error}</p>}
      </div>
    </Sheet>
  );
}

/** Four significant digits: enough for any price, never a float tail. */
function round(v: number): number {
  if (!(v > 0)) return 0;
  return Number(v.toPrecision(4));
}
