// Small Sui pieces the shell mounts: the profile picture, the top bar account chip and the gas cell
// of the status bar. Styles are the ui kit's (rare.market tokens); nothing here invents a look.

import { useEffect } from "react";
import { useLive } from "../../lib/live";
import { useApp } from "../../lib/store";
import { MImg } from "../../marketplace/MImg";
import { WalletAvatar } from "../../marketplace/parts";
import { Num } from "../../ui/Num";
import { useSuiLive } from "../live";
import { displayName, useProfile } from "../profile";

/** The player's picture: the NFT they picked, else rare.market's avatar, else the gem pattern. */
export function ProfileAvatar({ size = "1.5rem", className = "" }: { size?: string; className?: string }) {
  const address = useProfile((s) => s.address);
  const pick = useProfile((s) => s.avatar);
  const market = useProfile((s) => s.marketAvatar);
  if (!address) return null;
  const src = pick?.imageUrl ?? market;
  if (!src) return <WalletAvatar address={address} size={size} />;
  return (
    <span className={`relative inline-block shrink-0 overflow-hidden rounded-full bg-l3 ${className}`} style={{ width: size, height: size }}>
      <MImg src={src} label={pick?.name ?? "?"} width={160} className="absolute inset-0 h-full w-full" />
    </span>
  );
}

/** Top bar: picture, SuiNS name (or short address) and portfolio value with today's change. A tap opens the profile. */
export function ProfileChip() {
  const p = useLive((s) => s.portfolio);
  const prof = useProfile();
  useEffect(() => {
    void useProfile.getState().refresh();
  }, [p?.address]);
  if (!prof.address) return null;
  return (
    <button onClick={() => useApp.getState().open({ kind: "profile" })} className="flex items-center gap-[0.6rem] text-left">
      <ProfileAvatar size="1.75rem" />
      <span className="flex min-w-0 flex-col leading-none">
        <span className="max-w-[10rem] truncate text-small font-semibold text-fg">{displayName(prof)}</span>
        {p ? (
          <span className="mt-[0.2rem] flex items-center gap-[0.4rem] text-label">
            <Num kind="money" value={p.totalUsd} live className="text-muted" />
            <Num kind="pnl" value={p.change24hUsd} live />
          </span>
        ) : null}
      </span>
    </button>
  );
}

/** Status bar cell: Sui's reference gas price. */
export function GasCell() {
  const gas = useSuiLive((s) => s.gasMist);
  // Unknown values are left out, never shown as a dash.
  if (gas == null) return null;
  return (
    <span className="flex h-full items-center gap-[0.5rem] border-r border-hair px-[1rem] text-muted">
      Gas
      <span className="tabular-nums text-fg">{`${gas.toLocaleString("en-US")} MIST`}</span>
    </span>
  );
}
