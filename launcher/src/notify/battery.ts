// Low-battery warnings: which one to show for a reading.

/** The battery levels that warn, highest first. */
export const LOW_BATTERY = [15, 5] as const;

/**
 * Which low-battery warning to show now. `warned` is the lowest level already warned about
 * since the last charge: a warning shows once per level when the battery is discharging, and
 * charging (or climbing back above the level) re-arms it. Dropping straight to 4 % shows the
 * 5 % warning only.
 */
export function lowBattery(warned: number | null, b: { percent: number; charging: boolean } | null): { warn: number | null; warned: number | null } {
  if (!b) return { warn: null, warned };
  if (b.charging) return { warn: null, warned: null };
  const hit = [...LOW_BATTERY].reverse().find((l) => b.percent <= l) ?? null;
  // Back to 20 % without charging (a reading that jumped) re-arms; 16 % does not, so 15 % does not repeat.
  if (hit == null) return { warn: null, warned: b.percent >= 20 ? null : warned };
  if (warned != null && warned <= hit) return { warn: null, warned };
  return { warn: hit, warned: hit };
}

/** The sentence for a warning. */
export const lowBatteryText = (level: number) => (level <= 5 ? "Battery at 5%. Plug in soon." : "Battery at 15%.");
