// What A does on a notice: open the item or collection in Marketplace, the position or coin in
// Trade, or the transaction on SuiVision through the external-open bridge.

import { openSettingsSection } from "../screens/settings/jump";
import { useApp } from "../lib/store";
import { native } from "../lib/bridge";
import { openInMarket } from "../marketplace/nav";
import { openInTrade } from "../trade/nav";
import { openFriends } from "../friends/nav";
import { openOsRestart } from "../lib/osupdate";
import { acceptFromNotice, isOfferNotice } from "../inventory";
import type { Notice, NoticeAction } from "./types";

export function actionLabel(a: NoticeAction | null | undefined, n?: Notice | null): string {
  if (isOfferNotice(n)) return "Accept offer";
  switch (a?.kind) {
    case "item":
      return "Open item";
    case "collection":
      return "Open collection";
    case "position":
      return "Open position";
    case "coin":
      return "Open coin";
    case "tx":
      return "View transaction";
    case "trader":
      return "Open trader";
    case "friends":
      return "Open friends";
    case "party":
      return "Open party";
    case "os_update":
      return "Restart now";
    case "downloads":
      return "Open library";
    case "emulators":
      return "Open emulators";
    case "trophies":
      return "Open trophies";
    case "battery":
      return "Open power";
    default:
      return "Open";
  }
}

export const txUrl = (digest: string) => `https://suivision.xyz/txblock/${digest}`;

export function runAction(a: NoticeAction | null | undefined, digest?: string | null, n?: Notice | null) {
  if (isOfferNotice(n)) return void acceptFromNotice(n);
  switch (a?.kind) {
    case "item":
      return openInMarket({ kind: "item", id: a.itemId });
    case "collection":
      return openInMarket({ kind: "collection", slug: a.slug });
    case "position":
      return openInTrade({ kind: "perp", marketId: a.marketId });
    case "coin":
      return openInTrade({ kind: "token", coinType: a.coinType });
    case "tx":
      return void native.openWeb(txUrl(a.digest)).catch(() => {});
    case "trader":
      return openInTrade({ kind: "trader", address: a.address });
    case "friends":
      return openFriends(a.address ? { kind: "friend", address: a.address } : null);
    case "party":
      return openFriends({ kind: "party", id: a.partyId });
    case "os_update":
      return openOsRestart();
    case "downloads":
      return void useApp.getState().setTab("library");
    case "emulators":
      return openSettingsSection("emulators");
    case "trophies":
      return;
    case "battery":
      return useApp.getState().open({ kind: "qam" });
    default:
      if (digest) void native.openWeb(txUrl(digest)).catch(() => {});
  }
}
