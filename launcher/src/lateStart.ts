// Starts everything Home does not need for its first frame. App loads this once the launcher is idle
// after the first paint, so none of it (wallet, Sui client, notices, share, trophies, streaming,
// friends, overlay session) is fetched, parsed or run before the window shows.
import { isTauri, onWalletConnected } from "./lib/bridge";
import { setLiveAccount } from "./lib/live";
import { useApp } from "./lib/store";
import { runDebugNav } from "./nav/debug";
import { startJourneyProbe } from "./nav/journey";
import { startNotify } from "./notify";
import { startOverlay } from "./overlay/session";
import { startLocalApi } from "./localapi/host";
import { rarelingsOn } from "./perks/config";
import { usePerks } from "./perks/store";
import { shareDevState, startShare } from "./share";
import { startStream } from "./stream";
import { currentAddress, startSuiLive } from "./sui";
import { startTraderFeed } from "./trade/traders/feed";
import { startTrophies } from "./trophies";
import { startFriends } from "./friends";
import { keystore, watch } from "./wallet";

export function startLate(params: URLSearchParams) {
  startOverlay();
  // Games calling Rare (Local API, rare:platform) and rare:// links.
  startLocalApi();
  void onWalletConnected((game) => useApp.getState().toast(`${game || "A game"} connected to Rare Wallet`));
    void runDebugNav();
    startJourneyProbe();
    startSuiLive();
    // rare.market notices for every Rare Wallet account: toasts, the centre, desktop notifications.
    startNotify();
    startShare();
    startTrophies();
    // PC streaming: paired PCs and whether they answer, for Home and the Library.
    startStream();
    // Friends, presence and parties for the selected Rare Wallet account (needs startNotify first).
    startFriends();
    // The account whose value shows on Home and in the top bar. ?watch=0x... adds a watch account (dev and screenshots).
    void (async () => {
      const w = params.get("watch");
      if (w && !isTauri && !(await keystore.current())) await watch(w, "Demo wallet").catch(() => {});
      const address = (await currentAddress())?.address ?? null;
      setLiveAccount(address);
      // Rareling perks (badge, theme) for this account.
      if (rarelingsOn()) void usePerks.getState().check(address);
    })().catch(() => {});
    // Followed traders' new trades, as events (and toasts until a notifier takes them).
    startTraderFeed(async () => (await currentAddress())?.address ?? null);
    // Dev and screenshots: ?sui=profile or ?sui=saves opens that page.
    const sui = params.get("sui");
    if (sui === "profile" || sui === "saves") setTimeout(() => useApp.getState().open({ kind: sui }), 300);
    // Dev and screenshots: ?app=netflix opens that app's page, ?guidehold the running list.
    const appPage = params.get("app");
    if (appPage) setTimeout(() => useApp.getState().open({ kind: "app", id: appPage }), 300);
    if (params.has("guidehold")) setTimeout(() => useApp.getState().open({ kind: "running" }), 300);
    // ?streamsetup (or ?streampair) opens PC streaming setup, ?streamapp=<id> a PC game's page.
    if (params.has("streamsetup") || params.has("streampair")) setTimeout(() => useApp.getState().open({ kind: "stream" }), 300);
    const streamApp = params.get("streamapp");
    if (streamApp) setTimeout(() => useApp.getState().open({ kind: "streamapp", host: params.get("streamhost") ?? "5F6A2B10-71C3-4E7D-9A55-0B1C2D3E4F50", app: Number(streamApp) }), 300);
    // ?share=grid|detail|clip|uploading|done opens Share in that state.
    const share = params.get("share");
    if (share !== null) setTimeout(() => shareDevState(share), 300);
    // ?trophies opens the trophy case, ?trophies=<appid> one game's trophies.
    const tro = params.get("trophies");
    if (tro != null) setTimeout(() => useApp.getState().open({ kind: "trophies", appid: Number(tro) || undefined }), 300);
}

