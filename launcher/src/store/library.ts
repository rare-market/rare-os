// Owned Store games for the Library: licences from rare.market (/v1/store/licences?owner=) that
// aren't installed yet, as Library entries next to the scanned ones. Installed store games come
// from the Rust scan (source "rare", id "rare:<slug>").

import { useMemo } from "react";
import type { Game } from "../lib/types";
import { useStore, useStoreFresh } from "./state";
import { SHOW_STORE } from "./show";
import type { Licence } from "./types";

const NONE: Game[] = [];

export function licenceEntry(l: Licence): Game {
  return {
    id: `rare:${l.slug}`,
    source: "rare",
    title: l.name,
    installed: false,
    art: { cover: l.image_url ? [l.image_url] : [], hero: [], logo: [], header: [] },
    origin: "Rare Store",
    foreign: false,
  };
}

/** Owned store games that the scan doesn't list (not installed on this device). */
export function useOwnedStoreGames(scanned: Game[]): Game[] {
  // SHOW_STORE is a constant, so the hooks below run on every render or on none.
  useStoreFresh(SHOW_STORE);
  const licences = useStore((s) => s.licences);
  return useMemo(() => {
    if (!SHOW_STORE) return NONE;
    const have = new Set(scanned.filter((g) => g.source === "rare").map((g) => g.id));
    const seen = new Set<string>();
    return licences.filter((l) => !have.has(`rare:${l.slug}`) && !seen.has(l.slug) && seen.add(l.slug)).map(licenceEntry);
  }, [licences, scanned]);
}
