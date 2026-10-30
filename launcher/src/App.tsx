import { useDeferredValue, useEffect, useRef } from "react";
import { DockWatch } from "./components/DockWatch";
import { OsUpdateNotice } from "./notify/OsUpdateNotice";
import { BatteryNotice } from "./notify/BatteryNotice";
import { HintBar, LoadBar, Toasts, TopBar } from "./components/Chrome";
import { Backdrop } from "./components/Backdrop";
import { startIdleDim } from "./lib/idleDim";
import { pauseLive, startLive } from "./lib/live";
import { gameTookScreen, poll, startActivity } from "./lib/idle";
import { startSleep } from "./lib/sleep";
import { Osk, askText } from "./components/Osk";
import { native } from "./lib/bridge";
import { TABS, useApp } from "./lib/store";
import { feedback, setFeedback } from "./nav/feedback";
import { watchFocus } from "./nav/focus";
import type { Hint } from "./nav/hints";
import { startInput, usePadHandler } from "./nav/input";
import { Home } from "./screens/Home";
import { lazy } from "react";
import { Lazy, lazyNamed, preloadWhenIdle } from "./lib/lazy";
import { Boot } from "./components/Boot";
import { SteamHandoff } from "./components/SteamHandoff";
import { LaunchFail } from "./components/LaunchFail";
import { cancellable, startProgress, useStart } from "./lib/progress";
import { SHOW_STORE } from "./store/show";
import { useMarketHints } from "./marketplace/nav";
import { useWalletHints } from "./wallet/ui/hints";
import { setArea, useMarketArea } from "./market/area";
import { useTradeHints } from "./trade/nav";
import { useGameSheet } from "./localapi/sheets";
import { OptionsHost, focusedGame, gameOptions, openOptionsSheet } from "./components/Options";

/** Quick Settings was opened from first run: closing it reopens setup. */
let fromSetup = false;
useApp.subscribe((s, prev) => {
  if (fromSetup && prev.overlay?.kind === "qam" && !s.overlay) {
    fromSetup = false;
    s.open({ kind: "firstrun" });
  }
});

export function App() {
  const tab = useApp((s) => s.tab);
  // The screen on show. A tab change renders the new screen in the background and keeps the old one
  // up until it is ready (its chunk loaded, its first render done), so a switch never shows an empty
  // page: the Suspense boundary sits above the keyed view, and a deferred render that suspends keeps
  // the previous screen instead of showing the fallback.
  const shown = useDeferredValue(tab);
  const overlay = useApp((s) => s.overlay);
  const scanning = useApp((s) => s.scanning);
  // A sell, buy or claim sheet a game or a rare:// link opened (src/localapi).
  const gameSheet = useGameSheet((s) => !!s.sheet);
  const main = useRef<HTMLDivElement>(null);

  useEffect(() => {
    startActivity();
    startInput();
    startProgress();
    watchFocus();
    void useApp.getState().refreshDevice();
    // The last scan from disk fills Home at once; the fresh scan (it walks the Steam and app folders)
    // starts after the first frame has been drawn.
    void native.cachedLibrary().then((c) => {
      if (c && !useApp.getState().library) useApp.setState({ library: c });
    }).catch(() => {});
    const scanId = requestAnimationFrame(() => setTimeout(() => void useApp.getState().scan(), 0));
    // Battery and Wi-Fi in the top bar: every 30 s in use, every 2 minutes idle, none while away.
    const stopDevice = poll(() => useApp.getState().refreshDevice(), { active: 30_000, idle: 120_000, away: null });
    const params = new URLSearchParams(location.search);
    const t = params.get("tab");
    // Old deep links keep working: ?tab=trade is the Market tab's Trade area; ?tr= and ?mk= pick the area too.
    if (t === "trade" || params.has("tr")) setArea("trade");
    if (t === "trade") useApp.getState().setTab("market");
    else if (t && (TABS.some((x) => x.id === t) || t === "friends" || t === "settings")) useApp.getState().setTab(t as never);
    else if (params.has("tr")) useApp.getState().setTab("market");
    if (params.has("osk")) setTimeout(() => void askText("Sui address or SuiNS name to watch", "rare.sui", { placeholder: "0x... or name.sui" }), 600);
    void native.config().then((c) => {
      setFeedback({ sounds: c.uiSounds ?? true, rumble: c.rumble ?? true });
      if ((!c.onboarded && !params.has("nosignin")) || params.has("firstrun")) useApp.getState().open({ kind: "firstrun" });
      else if (params.has("signin")) useApp.getState().open({ kind: "signin" });
    });
    startLive();
    const stopIdle = startIdleDim();
    startSleep();
    // Everything else (wallet, Sui, notices, share, trophies, streaming, friends, overlay) starts after
    // the first frame, so Home is on screen before any of it is parsed.
    const idle = (window as { requestIdleCallback?: (f: () => void, o?: { timeout: number }) => number }).requestIdleCallback;
    const later = () => {
      void import("./lateStart").then((m) => m.startLate(params));
      // Then every screen's chunk, one per idle slot, so opening one never waits on a chunk.
      setTimeout(() => preloadWhenIdle(PRELOAD), 1500);
    };
    const lateId = idle ? idle(later, { timeout: 1500 }) : window.setTimeout(later, 300);
    // Sounds for what the whole shell does: a game starting, errors. A and B play their own.
    let launchedAt = 0;
    const unsub = useApp.subscribe((s, prev) => {
      if (s.launching && s.launching !== prev.launching) feedback("launch");
      if (s.toasts.length > prev.toasts.length && s.toasts[s.toasts.length - 1].tone === "error") feedback("error");
      if (s.running.length !== prev.running.length) pauseLive(s.running.length > 0);
      // A game started from here has the screen: stop drawing until we are back in front.
      if (s.launching && s.launching !== prev.launching) launchedAt = Date.now();
      if (s.running.length > prev.running.length && Date.now() - launchedAt < 8_000) gameTookScreen();
    });
    // Coming back from a game or Steam: pick up installs, playtime and new shortcuts.
    const onFocus = () => void useApp.getState().scan({ quiet: true });
    window.addEventListener("focus", onFocus);
    return () => {
      if (idle) (window as { cancelIdleCallback?: (n: number) => void }).cancelIdleCallback?.(lateId);
      else clearTimeout(lateId);
      cancelAnimationFrame(scanId);
      stopDevice();
      unsub();
      stopIdle();
      window.removeEventListener("focus", onFocus);
    };
  }, []);

  usePadHandler((b) => {
    const s = useApp.getState();
    switch (b) {
      case "lb":
      case "rb": {
        // LB/RB are always the launcher's tabs. A page or sheet over the tab closes first; only the
        // first run and sign-in keep them.
        if (s.overlay?.kind === "firstrun" || s.overlay?.kind === "signin") return true;
        if (s.overlay) s.close();
        const i = TABS.findIndex((t) => t.id === s.tab);
        // From Friends or Settings (not in the bar) the bumpers go to the first or last tab.
        const j = i < 0 ? (b === "rb" ? 0 : TABS.length - 1) : (i + (b === "rb" ? 1 : TABS.length - 1)) % TABS.length;
        s.setTab(TABS[j].id);
        feedback("tab");
        return true;
      }
      case "b": {
        // A start still going after 15 s with no window: B cancels it (lib/progress.ts).
        const slow = s.overlay ? null : cancellable(useStart.getState().starts, Date.now());
        if (slow) {
          void native.quit(slow).catch(() => {});
          s.toast("Cancelled");
          feedback("back");
          return true;
        }
        if (s.overlay?.kind === "signin") void native.config().then((c) => native.setConfig({ ...c, onboarded: true }));
        if (s.back()) feedback("back");
        return true;
      }
      case "qam":
        // Setup: Quick opens Quick Settings (Wi-Fi, brightness, volume are needed); closing it
        // returns to setup where it left off.
        if (s.overlay?.kind === "firstrun") fromSetup = true;
        feedback(s.overlay?.kind === "qam" ? "close" : "open");
        s.open(s.overlay?.kind === "qam" ? null : { kind: "qam" });
        return true;
      case "guide":
        // The Rare button with the launcher in front (over a game the native side opens the menu
        // itself): the Rare menu; pressed again, Home. Nothing during setup.
        if (s.overlay?.kind === "firstrun" || s.overlay?.kind === "signin") return true;
        if (s.overlay?.kind === "raremenu") {
          s.close();
          s.setTab("home");
          feedback("back");
          return true;
        }
        s.close();
        s.open({ kind: "raremenu" });
        return true;
      case "guidehold":
      case "exithold":
        // The Rare button held, or View + Menu: the Switcher.
        if (s.overlay?.kind === "firstrun" || s.overlay?.kind === "signin") return true;
        if (s.overlay?.kind === "running") return true;
        s.close();
        s.open({ kind: "running" });
        return true;
      case "start": {
        // Menu: Options for the focused item. Only on the launcher's screens and a game's page.
        if (s.overlay && s.overlay.kind !== "game") return false;
        const g = s.overlay?.kind === "game" ? s.library?.games.find((x) => x.id === (s.overlay as { id: string }).id) ?? null : focusedGame();
        if (!g) return false;
        openOptionsSheet(gameOptions(g));
        return true;
      }
      case "y":
        if (s.overlay) return false;
        feedback("open");
        s.open({ kind: "search" });
        return true;
      default:
        return false;
    }
  });

  const marketHints = useMarketHints((s) => s.hints);
  const tradeHints = useTradeHints((s) => s.sheet ?? s.hints);
  const marketArea = useMarketArea((s) => s.area);
  const history = useApp((s) => s.history);
  const canBack = !!overlay || history.length > 0 || tab !== "home";
  // Screen-level extras: the hint stack fills in X, Y and the rest.
  const base: Hint[] = overlay
    ? overlay.kind === "firstrun" || overlay.kind === "signin"
      ? []
      : [{ b: "lbrb", label: "Tabs" }]
    : [
        { b: "y", label: "Search" },
        { b: "lbrb", label: "Tabs" },
        { b: "quick", label: "Quick settings" },
      ];
  const walletHints = useWalletHints((s) => s.hints);
  const screenHints = overlay ? null : shown === "market" ? (marketArea === "trade" ? tradeHints : marketHints) : shown === "wallet" ? walletHints : null;
  // LB/RB (the launcher's tabs) show on every screen, also where a screen brings its own hints.
  const TABS_HINT: Hint = { b: "lbrb", label: "Tabs" };
  const hints = screenHints && !screenHints.some((h) => h.b === "lbrb") ? [...screenHints, TABS_HINT] : screenHints;

  return (
    <div className="relative flex h-full flex-col">
      {shown === "home" && <Backdrop />}
      <TopBar />
      <LoadBar on={scanning} />
      <main ref={main} className="relative min-h-0 flex-1" inert={overlay ? true : undefined}>
        <Lazy>
          <div key={shown} className="fade-in h-full">
            {shown === "home" && <Home />}
            {shown === "library" && <Library />}
            {shown === "games" && <SuiGames />}
            {SHOW_STORE && shown === "store" && <Store />}
            {shown === "wallet" && <Wallet />}
            {shown === "market" && <Market />}
            {shown === "friends" && <Friends />}
            {shown === "settings" && <Settings />}
          </div>
        </Lazy>
      </main>
      <HintBar override={hints} base={base} canBack={canBack} />
      <Lazy>
      {overlay?.kind === "game" && <GameDetail id={overlay.id} />}
      {overlay?.kind === "suigame" && <SuiGameDetail slug={overlay.slug} />}
      {SHOW_STORE && overlay?.kind === "storegame" && <StoreGamePage key={overlay.slug} slug={overlay.slug} />}
      {overlay?.kind === "signin" && <SignIn />}
      {overlay?.kind === "firstrun" && <FirstRun />}
      {overlay?.kind === "search" && <Search />}
      {overlay?.kind === "qam" && <QuickSettings />}
      {overlay?.kind === "profile" && <ProfilePage />}
      {overlay?.kind === "saves" && <CloudSaves />}
      {overlay?.kind === "app" && <AppSheet id={overlay.id} />}
      {overlay?.kind === "running" && <LauncherSwitcher />}
      {overlay?.kind === "raremenu" && <LauncherMenu />}
      {overlay?.kind === "stream" && <StreamSetup />}
      {overlay?.kind === "streamapp" && <StreamAppSheet key={`${overlay.host}:${overlay.app}`} uuid={overlay.host} appId={overlay.app} />}
      {overlay?.kind === "share" && <ShareScreen />}
      {overlay?.kind === "trophies" && <TrophyCase key={`${overlay.appid ?? ""}`} appid={overlay.appid} from={overlay.from ?? undefined} />}
      <InGameOverlay />
      {gameSheet && <GameSheets />}
      </Lazy>
      <Lazy>
        <WalletHost />
      </Lazy>
      <OptionsHost />
      <Osk />
      <SteamHandoff />
      {overlay?.kind === "launchfail" && <LaunchFail id={overlay.id} reason={overlay.reason} code={overlay.code} />}
      <Toasts />
      <DockWatch />
      <OsUpdateNotice />
      <BatteryNotice />
      <Boot />
    </div>
  );
}

// Everything but Home loads on first use, and is fetched while the launcher is idle after the first frame.
const GameDetail = lazyNamed(() => import("./screens/GameDetail"), "GameDetail");
const Library = lazyNamed(() => import("./screens/Library"), "Library");
const Settings = lazyNamed(() => import("./screens/Settings"), "Settings");
const SignIn = lazyNamed(() => import("./screens/SignIn"), "SignIn");
// The first-run steps (Wi-Fi, move assets, Steam, passkey) register themselves into FirstRun when loaded.
const FirstRun = lazy(async () => {
  await Promise.all([import("./onboarding"), import("./wallet/ui/onboard/firstrun")]);
  return { default: (await import("./screens/FirstRun")).FirstRun };
});
const WalletHost = lazyNamed(() => import("./wallet/ui/WalletHost"), "WalletHost");
const SuiGames = lazyNamed(() => import("./screens/SuiGames"), "SuiGames");
const SuiGameDetail = lazyNamed(() => import("./screens/SuiGames"), "SuiGameDetail");
const Wallet = lazyNamed(() => import("./screens/Wallet"), "Wallet");
const Market = lazyNamed(() => import("./market/Market"), "Market");
const Search = lazyNamed(() => import("./components/Search"), "Search");
const QuickSettings = lazyNamed(() => import("./components/QuickSettings"), "QuickSettings");
const Store = lazyNamed(() => import("./store/Store"), "Store");
const StoreGamePage = lazyNamed(() => import("./store/GamePage"), "StoreGamePage");
const ProfilePage = lazyNamed(() => import("./sui/ui/ProfilePage"), "ProfilePage");
const CloudSaves = lazyNamed(() => import("./sui/ui/CloudSaves"), "CloudSaves");
const AppSheet = lazyNamed(() => import("./apps/sheets"), "AppSheet");
const LauncherSwitcher = lazyNamed(() => import("./overlay/Overlay"), "LauncherSwitcher");
const LauncherMenu = lazyNamed(() => import("./overlay/Overlay"), "LauncherMenu");
const StreamSetup = lazyNamed(() => import("./stream/sheets"), "StreamSetup");
const StreamAppSheet = lazyNamed(() => import("./stream/sheets"), "StreamAppSheet");
const ShareScreen = lazyNamed(() => import("./share/ShareScreen"), "ShareScreen");
const TrophyCase = lazyNamed(() => import("./trophies/ui/TrophyCase"), "TrophyCase");
const InGameOverlay = lazyNamed(() => import("./overlay/Overlay"), "InGameOverlay");
const GameSheets = lazyNamed(() => import("./localapi/GameSheets"), "GameSheets");
const Friends = lazyNamed(() => import("./friends/ui/FriendsScreen"), "Friends");

// Idle preload order: what people open first.
const PRELOAD = [
  Library,
  GameDetail,
  QuickSettings,
  Search,
  SuiGames,
  SuiGameDetail,
  AppSheet,
  LauncherSwitcher,
  Settings,
  Wallet,
  Market,
  Friends,
  InGameOverlay,
  ...(SHOW_STORE ? [Store, StoreGamePage] : []),
];
