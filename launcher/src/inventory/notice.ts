// Accept an offer straight from its notification. The notice carries the item and the offer's
// price, so: load the item, take the best fillable offer, and if it is still the one the notice
// announced, sell into it through the wallet's review sheet. If a better offer exists now, or
// this one is gone, nothing is signed and the item sheet opens with what is best right now.

import { useApp } from "../lib/store";
import { paths } from "../marketplace/api";
import { fmtSui } from "../marketplace/format";
import type { Notice } from "../notify/types";
import { SignRejected } from "../wallet/sign";
import { currentSigningAccount } from "../wallet/market";
import { openItemSheet, runOptimistic, source } from "./store";
import { tradeApi } from "./trade";
import type { InvItem } from "./types";

export type AcceptResult = "sold" | "cancelled" | "sheet" | "failed";

export const isOfferNotice = (n: Notice | null | undefined): n is Notice & { item: NonNullable<Notice["item"]> } => n?.kind === "offer_received" && !!n.item?.id;

/** Opens the inventory's item sheet from anywhere (a notice, a search). */
export function showItem(id: string) {
  openItemSheet(id);
  useApp.getState().setTab("wallet");
}

export async function acceptFromNotice(n: Notice): Promise<AcceptResult> {
  if (!isOfferNotice(n)) return "failed";
  const toast = useApp.getState().toast;
  const id = n.item.id;
  const account = await currentSigningAccount();
  if (!account) {
    toast("Add an account that can sign to accept offers");
    showItem(id);
    return "sheet";
  }
  try {
    const item = await source()<InvItem>(paths.item(id));
    const api = tradeApi();
    const q = await api.quote(item, account.address);
    if (!q) {
      toast("That offer is no longer open");
      showItem(id);
      return "sheet";
    }
    const announced = n.amount?.raw;
    if (q.blocked || (announced && BigInt(announced) !== BigInt(q.offer.priceMist))) {
      toast(q.blocked ? q.blocked : "A better offer is open now. Check it before selling");
      showItem(id);
      return "sheet";
    }
    await runOptimistic({ kind: "remove", ids: [id], why: "sold" }, () => api.sell(item, q.offer, account.address));
    toast(`Sold ${item.name} for ${fmtSui(q.netMist, { dp: 3, trim: true })} SUI`);
    return "sold";
  } catch (e) {
    if (e instanceof SignRejected) return "cancelled";
    toast(/failed to fetch|rare\.market \d+/i.test((e as Error)?.message ?? "") ? "Couldn't reach rare.market" : (e as Error)?.message || "Couldn't accept the offer");
    return "failed";
  }
}

