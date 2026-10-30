// Errors reach the player as one plain sentence: what happened and the next step. The raw error goes
// to the log (and the console in the browser), never to a toast or a sheet.

import { native } from "./bridge";

/** Logs the raw error and returns the sentence to show, "Couldn't ... . Try again." */
export function failed(what: string, e: unknown, next = "Try again."): string {
  const raw = e instanceof Error ? e.message : typeof e === "string" ? e : JSON.stringify(e) ?? String(e);
  void native.debugLog(`error: ${what}: ${raw}`).catch(() => {});
  return next ? `${what} ${next}` : what;
}
