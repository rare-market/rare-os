// The one Local API sheet on screen (sell, buy or claim), and the overlay tab a `rare://overlay?tab=`
// link asked for. GameSheets renders the sheet; when the player is done it answers the game that
// asked (reqId) and, if the overlay came up only for this sheet, hands the screen back to the game.

import { create } from "zustand";
import { closeOverlay } from "../overlay/session";
import type { BuySheetData, ClaimSheetData, SellSheet } from "./methods";

interface Common {
  /** The Local API request to answer, or null for a sheet a link opened. */
  reqId: number | null;
  /** The overlay came up for this sheet: close it when the sheet is done. */
  closeAfter: boolean;
}

export type ActiveSheet = (Common & { kind: "sell"; data: SellSheet }) | (Common & { kind: "buy"; data: BuySheetData }) | (Common & { kind: "claim"; data: ClaimSheetData });

export type OverlayTabId = "items" | "market" | "trophies" | "news";

export const useGameSheet = create<{ sheet: ActiveSheet | null; overlayTab: OverlayTabId | null }>(() => ({ sheet: null, overlayTab: null }));

type Replier = (reqId: number, ok: boolean, value: unknown) => void;
let replier: Replier = () => {};

export function setReplier(f: Replier) {
  replier = f;
}

/** Shows a sheet; false when one is already up (the caller answers "busy"). */
export function showSheet(s: ActiveSheet): boolean {
  if (useGameSheet.getState().sheet) return false;
  useGameSheet.setState({ sheet: s });
  return true;
}

/** The player finished: answers the game with `result` and gives the screen back when it was the game's. */
export function finishSheet(result: unknown) {
  const s = useGameSheet.getState().sheet;
  if (!s) return;
  useGameSheet.setState({ sheet: null });
  if (s.reqId !== null) replier(s.reqId, true, result);
  if (s.closeAfter) void closeOverlay();
}

/** Rust gave up on a request (10-minute limit): the sheet goes without an answer. */
export function dropSheet(reqId: number) {
  const s = useGameSheet.getState().sheet;
  if (!s || s.reqId !== reqId) return;
  useGameSheet.setState({ sheet: null });
  if (s.closeAfter) void closeOverlay();
}

/** The tab a link asked the overlay to open on (once). */
export function takeOverlayTab(): OverlayTabId | null {
  const t = useGameSheet.getState().overlayTab;
  if (t) useGameSheet.setState({ overlayTab: null });
  return t;
}
