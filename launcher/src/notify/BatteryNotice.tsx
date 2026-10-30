// Launcher-level watcher for the battery: a toast at 15 % and at 5 % while it discharges, which
// shows over Do not disturb and over a game (notify/logic.ts route). Renders nothing.

import { useEffect, useRef } from "react";
import { useApp } from "../lib/store";
import { lowBattery, lowBatteryText } from "./battery";
import { notifyLocal } from "./stream";

export function BatteryNotice() {
  const battery = useApp((s) => s.device?.battery ?? null);
  const warned = useRef<number | null>(null);
  const percent = battery?.percent;
  const charging = battery?.charging;
  useEffect(() => {
    const r = lowBattery(warned.current, percent == null ? null : { percent, charging: !!charging });
    warned.current = r.warned;
    if (r.warn == null) return;
    notifyLocal({ kind: "battery", title: lowBatteryText(r.warn), action: { kind: "battery" } });
  }, [percent, charging]);
  return null;
}
