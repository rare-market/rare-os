// Loads the inventory. rare.market's grouped endpoint when it is deployed; until then the same
// answer is composed here from the wallet's items, the games catalog and collection floors.

import { native } from "../lib/bridge";
import { loadGames } from "../lib/rare";
import type { CollectionDetail, ItemCard, Page } from "../marketplace/api";
import { canonType } from "../sui/chain";
import { groupItems, normalizeInventory } from "./logic";
import type { Inventory, InvItem } from "./types";

export type RareGet = <T>(path: string) => Promise<T>;
const enc = encodeURIComponent;

export const isMissing = (e: unknown) => /\b404\b/.test((e as Error)?.message ?? "");

export async function loadInventory(get: RareGet, address: string, games = () => loadGames().then((r) => r.games)): Promise<Inventory> {
  try {
    return normalizeInventory(await get<{ data: unknown; total?: number }>(`/v1/accounts/${enc(address)}/inventory`));
  } catch (e) {
    if (!isMissing(e)) throw e;
  }
  const items: ItemCard[] = [];
  let cursor: string | null = null;
  for (let i = 0; i < 4; i++) {
    const page: Page<ItemCard> = await get<Page<ItemCard>>(`/v1/accounts/${enc(address)}/items?limit=100${cursor ? `&cursor=${enc(cursor)}` : ""}`);
    items.push(...page.data);
    cursor = page.nextCursor;
    if (!cursor) break;
  }
  const catalog = await games().catch(() => []);
  const types = Array.from(new Set(items.map((x) => canonType(x.type)))).slice(0, 24);
  const floors: Record<string, string | null> = {};
  await Promise.all(
    types.map(async (t) => {
      floors[t] = await get<CollectionDetail>(`/v1/collections/${enc(t)}`).then((d) => d.stats?.floorMist ?? null, () => null);
    }),
  );
  const groups = groupItems(items as InvItem[], catalog, floors);
  return { groups, total: items.length };
}

export const liveGet: RareGet = (p) => native.rareGet(p);
