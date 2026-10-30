// Small friends pieces from the ui kit: a player's picture (their NFT, else the wallet gem pattern),
// the presence line (a Dot and muted text, never a pill) and a game's art for a friend's game.

import { Art } from "../../components/Art";
import { useApp } from "../../lib/store";
import { BUILTIN_WEB_GAMES } from "../../lib/webgames";
import { Avatar, Dot, WalletAvatar } from "../../ui";
import type { GameRef, Person, Presence } from "../types";
import { presenceText } from "../format";

export { presenceText };

export function FriendAvatar({ p, size = "2.25rem" }: { p: Pick<Person, "address" | "avatarUrl" | "name">; size?: string }) {
  return p.avatarUrl ? <Avatar src={p.avatarUrl} label={p.name ?? p.address} size={size} /> : <WalletAvatar address={p.address} size={size} />;
}

export function PresenceLine({ p, long = false, className = "" }: { p: Presence; long?: boolean; className?: string }) {
  return (
    <span className={`flex min-w-0 items-center gap-[0.45rem] ${className}`}>
      {p.state !== "offline" && <Dot tone={p.state === "online" ? "primary" : "muted"} />}
      <span className={`truncate ${p.state === "online" && p.game ? "text-fg/85" : "text-muted"}`}>{presenceText(p, long)}</span>
    </span>
  );
}

/** Cover art for a friend's game: this device's own art when it has the game, Steam's for Steam games. */
export function gameArt(ref: GameRef, kind: "cover" | "header" = "cover"): string[] {
  const own = useApp.getState().library?.games.find((g) => g.id === ref.id);
  if (own) return kind === "cover" ? own.art.cover : own.art.header.length ? own.art.header : own.art.hero;
  if (ref.kind === "steam" && ref.appid) {
    const base = `https://cdn.cloudflare.steamstatic.com/steam/apps/${ref.appid}`;
    return kind === "cover" ? [`${base}/library_600x900.jpg`, `${base}/library_600x900_2x.jpg`] : [`${base}/header.jpg`];
  }
  const builtin = BUILTIN_WEB_GAMES.find((b) => `web:${b.id}` === ref.id);
  if (builtin) return kind === "cover" ? (builtin.cover ? [builtin.cover] : []) : builtin.hero ? [builtin.hero] : [];
  return [];
}

export function GameArt({ game, kind = "cover", className = "" }: { game: GameRef; kind?: "cover" | "header"; className?: string }) {
  return <Art sources={gameArt(game, kind)} title={game.title} variant={kind} className={className} />;
}

export const shortAddress = (a: string) => `${a.slice(0, 6)}...${a.slice(-4)}`;
