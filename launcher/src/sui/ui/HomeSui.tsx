// Home's Sui shelf: your profile card (picture, SuiNS name, portfolio and its change, Sui games
// you own, cloud saves), then what is minting on rare.market's launchpad right now. Network numbers
// (SUI price, TPS, gas) live in the status bar and the SUI panel, so they are not repeated here.

import { useEffect, useState } from "react";
import { Cloud, Images } from "lucide-react";
import { native } from "../../lib/bridge";
import { gamesNow, loadGames, RARE, type SuiGame } from "../../lib/rare";
import { useApp } from "../../lib/store";
import { useLive } from "../../lib/live";
import { MImg } from "../../marketplace/MImg";
import { Rail } from "../../ui";
import { Num } from "../../ui/Num";
import { useSuiLive, type Mint } from "../live";
import { useOwnership } from "../ownership";
import { displayName, useProfile } from "../profile";
import { ProfileAvatar } from "./bits";
import { newCount, useShare } from "../../share/store";
import { rarelingsOn } from "../../perks/config";

export function SuiShelf() {
  const prof = useProfile();
  const mints = useSuiLive((s) => s.mints);
  const change = useSuiLive((s) => s.change);
  const mine = useSuiLive((s) => s.portfolioUsd);
  const trade = useLive((s) => s.portfolio);
  const owned = useOwnership((s) => s.bySlug);
  const [games, setGames] = useState<SuiGame[]>(() => gamesNow() ?? []);

  useEffect(() => {
    void loadGames().then((r) => setGames(r.games));
  }, []);
  useEffect(() => {
    if (prof.address && games.length) void useOwnership.getState().load(prof.address, games);
  }, [prof.address, games]);

  // Total value (coins plus collectibles), with this device's 24 h change; the shell's coin change until there is history.
  const value = mine ?? trade?.totalUsd ?? null;
  const delta = change?.usd ?? trade?.change24hUsd ?? null;
  const ownedCount = owned.size;

  return (
    <Rail id="sui-shelf" title="On Sui" note={mints?.length ? `${mints.length} minting now` : undefined}>
      <button
        data-nav
        data-hint-a={prof.address ? "Profile" : "Open Wallet"}
        onClick={() => (prof.address ? useApp.getState().open({ kind: "profile" }) : useApp.getState().setTab("wallet"))}
        className="tile tile-wide flex h-[12rem] w-[19rem] shrink-0 flex-col justify-between rounded-card border border-hair bg-l1 p-[1.1rem] text-left"
      >
        {prof.address ? (
          <>
            <span className="flex items-center gap-[0.75rem]">
              <ProfileAvatar size="2.75rem" />
              <span className="min-w-0">
                <span className="block truncate text-body font-semibold text-fg">{displayName(prof)}</span>
                <span className="block text-meta text-muted">{ownedCount ? `${ownedCount} Sui ${ownedCount === 1 ? "game" : "games"} owned` : "Your profile"}</span>
              </span>
            </span>
            <span className="flex items-end justify-between gap-[1rem]">
              <span className="min-w-0">
                <span className="block text-label text-secondary">Total value</span>
                {value != null ? <Num kind="money" value={value} live className="text-row font-semibold" /> : <span className="text-row text-muted">--</span>}
              </span>
              {delta != null && (
                <span className="text-right">
                  <span className="block text-label text-secondary">Today</span>
                  <Num kind="pnl" value={delta} live className="text-small font-medium" />
                </span>
              )}
            </span>
          </>
        ) : (
          <span className="flex h-full flex-col justify-between gap-[0.75rem]">
            <span className="block text-body font-semibold text-fg">Your Sui profile</span>
            <span className="block text-small text-muted">Add a Rare Wallet account to see your name, picture, games you own and cloud saves.</span>
          </span>
        )}
      </button>

      <button data-nav data-hint-a="Cloud saves" onClick={() => useApp.getState().open({ kind: "saves" })} className="tile flex aspect-square w-[12rem] shrink-0 flex-col justify-end gap-[0.2rem] rounded-card border border-hair bg-l1 p-[1rem] text-left">
        <Cloud size="1.6rem" className="mb-auto text-icon-2" />
        <span className="block text-body font-bold text-fg">Cloud saves</span>
        <span className="block text-meta text-muted">Emulator saves on Walrus</span>
      </button>

      <ShareTile />

      {(mints ?? []).map((m) => (
        <MintCard key={m.key} m={m} />
      ))}

      <button data-nav data-hint-a="Open" onClick={() => void native.openWeb(`${RARE}/launchpad`)} className="tile relative aspect-square w-[12rem] shrink-0 overflow-hidden rounded-card border border-hair bg-l1 text-left">
        {rarelingsOn() && <img src="home/launchpad-rarelings.webp" alt="" draggable={false} className="absolute inset-0 h-full w-full object-cover" />}
        <span className="absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/95 via-black/50 to-transparent" />
        <span className="absolute inset-x-[1rem] bottom-[0.9rem]">
          <span className="block truncate text-body font-bold text-white">Launchpad</span>
          <span className="block truncate text-meta text-white/60">{mints && mints.length === 0 ? "No live mints" : "New drops"}</span>
        </span>
      </button>
    </Rail>
  );
}

function MintCard({ m }: { m: Mint }) {
  return (
    <button data-nav data-hint-a="Mint" onClick={() => void native.openWeb(m.href)} className="tile relative aspect-square w-[12rem] shrink-0 overflow-hidden rounded-card border border-hair bg-l3 text-left">
      <MImg src={m.imageUrl} label={m.name} width={320} className="absolute inset-0 h-full w-full" />
      <span className="absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/95 via-black/50 to-transparent" />
      <span className="absolute inset-x-[1rem] bottom-[0.9rem]">
        <span className="block truncate text-body font-bold text-white">{m.name}</span>
        <span className="block truncate text-meta text-white/60">{m.sub}</span>
      </span>
    </button>
  );
}

function ShareTile() {
  const fresh = useShare(newCount);
  return (
    <button data-nav data-hint-a="Share" onClick={() => useApp.getState().open({ kind: "share" })} className="tile flex aspect-square w-[12rem] shrink-0 flex-col justify-end gap-[0.2rem] rounded-card border border-hair bg-l1 p-[1rem] text-left">
      <Images size="1.6rem" className="mb-auto text-icon-2" />
      <span className="block text-body font-bold text-fg">Share</span>
      <span className="block text-meta text-muted">{fresh > 0 ? `${fresh} new ${fresh === 1 ? "capture" : "captures"}` : "Screenshots and clips"}</span>
    </button>
  );
}
