// "Owned" on Sui game cards and pages, from what the account holds on chain (ownership.ts), and the
// link ownership unlocks: the player's own items in Rare Marketplace.

import { useEffect } from "react";
import { loadGames, type SuiGame } from "../../lib/rare";
import { useApp } from "../../lib/store";
import { openInMarket } from "../../marketplace/nav";
import { Button, Dot } from "../../ui";
import { useOwnership } from "../ownership";
import { useProfile } from "../profile";

/** Loads ownership for the current account once (cached a minute). */
export function useOwnedGames() {
  const address = useProfile((s) => s.address);
  useEffect(() => {
    void useProfile.getState().refresh();
  }, []);
  useEffect(() => {
    if (address) void loadGames().then((r) => useOwnership.getState().load(address, r.games));
  }, [address]);
  return useOwnership((s) => s.bySlug);
}

/** Muted line for cards and the game page: a white dot and "Owned · 3 items". Nothing when not owned. */
export function OwnedLine({ slug, className = "" }: { slug: string; className?: string }) {
  const own = useOwnership((s) => s.bySlug.get(slug));
  if (!own) return null;
  return (
    <span className={`flex items-center gap-[0.45rem] text-fg ${className}`}>
      <Dot tone="primary" />
      Owned · {own.count.toLocaleString("en-US")} {own.count === 1 ? "item" : "items"}
    </span>
  );
}

/** Buttons ownership unlocks on the game page. */
export function OwnedActions({ game }: { game: SuiGame }) {
  const own = useOwnership((s) => s.bySlug.get(game.slug));
  if (!own) return null;
  const col = own.collections.find((c) => c.slug);
  if (!col?.slug) return null;
  return (
    <Button
      hint="Your items"
      onClick={() => {
        useApp.getState().close();
        openInMarket({ kind: "collection", slug: col.slug! });
      }}
    >
      Your items
    </Button>
  );
}
