// The Rare menu: a right-side panel 32rem wide over a dimmed frame of the game. It opens the same way everywhere: press the Rare button. LT and RT switch tabs,
// B goes back to the game, the Rare button again goes Home (the native side does that one).
//
// Over a game it always opens on Game with Resume focused, so Rare then A is always back to the
// game. Tabs: Game, Items and Market (a Sui game with collections), Trophies (a Steam game, or a
// Sui game), Friends, Notifications. Over the launcher with nothing in front, Game is replaced by
// Rare (Home, profile, Settings, Power, Desktop mode), and the menu opens on Notifications when
// any are unread.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fileSrc } from "../lib/bridge";
import { isGuest } from "../lib/guest";
import { useApp } from "../lib/store";
import { appByRunId } from "../apps/catalog";
import { QuickSettings } from "../components/QuickSettings";
import type { ItemCard } from "../marketplace/api";
import { BuySheet } from "../marketplace/BuySheet";
import { focusFirst } from "../nav/focus";
import { feedback } from "../nav/feedback";
import { usePadHandler } from "../nav/input";
import { useFriends } from "../friends/store";
import { TOAST_MS, useNotify, useUnread } from "../notify/store";
import { useCaseData } from "../trophies/ui/useCase";
import { useLive } from "../lib/live";
import { Glyph, Hint, LoadBar, MImg, StatLine, Sui, Tabs, WalletAvatar } from "../ui";
import { GameTab } from "./GameTab";
import { gameKind, inviteUp, menuTabs, openingTab, type MenuTab } from "./menuModel";
import { FriendsTab, NoticesTab, RareTab, type Where } from "./menuTabs";
import type { OverlayItem } from "./data";
import { resolveGame } from "./game";
import { Frozen, ItemDetail, ItemList, MarketList, Trophies, useAccount, useGames, useOverlayData } from "./panels";
import { closeOverlay, type Session } from "./session";
import { takeOverlayTab } from "../localapi/sheets";
import { AddPasskey } from "../wallet/ui/AddPasskey";

const LABEL: Record<MenuTab, string> = { game: "Game", rare: "Rare", items: "Items", market: "Market", trophies: "Trophies", friends: "Friends", news: "Notifications" };

/**
 * `session` is the native overlay over a game; null is the launcher's own menu (the launcher has
 * the screen), which `onClose` closes.
 */
export function RareMenu({ session, onClose }: { session: Session | null; onClose?: () => void }) {
  const overGame = !!session?.gameId;
  const library = useApp((s) => s.library);
  const games = useGames();
  const game = useMemo(() => {
    const g = resolveGame(session?.gameId ?? null, library, games);
    // An app (Telegram, YouTube, Steam's own UI): its own name and icon.
    const app = session?.gameId ? appByRunId(session.gameId) : undefined;
    return app ? { ...g, title: app.name, banner: null, logo: app.icon } : g;
  }, [session?.gameId, library, games]);
  const account = useAccount();
  const address = account?.address ?? null;
  const sui = overGame && game.collections.length > 0;
  const { data, loading, refresh } = useOverlayData(game, address, sui && account !== undefined && games.length > 0);
  const caseData = useCaseData();
  const rareGame = game.steamAppId ? caseData.games.find((g) => g.appid === game.steamAppId) ?? null : null;
  const unread = useUnread();
  const tabs = menuTabs({ overGame, collections: sui, trophies: !!game.steamAppId || game.packages.length > 0 });
  // A party invite toast up when the Rare button was pressed: the menu opens on it, Join focused.
  const [invite] = useState(() => inviteUp(useNotify.getState().toasts, Date.now(), TOAST_MS));
  // A rare://overlay?tab= link (Local API) asked for a tab, else an invite toast, else the usual.
  const [tab, setTab] = useState<MenuTab>(() => (overGame ? takeOverlayTab() : null) ?? (invite ? "friends" : openingTab(overGame, unread)));
  const [detail, setDetail] = useState<OverlayItem | null>(null);
  const [buying, setBuying] = useState<ItemCard | null>(null);
  const [sub, setSub] = useState<"qs" | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const suiUsd = useLive().suiUsd ?? null;

  const close = useCallback(() => {
    if (session) void closeOverlay();
    else onClose?.();
  }, [session, onClose]);
  const where: Where = { overGame, close };

  useEffect(() => {
    feedback("open");
  }, []);
  // A tab that went away (the catalog loaded and the game has no collections): back to the first.
  useEffect(() => {
    if (!tabs.includes(tab)) setTab(tabs[0]);
  }, [tabs.join(","), tab]); // eslint-disable-line react-hooks/exhaustive-deps
  // Land on the tab's first row (Resume on Game, the default button in an item) once it has content.
  const friends = useFriends((s) => s.status);
  const landing = `${tab}:${detail?.id ?? ""}:${data ? "data" : ""}:${sub ?? ""}:${tab === "friends" ? friends : ""}`;
  useEffect(() => {
    if (sub) return;
    const t = setTimeout(() => {
      const join = invite && tab === "friends" ? root.current?.querySelector<HTMLElement>("[data-invite-join] [data-nav]") : null;
      if (join) join.focus();
      else if (root.current) focusFirst(root.current);
    }, 40);
    return () => clearTimeout(t);
  }, [landing]); // eslint-disable-line react-hooks/exhaustive-deps

  const back = useCallback(() => {
    feedback("back");
    close();
  }, [close]);

  usePadHandler(
    (b) => {
      if (buying) {
        if (b === "b") {
          setBuying(null);
          feedback("back");
          return true;
        }
        return false;
      }
      switch (b) {
        case "b":
          if (detail) {
            setDetail(null);
            feedback("back");
          } else back();
          return true;
        case "lt":
        case "rt": {
          if (detail) return true;
          const i = tabs.indexOf(tab);
          setTab(tabs[(i + (b === "rt" ? 1 : tabs.length - 1)) % tabs.length]);
          feedback("tab");
          return true;
        }
        // No top tabs here, and nothing underneath may act (search, Steam, the launcher's tabs).
        case "lb":
        case "rb":
        case "x":
        case "y":
        case "select":
        case "start":
        case "exithold":
          return true;
        default:
          return false;
      }
    },
    !sub,
  );

  const items = data?.items ?? [];
  const worth = items.reduce((s, x) => s + BigInt(x.floorMist ?? "0"), 0n);
  const offers = items.reduce((s, x) => s + BigInt(x.topOfferMist ?? "0"), 0n);
  const backdrop = session ? (session.frame ? fileSrc(session.frame) : game.banner) : null;
  const kind = gameKind(session?.gameId ?? null);

  return (
    <div className="fixed inset-0 z-40 flex">
      {session ? <Frozen src={backdrop} /> : <div aria-hidden className="absolute inset-0 bg-black/55" />}

      <div className="relative flex min-w-0 flex-1 flex-col justify-end px-[2.5rem] pb-[2.25rem]">
        {overGame && (
          <div className="flex items-center gap-[0.9rem]">
            {game.logo && (
              <span className="relative size-[2.75rem] shrink-0 overflow-hidden rounded-card bg-l3">
                {/^https?:/.test(game.logo) ? <MImg src={game.logo} label={game.title} width={120} className="absolute inset-0 size-full" /> : <img src={game.logo} alt="" className="absolute inset-0 size-full object-contain" />}
              </span>
            )}
            <div className="min-w-0">
              <div className="truncate text-title font-semibold">{game.title}</div>
              <div className="text-small text-fg/75">{kind === "app" ? "Open" : "Running"} under Rare</div>
            </div>
          </div>
        )}
        {session && (
          <div className="mt-[1rem] flex items-center gap-[1.25rem] text-small text-fg/80">
            <Hint b="b">{overGame ? (kind === "app" ? "Back to the app" : "Back to game") : "Close"}</Hint>
            {!(session.guest || isGuest()) && (
              <>
                <span className="flex items-center gap-[0.45rem]">
                  <Glyph b="rare" /> Home
                </span>
                <span className="flex items-center gap-[0.45rem]">
                  Hold <Glyph b="rare" /> to switch games
                </span>
              </>
            )}
          </div>
        )}
      </div>

      <aside ref={root} data-nav-scope className="slide-in-right relative flex h-full w-[32rem] shrink-0 flex-col border-l border-hair bg-l1 shadow-[-2rem_0_4rem_rgb(0_0_0/0.45)]">
        <header className="flex flex-col gap-[0.9rem] px-[1.4rem] pt-[1.3rem]">
          <div className="flex items-center gap-[0.6rem]">
            <span className="text-row font-semibold">Rare</span>
            {overGame && <span className="min-w-0 truncate text-small text-secondary">in {game.title}</span>}
            {address && (
              <span className="ml-auto flex shrink-0 items-center gap-[0.4rem] text-meta text-secondary">
                <WalletAvatar address={address} size="1.1rem" />
                <span className="num">{`${address.slice(0, 6)}...${address.slice(-4)}`}</span>
              </span>
            )}
          </div>
          {tab === "items" && !detail && address && items.length > 0 && (
            <StatLine
              items={[
                { label: "Items", value: <span className="tabular-nums">{items.length}</span> },
                { label: "At floor", value: worth > 0n ? <Sui mist={worth} dp={2} /> : null },
                { label: "Best offers", value: offers > 0n ? <Sui mist={offers} dp={2} /> : null },
              ]}
            />
          )}
          {!detail && (
            <div className="flex h-[2.4rem] items-stretch gap-[0.8rem] border-b border-hair">
              <span className="flex items-center">
                <Glyph b="lt" size="sm" />
              </span>
              <Tabs
                size="sm"
                value={tab}
                items={tabs.map((t) => ({
                  value: t,
                  label: LABEL[t],
                  count: t === "news" ? unread || undefined : t === "trophies" ? (rareGame?.sum.unlocked ?? 0) + (data?.trophies.length ?? 0) || undefined : undefined,
                }))}
              />
              <span className="ml-auto flex items-center">
                <Glyph b="rt" size="sm" />
              </span>
            </div>
          )}
        </header>

        <div className="relative min-h-0 flex-1">
          <LoadBar on={loading && (tab === "items" || tab === "market" || tab === "trophies" || tab === "game")} />
          <div className="h-full overflow-y-auto px-[0.65rem] pb-[1rem] pt-[0.6rem]">
            {tab === "game" && session ? (
              <GameTab session={session} game={game} items={sui ? data?.items ?? null : null} onItems={() => setTab("items")} onPerformance={() => setSub("qs")} />
            ) : tab === "rare" ? (
              <RareTab w={where} />
            ) : tab === "friends" ? (
              <FriendsTab w={where} />
            ) : tab === "news" ? (
              <NoticesTab w={where} />
            ) : account === null && tab !== "trophies" ? (
              <AddPasskey need="items" account={null} className="px-[0.75rem] pt-[0.4rem]" />
            ) : detail ? (
              <ItemDetail item={detail} account={account ?? null} floors={data?.floors ?? {}} onDone={refresh} />
            ) : tab === "items" ? (
              <ItemList items={items} ready={!!data} onOpen={setDetail} />
            ) : tab === "market" ? (
              <MarketList items={data?.market ?? []} ready={!!data && !loading} onOpen={setBuying} />
            ) : tab === "trophies" ? (
              <Trophies list={data?.trophies ?? []} rare={rareGame} ready={(!sui || (!!data && !loading)) && !caseData.loading} game={game.title} />
            ) : null}
          </div>
        </div>

        <footer className="flex h-[3rem] shrink-0 items-center gap-[1.1rem] border-t border-hair px-[1.4rem] text-small text-fg/75">
          {!detail && <Hint b="ltrt">Tabs</Hint>}
          <Hint b="a">Select</Hint>
          <span className="ml-auto">
            <Hint b="b">{detail ? "Back" : overGame ? (kind === "app" ? "Back to the app" : "Back to game") : "Close"}</Hint>
          </span>
        </footer>
      </aside>

      {buying?.listing && <BuySheet item={buying} listing={buying.listing} account={account ?? null} suiUsd={suiUsd} onClose={() => setBuying(null)} />}
      {sub === "qs" && <QuickSettingsHere onBack={() => (feedback("back"), setSub(null))} />}
    </div>
  );
}

/** Quick Settings inside the menu (the Game tab's Performance row): B or Quick goes back to the menu. */
function QuickSettingsHere({ onBack }: { onBack: () => void }) {
  return (
    <div className="absolute inset-0 z-10">
      <QuickSettings onClose={onBack} />
    </div>
  );
}
