// The move plan for a Playtron (GameOS) wallet: what to sign, in which order.
//
// Order: kiosks first, then staked SUI, SuiNS names,
// collectibles, and coins last so there is gas for every earlier step.
//   - A normal kiosk moves whole: sending its KioskOwnerCap moves every item (listed, locked or
//     not) and its sale proceeds.
//   - A personal kiosk can't change owner, so items leave one by one: delist, take, send.
//     Locked items there can only leave through the collection's own rules, so they stay.
//   - Staked SUI moves as it is and keeps earning; SuiNS registrations move as objects.
//   - With own gas, the last transaction sends the gas coin itself: every SUI that's left after
//     the fee. With sponsored gas, SUI is sent like any other coin.
//
// buildBatch returns an unsigned Transaction with the Playtron address as sender. It is
// signed on Playtron's own surface; Rare never sees Playtron credentials.

import { Transaction } from "@mysten/sui/transactions";
import { normalizeSuiAddress } from "@mysten/sui/utils";
import { SUI_TYPE, stuckReason } from "./scan";
import type { GasMode, MoveBatch, MoveLine, MovePlan, RescueCoin, RescueKiosk, RescueScan, StuckLine } from "./types";

const TWO = "0x2";
/** objects per transaction; well under the 2048-input and 128 KB limits */
const PER_TX = 200;
/** coin objects per transaction (inputs cap at 2048; gas coins come on top) */
const MAX_COIN_OBJECTS = 1000;
/** arguments per TransferObjects command (cap 512) */
const MAX_ARGS = 400;
/** SUI kept in mind for fees per transaction (MIST), for the low-gas warning */
export const FEE_PER_TX = 20_000_000n;

/** How many of the largest SUI coins to leave out of explicit transfers so they can pay gas. */
function keepForGas(bals: bigint[] | undefined, n: number): number {
  if (n === 0) return 0;
  if (!bals || bals.length !== n) return 1;
  let sum = 0n;
  for (let i = 0; i < n && i < 50; i++) {
    sum += bals[i];
    if (sum >= FEE_PER_TX * 4n) return i + 1;
  }
  return Math.min(n, 50);
}

export function formatAmount(raw: bigint, decimals: number, max = 4): string {
  const neg = raw < 0n;
  const v = neg ? -raw : raw;
  const base = 10n ** BigInt(decimals);
  const whole = v / base;
  let frac = (v % base).toString().padStart(decimals, "0").slice(0, max).replace(/0+$/, "");
  if (!frac && whole === 0n && v > 0n) frac = "0".repeat(Math.max(0, max - 1)) + "1";
  const w = whole.toLocaleString("en-US");
  return `${neg ? "-" : ""}${w}${frac ? `.${frac}` : ""}`;
}

const sui = (mist: bigint) => `${formatAmount(mist, 9)} SUI`;

type Step =
  | { kind: "kiosk-cap"; kiosk: RescueKiosk }
  | { kind: "kiosk-item"; kiosk: RescueKiosk; itemId: string; type: string; listed: boolean }
  | { kind: "kiosk-profits"; kiosk: RescueKiosk }
  | { kind: "object"; id: string }
  | { kind: "coin"; coinType: string; raw: bigint; addressRaw: bigint; objectIds: string[] }
  | { kind: "gas" };

interface Planned {
  plan: MovePlan;
  steps: Step[][];
}

function chunk<T>(xs: T[], n: number): T[][] {
  const out: T[][] = [];
  for (let i = 0; i < xs.length; i += n) out.push(xs.slice(i, i + n));
  return out;
}

function make(scan: RescueScan, to: string, gas: GasMode): Planned {
  const dest = normalizeSuiAddress(to);
  const groups: Array<{ title: string; lines: MoveLine[]; steps: Step[] }> = [];
  const stuck: StuckLine[] = [];

  // 1. Kiosks: every normal kiosk moves whole, in one transaction (empty ones included, so
  // nothing sold into them later lands in the old wallet).
  const whole = scan.kiosks.filter((k) => !k.personal);
  for (const [i, part] of chunk(whole, PER_TX).entries()) {
    groups.push({
      title: whole.length > PER_TX ? `Kiosks, part ${i + 1}` : whole.length > 1 ? "Kiosks" : "Kiosk",
      lines: part.map((k) => {
        const listed = k.items.filter((it) => it.listed != null).length;
        const parts = [k.items.length ? `${k.items.length} item${k.items.length === 1 ? "" : "s"} inside` : "Empty"];
        if (listed) parts.push(`${listed} listed, still listed after the move`);
        if (k.profits > 0n) parts.push(`${sui(k.profits)} in sales`);
        return { kind: "kiosk" as const, id: k.kioskId, label: "Kiosk and everything in it", detail: parts.join(", ") };
      }),
      steps: part.map((k) => ({ kind: "kiosk-cap" as const, kiosk: k })),
    });
  }
  for (const k of scan.kiosks) {
    if (!k.personal) continue;
    const movable = k.items.filter((i) => !i.locked && i.transferable && !i.exclusive);
    for (const i of k.items) {
      if (i.exclusive) stuck.push({ id: i.objectId, label: i.name, reason: "Listed through a marketplace. Cancel that listing on the marketplace, then scan again." });
      else if (i.locked) stuck.push({ id: i.objectId, label: i.name, reason: "Locked by its collection's rules in a personal kiosk. Sell it, or move it with a marketplace that supports the collection." });
      else if (!i.transferable) stuck.push({ id: i.objectId, label: i.name, reason: stuckReason(i.type) });
    }
    const steps: Step[] = movable.map((i) => ({ kind: "kiosk-item", kiosk: k, itemId: i.objectId, type: i.type, listed: i.listed != null }));
    const lines: MoveLine[] = movable.map((i) => ({
      kind: "kiosk",
      id: i.objectId,
      label: i.name,
      detail: i.listed != null ? `Listed for ${sui(i.listed)}: delisted first` : "From your personal kiosk",
    }));
    if (k.profits > 0n) {
      steps.push({ kind: "kiosk-profits", kiosk: k });
      lines.push({ kind: "coin", id: k.kioskId, label: "Kiosk sales", detail: sui(k.profits) });
    }
    for (const [i, part] of chunk(steps.map((s, j) => [s, lines[j]] as const), PER_TX / 2).entries()) {
      groups.push({ title: i ? "Personal kiosk (continued)" : "Personal kiosk", lines: part.map((p) => p[1]), steps: part.map((p) => p[0]) });
    }
  }

  // 2. Staked SUI
  if (scan.staked.length) {
    for (const part of chunk(scan.staked, PER_TX)) {
      groups.push({
        title: "Staked SUI",
        lines: part.map((s) => ({ kind: "stake", id: s.objectId, label: `${sui(s.principal)} staked`, detail: "Moves as it is and keeps earning" })),
        steps: part.map((s) => ({ kind: "object", id: s.objectId })),
      });
    }
  }

  // 3. SuiNS names
  if (scan.names.length) {
    groups.push({
      title: "SuiNS names",
      lines: scan.names.map((n) => ({
        kind: "name",
        id: n.objectId,
        label: n.domain,
        detail: n.domain === scan.defaultName ? "Points at the old wallet now: point it at your new one after it arrives" : undefined,
      })),
      steps: scan.names.map((n) => ({ kind: "object", id: n.objectId })),
    });
  }

  // 4. Collectibles and other objects
  for (const [i, part] of chunk(scan.nfts, PER_TX).entries()) {
    groups.push({
      title: i ? "Collectibles (continued)" : "Collectibles",
      lines: part.map((n) => ({ kind: "nft", id: n.objectId, label: n.name, detail: n.collection })),
      steps: part.map((n) => ({ kind: "object", id: n.objectId })),
    });
  }

  // 5. Coins, last. Packed so no transaction passes the input and per-command limits.
  const suiCoin = scan.coins.find((c) => c.coinType === SUI_TYPE);
  const portions: Array<{ line: MoveLine; step: Step; size: number }> = [];
  const addCoin = (c: RescueCoin, ids: string[], bals: bigint[] | undefined, withAddress: boolean) => {
    for (const [i, part] of chunk(ids, MAX_COIN_OBJECTS).entries()) {
      const partBals = bals?.slice(i * MAX_COIN_OBJECTS, i * MAX_COIN_OBJECTS + part.length);
      const sum = partBals ? partBals.reduce((a, b) => a + b, 0n) : c.coinRaw;
      const address = withAddress && i === 0 ? c.addressRaw : 0n;
      portions.push({
        line: { kind: "coin", id: c.coinType, label: c.symbol, detail: `${formatAmount(sum + address, c.decimals)} ${c.symbol}` },
        step: { kind: "coin", coinType: c.coinType, raw: sum + address, addressRaw: address, objectIds: part },
        size: part.length + (address > 0n ? 1 : 0),
      });
    }
    if (withAddress && ids.length === 0 && c.addressRaw > 0n) {
      portions.push({
        line: { kind: "coin", id: c.coinType, label: c.symbol, detail: `${formatAmount(c.addressRaw, c.decimals)} ${c.symbol}` },
        step: { kind: "coin", coinType: c.coinType, raw: c.addressRaw, addressRaw: c.addressRaw, objectIds: [] },
        size: 1,
      });
    }
  };
  for (const c of scan.coins) if (c.coinType !== SUI_TYPE) addCoin(c, c.objectIds, c.objectBalances, true);
  let gasLine: MoveLine | null = null;
  if (suiCoin) {
    if (gas === "own") {
      // Keep the largest coins for fees; they leave last as the gas coin itself.
      const keep = keepForGas(suiCoin.objectBalances, suiCoin.objectIds.length);
      addCoin(suiCoin, suiCoin.objectIds.slice(keep), suiCoin.objectBalances?.slice(keep), true);
      if (keep > 0) gasLine = { kind: "coin", id: SUI_TYPE, label: "SUI", detail: `All SUI that's left after fees (about ${sui(suiCoin.raw)} in total)` };
    } else {
      addCoin(suiCoin, suiCoin.objectIds, suiCoin.objectBalances, true);
    }
  }
  const coinGroups: Array<{ lines: MoveLine[]; steps: Step[]; size: number }> = [];
  for (const p of portions) {
    let g = coinGroups[coinGroups.length - 1];
    if (!g || g.size + p.size > MAX_COIN_OBJECTS) coinGroups.push((g = { lines: [], steps: [], size: 0 }));
    g.lines.push(p.line);
    g.steps.push(p.step);
    g.size += p.size;
  }
  if (gasLine) {
    if (!coinGroups.length) coinGroups.push({ lines: [], steps: [], size: 0 });
    const last = coinGroups[coinGroups.length - 1];
    last.lines.push(gasLine);
    last.steps.push({ kind: "gas" });
  }
  for (const [i, g] of coinGroups.entries()) groups.push({ title: coinGroups.length > 1 ? `Coins, part ${i + 1} of ${coinGroups.length}` : "Coins", lines: g.lines, steps: g.steps });

  for (const s of scan.soulbound) stuck.push({ id: s.objectId, label: s.name, reason: stuckReason(s.type) });

  const batches: MoveBatch[] = groups.map((g, i) => ({ index: i + 1, title: g.title, lines: g.lines }));
  let feeNote: string | null = null;
  if (gas === "own" && batches.length) {
    const need = FEE_PER_TX * BigInt(batches.length);
    const have = suiCoin?.raw ?? 0n;
    if (have < need) feeNote = `Each step needs a little SUI for its fee. This wallet has ${sui(have)}; send it about ${sui(need - have)} more first, or use a wallet that pays fees for you.`;
  }
  return { plan: { from: scan.address, to: dest, batches, stuck, empty: batches.length === 0, feeNote }, steps: groups.map((g) => g.steps) };
}

/** What to move and in which order. `gas` is who pays fees: the wallet itself ("own") or a sponsor. */
export function planMoves(scan: RescueScan, to: string, gas: GasMode = "own"): MovePlan {
  return make(scan, to, gas).plan;
}

/** The unsigned transaction for batch `index` (1-based) of `plan`; sender is the Playtron address. */
export function buildBatch(scan: RescueScan, plan: MovePlan, index: number, gas: GasMode = "own"): Transaction {
  const { steps } = make(scan, plan.to, gas);
  const batch = steps[index - 1];
  if (!batch) throw new Error(`No step ${index} in this plan`);
  const tx = new Transaction();
  tx.setSender(scan.address);
  const to = plan.to;
  const send: Parameters<Transaction["transferObjects"]>[0] = [];

  // Personal kiosk items: borrow the owner cap once per kiosk, take everything, return it.
  type KStep = Extract<Step, { kind: "kiosk-item" | "kiosk-profits" }>;
  const byKiosk = new Map<string, KStep[]>();
  for (const s of batch) {
    if (s.kind === "kiosk-item" || s.kind === "kiosk-profits") {
      const list = byKiosk.get(s.kiosk.kioskId) ?? [];
      list.push(s);
      byKiosk.set(s.kiosk.kioskId, list);
    }
  }
  for (const list of byKiosk.values()) {
    const k = list[0].kiosk;
    const pkg = k.personalPackage!;
    const kiosk = tx.object(k.kioskId);
    const pcap = tx.object(k.capId);
    const [cap, borrow] = tx.moveCall({ target: `${pkg}::personal_kiosk::borrow_val`, arguments: [pcap] });
    for (const s of list) {
      if (s.kind === "kiosk-item") {
        if (s.listed) tx.moveCall({ target: `${TWO}::kiosk::delist`, typeArguments: [s.type], arguments: [kiosk, cap, tx.pure.id(s.itemId)] });
        send.push(tx.moveCall({ target: `${TWO}::kiosk::take`, typeArguments: [s.type], arguments: [kiosk, cap, tx.pure.id(s.itemId)] }));
      } else if (s.kind === "kiosk-profits") {
        send.push(tx.moveCall({ target: `${TWO}::kiosk::withdraw`, arguments: [kiosk, cap, tx.pure.option("u64", null)] }));
      }
    }
    tx.moveCall({ target: `${pkg}::personal_kiosk::return_val`, arguments: [pcap, cap, borrow] });
  }

  let gasLast = false;
  for (const s of batch) {
    switch (s.kind) {
      case "kiosk-cap":
        send.push(tx.object(s.kiosk.capId));
        break;
      case "object":
        send.push(tx.object(s.id));
        break;
      case "coin":
        if (s.addressRaw === s.raw && s.objectIds.length === 0) {
          // Only in the address balance: withdraw it into a coin.
          send.push(tx.coin({ type: s.coinType, balance: s.raw, useGasCoin: false }));
        } else if (s.addressRaw > 0n) {
          // Both: coin objects as they are, plus the address balance as a coin.
          for (const id of s.objectIds) send.push(tx.object(id));
          send.push(tx.coin({ type: s.coinType, balance: s.addressRaw, useGasCoin: false }));
        } else {
          for (const id of s.objectIds) send.push(tx.object(id));
        }
        break;
      case "gas":
        gasLast = true;
        break;
    }
  }
  for (const part of chunk(send, MAX_ARGS)) tx.transferObjects(part, to);
  // Last of all: the gas coin itself, so every SUI left after the fee moves too.
  if (gasLast) tx.transferObjects([tx.gas], to);
  return tx;
}
