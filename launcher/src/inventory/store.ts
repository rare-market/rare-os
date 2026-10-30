// Inventory state. `base` is what rare.market last said; `pending` is what the player just did.
// The screen shows applyPending(base, pending). A pending operation ends when a fetch shows it
// (the market indexer caught up), after 90 seconds, or at once if the action fails (rollback).
// Fetches are triggered by the notify stream (sold, bought, nft_received, offer_received) and,
// after an own action, on a short schedule, since listing and sending make no notice.

import { create } from "zustand";
import { kept, persist } from "../lib/state";
import { useNotify } from "../notify/store";
import type { Notice } from "../notify/types";
import { liveGet, loadInventory, type RareGet } from "./api";
import { applyPending, MOVES, settle } from "./logic";
import type { Inventory, InvOp, Pending } from "./types";

const EMPTY: Inventory = { groups: [], total: 0 };

interface State {
  address: string | null;
  base: Inventory | null;
  pending: Pending[];
  loading: boolean;
  error: boolean;
  /** Item id whose sheet is open (the sheet host lives in the wallet screen). */
  sheetId: string | null;
}

export const useInventory = create<State>(() => ({ address: null, base: null, pending: [], loading: false, error: false, sheetId: null }));

let getter: RareGet = liveGet;
/** Tests and mock mode swap the data source. */
export function setInventorySource(g: RareGet) {
  getter = g;
}

let seq = 0;
let inflight = 0;
const timers = new Set<ReturnType<typeof setTimeout>>();

export function currentView(): Inventory {
  const s = useInventory.getState();
  return s.base ? applyPending(s.base, s.pending) : EMPTY;
}

/** Fetches now. A newer fetch wins; a failure keeps what is on screen. */
export async function refreshInventory(address?: string): Promise<void> {
  const st = useInventory.getState();
  const addr = address ?? st.address;
  if (!addr) return;
  // Another account: its last inventory from Rust's cache at once (`ui:inv:<address>`), then the fetch.
  if (addr !== st.address) useInventory.setState({ address: addr, base: kept<Inventory>(`ui:inv:${addr}`) ?? null, pending: [] });
  const mine = ++inflight;
  useInventory.setState({ loading: true });
  try {
    const inv = await loadInventory(getter, addr);
    if (mine !== inflight) return;
    useInventory.setState((s) => ({ base: inv, error: false, pending: settle(s.pending, inv, Date.now()) }));
    persist(`ui:inv:${addr}`, inv);
  } catch {
    if (mine === inflight) useInventory.setState({ error: true });
  } finally {
    if (mine === inflight) useInventory.setState({ loading: false });
  }
}

/** After an own action: look again at 3, 8, 20 and 45 seconds, until nothing is pending. */
function followUp() {
  for (const ms of [3000, 8000, 20_000, 45_000]) {
    const t = setTimeout(() => {
      timers.delete(t);
      if (useInventory.getState().pending.length) void refreshInventory();
    }, ms);
    timers.add(t);
  }
}

export function stopFollowUps() {
  for (const t of timers) clearTimeout(t);
  timers.clear();
}

/**
 * Applies `op` on screen at once, then runs the action. If the action throws the operation is
 * rolled back and the error goes on to the caller; if it succeeds the operation stays until the
 * indexer shows it.
 */
export async function runOptimistic<T>(op: InvOp, action: () => Promise<T>): Promise<T> {
  const p: Pending = { key: `p${++seq}`, op, at: Date.now() };
  useInventory.setState((s) => ({ pending: [...s.pending, p] }));
  try {
    const r = await action();
    p.at = Date.now();
    followUp();
    return r;
  } catch (e) {
    useInventory.setState((s) => ({ pending: s.pending.filter((x) => x.key !== p.key) }));
    throw e;
  }
}

/** The data source in use (rare.market, or the fixtures in mock mode). */
export const source = () => getter;

export const openItemSheet = (id: string | null) => useInventory.setState({ sheetId: id });

// ---------- the notify stream ----------

let debounce: ReturnType<typeof setTimeout> | null = null;
export function onNotices(list: Notice[], seen: Set<string>) {
  let moved = false;
  for (const n of list) {
    if (seen.has(n.id)) continue;
    seen.add(n.id);
    if (MOVES.has(n.kind)) moved = true;
  }
  if (!moved || !useInventory.getState().address) return;
  if (debounce) clearTimeout(debounce);
  debounce = setTimeout(() => void refreshInventory(), 1200);
}

let started = false;
/** Called once; the first batch of notices is the backlog and is not acted on. */
export function startInventorySync() {
  if (started) return;
  started = true;
  const seen = new Set<string>(useNotify.getState().notices.map((n) => n.id));
  useNotify.subscribe((s) => onNotices(s.notices, seen));
}

/** Items the player just sent: gone from the screen now, confirmed by the next fetches. */
export function noteSentItems(ids: string[]) {
  const p: Pending = { key: `p${++seq}`, op: { kind: "remove", ids, why: "sent" }, at: Date.now() };
  useInventory.setState((s) => ({ pending: [...s.pending, p] }));
  followUp();
}
