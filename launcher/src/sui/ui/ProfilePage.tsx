// The player's profile: picture and SuiNS name, money (SUI plus collectibles at rare.market's
// value, today's change), the games they played on this device, the Sui games they own on chain,
// and their collectibles. The picture is picked from their own NFTs (A on any collectible here).

import { useEffect, useMemo, useRef, useState } from "react";
import { Cloud, ImageIcon, Wallet } from "lucide-react";
import { GameTile } from "../../components/ui";
import { loadGames, type SuiGame } from "../../lib/rare";
import { useApp } from "../../lib/store";
import type { ItemCard } from "../../marketplace/api";
import { MImg } from "../../marketplace/MImg";
import { openInMarket } from "../../marketplace/nav";
import { Sui } from "../../marketplace/parts";
import { focusFirst } from "../../nav/focus";
import { useHints } from "../../nav/hints";
import { usePadHandler } from "../../nav/input";
import { Button, Empty, Rail, StatLine } from "../../ui";
import { Num } from "../../ui/Num";
import { rareSui } from "../data";
import { totalUsd, useSuiLive } from "../live";
import { useLive } from "../../lib/live";
import { useOwnership, type GameOwnership } from "../ownership";
import { displayName, useProfile } from "../profile";
import { ProfileAvatar } from "./bits";
import { pickUsername } from "../../friends/ui/pickUsername";
import { PerksSection, RarelingBadge } from "../../perks/ui";
import { ProfileTrophies } from "../../trophies";

const hours = (min: number) => (min < 60 ? `${min} min` : `${Math.round(min / 60).toLocaleString("en-US")} h`);

function sinceLabel(t: number): string {
  const h = (Date.now() - t) / 3600_000;
  if (h >= 20) return "24h";
  return `Since ${new Date(t).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}`;
}

export function ProfilePage() {
  const prof = useProfile();
  const live = useSuiLive();
  const library = useApp((s) => s.library);
  const owned = useOwnership((s) => s.bySlug);
  const [games, setGames] = useState<SuiGame[]>([]);
  const [items, setItems] = useState<ItemCard[] | null>(null);
  const [picking, setPicking] = useState(false);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void prof.refresh(true);
    void loadGames().then((r) => setGames(r.games));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (!prof.address) return;
    void rareSui
      .items(prof.address, 60)
      .then((r) => setItems(r.data))
      .catch(() => setItems([]));
    if (games.length) void useOwnership.getState().load(prof.address, games);
  }, [prof.address, games]);
  useEffect(() => {
    setTimeout(() => root.current && focusFirst(root.current), 30);
  }, []);

  const played = useMemo(
    () =>
      (library?.games ?? [])
        .filter((g) => g.source !== "apps" && (g.playtimeMinutes || g.lastPlayed))
        .sort((a, b) => (b.playtimeMinutes ?? 0) - (a.playtimeMinutes ?? 0) || (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0)),
    [library],
  );
  const minutes = played.reduce((s, g) => s + (g.playtimeMinutes ?? 0), 0);
  const ownedGames = games.filter((g) => owned.has(g.slug));
  const coinsUsd = useLive((st) => (st.portfolio && prof.address && st.portfolio.address.toLowerCase() === prof.address.toLowerCase() ? st.portfolio.totalUsd : null));
  const usd = totalUsd({ suiMist: prof.suiMist, nftMist: prof.nftMist, suiUsd: live.suiUsd ?? prof.suiUsd, coinsUsd });

  if (!prof.address) {
    return (
      <div ref={root} data-nav-scope className="rise fixed inset-0 z-30 flex items-center bg-bg px-[3rem]">
        <Empty title="Add your Sui account" body="Your SuiNS name, picture, Sui games and collectibles show up here once Rare Wallet has an account.">
          <Button variant="primary" def icon={<Wallet size="1.1rem" />} onClick={() => (useApp.getState().close(), useApp.getState().setTab("wallet"))}>
            Open Rare Wallet
          </Button>
          <Button onClick={() => (useApp.getState().close(), useApp.getState().setTab("settings"))}>Settings</Button>
        </Empty>
      </div>
    );
  }

  return (
    <div ref={root} data-nav-scope className="rise fixed inset-0 z-30 bg-bg">
      <div data-scroll className="h-full overflow-y-auto pb-[3rem]">
        <header className="flex items-end gap-[1.5rem] px-[2.5rem] pt-[2.25rem]">
          <ProfileAvatar size="6rem" />
          <div className="min-w-0 flex-1">
            <div className="text-label text-secondary">Profile</div>
            <div className="flex items-baseline gap-[1rem]">
              <h1 className="truncate text-title font-semibold tracking-[-0.02em]">{displayName(prof)}</h1>
              <RarelingBadge />
            </div>
            {prof.username && prof.name && <div className="truncate text-body text-muted">{prof.name}</div>}
            <div className="num truncate text-meta text-muted">{prof.address}</div>
          </div>
          <div data-nav-group="profile-actions" data-nav-remember className="flex gap-[0.75rem]">
            <Button def icon={<ImageIcon size="1.05rem" />} hint="Pick a picture" onClick={() => setPicking(true)}>
              Change picture
            </Button>
            <Button onClick={() => void pickUsername()}>{prof.username ? "Change username" : "Pick a username"}</Button>
            <Button icon={<Cloud size="1.05rem" />} onClick={() => useApp.getState().open({ kind: "saves" })}>
              Cloud saves
            </Button>
            <Button icon={<Wallet size="1.05rem" />} onClick={() => (useApp.getState().close(), useApp.getState().setTab("wallet"))}>
              Wallet
            </Button>
            <Button onClick={() => (useApp.getState().close(), useApp.getState().setTab("friends"))}>Friends</Button>
            <Button onClick={() => (useApp.getState().close(), useApp.getState().setTab("settings"))}>Settings</Button>
          </div>
        </header>

        <StatLine
          size="lg"
          className="mx-[2.5rem] mt-[1.75rem] rounded-panel bg-l1 px-[1.25rem] py-[1rem]"
          items={[
            { label: "Total value", value: usd != null ? <Num kind="money" value={usd} live /> : null },
            { label: live.change ? sinceLabel(live.change.since) : "Today", value: live.change ? <Num kind="pnl" value={live.change.usd} live /> : null },
            { label: coinsUsd != null ? "Coins" : "SUI", value: coinsUsd != null ? <Num kind="money" value={coinsUsd} /> : prof.suiMist !== null ? <Sui mist={prof.suiMist} dp={2} /> : null },
            { label: "Collectibles", value: prof.nftMist !== null ? <Sui mist={prof.nftMist} dp={2} /> : null },
            { label: "Games played", value: played.length ? played.length.toLocaleString("en-US") : null },
            { label: "Time played", value: minutes ? hours(minutes) : null },
          ]}
        />

        <div className="mt-[2rem] flex flex-col gap-[2.25rem]">
          <PerksSection address={prof.address} />
          <ProfileTrophies />

          <Rail id="profile-sui-games" title="Your Sui games" note={ownedGames.length ? `${ownedGames.length} owned on Sui` : undefined}>
            {ownedGames.length ? (
              ownedGames.map((g) => <OwnedGameCard key={g.slug} game={g} own={owned.get(g.slug)!} />)
            ) : (
              <Empty className="w-[36rem] shrink-0" title="No Sui game items yet" body="Items from games in Rare's Sui Games catalog show up here, from your wallet and your kiosks." />
            )}
          </Rail>

          {played.length > 0 && (
            <Rail id="profile-played" title="Games you played" note="on this device">
              {played.slice(0, 16).map((g) => (
                <GameTile key={g.id} game={g} />
              ))}
            </Rail>
          )}

          <Rail id="profile-collectibles" title="Collectibles" note={items?.length ? `${items.length}${items.length >= 60 ? "+" : ""}` : undefined}>
            {items && items.length > 0 ? (
              items.slice(0, 24).map((it) => <ItemTile key={it.id} item={it} hint="View" onClick={() => (useApp.getState().close(), openInMarket({ kind: "item", id: it.id }))} />)
            ) : (
              <Empty className="w-[36rem] shrink-0" title={items ? "No collectibles yet" : "Loading collectibles"} />
            )}
          </Rail>
        </div>
      </div>
      {picking && <AvatarPicker items={items ?? []} onDone={() => setPicking(false)} />}
    </div>
  );
}

function OwnedGameCard({ game, own }: { game: SuiGame; own: GameOwnership }) {
  const art = own.items.find((i) => i.imageUrl)?.imageUrl ?? game.logoUrl;
  return (
    <button
      data-nav
      data-hint-a="Game page"
      onClick={() => useApp.getState().open({ kind: "suigame", slug: game.slug })}
      className="tile relative aspect-square w-[12rem] shrink-0 overflow-hidden rounded-card border border-hair bg-l1 text-left"
    >
      <MImg src={art} label={game.name} width={320} className="absolute inset-0 h-full w-full" />
      <span className="absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/95 via-black/50 to-transparent" />
      <span className="absolute inset-x-[1rem] bottom-[0.9rem]">
        <span className="block truncate text-body font-bold text-white">{game.name}</span>
        <span className="block truncate text-meta text-white/60">
          {own.count.toLocaleString("en-US")} {own.count === 1 ? "item" : "items"}
        </span>
      </span>
    </button>
  );
}

function ItemTile({ item, onClick, hint, chosen, def, className = "w-[9rem]" }: { item: ItemCard; onClick: () => void; hint: string; chosen?: boolean; def?: boolean; className?: string }) {
  return (
    <button data-nav data-hint-a={hint} data-nav-default={def ? "" : undefined} onClick={onClick} className={`tile relative aspect-square shrink-0 overflow-hidden rounded-card border border-hair bg-l3 text-left ${className}`}>
      <MImg src={item.imageUrl} label={item.name} width={256} className="absolute inset-0 h-full w-full" />
      <span className="absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/95 via-black/50 to-transparent" />
      <span className="absolute inset-x-[0.6rem] bottom-[0.5rem] block truncate text-label font-semibold text-white">{item.name}</span>
      {chosen && <span className="absolute right-[0.5rem] top-[0.5rem] h-[0.6rem] w-[0.6rem] rounded-full bg-fg" aria-label="Current picture" />}
    </button>
  );
}

/** Picks the profile picture from the player's own NFTs. A sets it, X goes back to the default. */
function AvatarPicker({ items, onDone }: { items: ItemCard[]; onDone: () => void }) {
  const pick = useProfile((s) => s.avatar);
  const root = useRef<HTMLDivElement>(null);
  const withArt = items.filter((i) => i.imageUrl);
  useEffect(() => {
    setTimeout(() => root.current && focusFirst(root.current), 30);
  }, []);
  useHints([{ b: "x", label: "Use default" }]);
  usePadHandler((b) => {
    if (b === "b") return onDone(), true;
    if (b === "x") return useProfile.getState().pickAvatar(null), onDone(), true;
    return false;
  });
  return (
    <div ref={root} data-nav-scope className="pop fixed inset-x-0 bottom-0 top-[4.25rem] z-40 overflow-y-auto rounded-t-sheet border-t border-hair bg-l1 px-[2.5rem] pb-[2rem] pt-[1.5rem]">
      <div className="mb-[1.25rem] text-row font-semibold">Pick your picture</div>
      {withArt.length ? (
        <div data-nav-group="avatar-grid" className="grid grid-cols-7 gap-[1rem]">
          {withArt.map((it, i) => (
              <ItemTile
                key={it.id}
                item={it}
                def={pick ? pick.id === it.id : i === 0}
                className="w-full"
                hint="Use this"
                chosen={pick?.id === it.id}
                onClick={() => {
                  useProfile.getState().pickAvatar(it);
                  useApp.getState().toast(`${it.name} is your picture`);
                  onDone();
                }}
              />
          ))}
        </div>
      ) : (
        <Empty title="No NFTs with art yet" body="Any NFT you hold can be your picture. Buy one in Rare Marketplace and it shows up here." />
      )}
    </div>
  );
}
