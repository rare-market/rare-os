// `notices` for games: offer_received, sold and bought notices from rare.market's live stream
// (src/notify) about one of a running game's items go to that game (Rust fans them out to its
// /v1/notices streams and, for a connected web game, to rare:platform's on("notice")).
// A notice names the item's collection by slug; the game's collections are Move types, so their
// slugs come from rare.market once and are kept.

import { useApp } from "../lib/store";
import { useNotify, type Notice } from "../notify";
import { defaultDeps } from "./host";
import { gameFor, type Deps } from "./methods";

const KINDS = new Set(["offer_received", "sold", "bought"]);

/** What a game receives (the shape @rare/sdk's parseNotice reads). */
export function gameNotice(n: Notice) {
  return {
    id: n.id,
    kind: n.kind,
    title: n.title,
    body: n.body,
    itemId: n.item?.id ?? null,
    item: n.item ? { id: n.item.id, name: n.item.name, imageUrl: n.item.imageUrl, collection: n.item.collection } : null,
    amountMist: n.amount && n.amount.coinType.endsWith("::sui::SUI") ? n.amount.raw : null,
    digest: n.digest,
    at: n.createdAt,
  };
}

const slugCache = new Map<string, Promise<string | null>>();

async function slugsOf(gameId: string, d: Deps): Promise<Set<string>> {
  const game = await gameFor(gameId, d);
  const out = await Promise.all(
    game.collections.map((c) => {
      let p = slugCache.get(c.nftType);
      if (!p) {
        p = d.get<{ slug?: string | null }>(`/v1/collections/${encodeURIComponent(c.nftType)}`).then((x) => x?.slug ?? null, () => null);
        slugCache.set(c.nftType, p);
      }
      return p;
    }),
  );
  return new Set(out.filter((s): s is string => !!s));
}

/** The running games a notice is about. */
export async function gamesFor(n: Notice, running: string[], d: Deps): Promise<string[]> {
  const slug = n.item?.collection?.slug;
  if (!KINDS.has(n.kind) || !slug) return [];
  const hits = await Promise.all(running.map(async (id) => ((await slugsOf(id, d)).has(slug) ? id : null)));
  return hits.filter((x): x is string => !!x);
}

export function startNoticeForward(send: (gameId: string, notice: ReturnType<typeof gameNotice>) => void, d: Deps = defaultDeps()) {
  const seen = new Set(useNotify.getState().notices.map((n) => n.id));
  useNotify.subscribe((s) => {
    const fresh = s.notices.filter((n) => !seen.has(n.id));
    if (!fresh.length) return;
    fresh.forEach((n) => seen.add(n.id));
    const running = useApp.getState().running;
    if (!running.length) return;
    for (const n of fresh) void gamesFor(n, running, d).then((ids) => ids.forEach((id) => send(id, gameNotice(n))));
  });
}
