// Small pieces shared by Top traders and the trader page.

import { Avatar, WalletAvatar } from "../../ui";
import type { UserChip } from "./api";
import { useRareName } from "../../friends/names";
import { traderName, traderNameWith } from "./api";

export function TraderAvatar({ user, size = "2.25rem" }: { user: Pick<UserChip, "address" | "avatarUrl" | "username" | "displayName">; size?: string }) {
  return user.avatarUrl ? <Avatar src={user.avatarUrl} label={traderName(user)} size={size} /> : <WalletAvatar address={user.address} size={size} />;
}

/** A trader's name: their Rare username when they have one, else as on the board. */
export function TraderLabel({ user }: { user: Pick<UserChip, "address" | "username" | "displayName"> }) {
  return <>{traderNameWith(useRareName(user.address), user)}</>;
}

/** 0.534 -> "53%"; unknown is left out. */
export const pct = (v: number | null | undefined) => (v == null || !Number.isFinite(v) ? "" : `${Math.round(v * 100)}%`);

/** "3m", "5h", "2d" since a time. */
export function ago(ms: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86_400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86_400)}d`;
}

/** The ledger's labels as words: "Smart money", "Whale". */
export function labelWords(l: { smart?: boolean; whale?: boolean; kol?: boolean } | undefined): string[] {
  return [l?.smart && "Smart money", l?.whale && "Whale", l?.kol && "KOL"].filter((x): x is string => !!x);
}
