// Plain-words PIN errors for the unlock sheet.

const MAX_TRIES = 5;

export function wrongMessage(failures: unknown, what = "PIN"): string {
  const n = typeof failures === "number" ? failures : 0;
  const left = Math.max(1, MAX_TRIES - n);
  return `Wrong ${what}. ${left} ${left === 1 ? "try" : "tries"} before a wait.`;
}

export function waitText(ms: number): string {
  const s = Math.max(1, Math.ceil(ms / 1000));
  return s >= 60 ? `${Math.ceil(s / 60)} min` : `${s} s`;
}

