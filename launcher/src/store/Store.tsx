// The Store tab: featured, new, top and on-sale shelves from rare.market's /v1/store/games, and
// search. A tile opens the game page (overlay "storegame"). X searches; B leaves the results.

import { Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { askText } from "../components/Osk";
import { useApp } from "../lib/store";
import { feedback } from "../nav/feedback";
import { focusFirst } from "../nav/focus";
import { useHints } from "../nav/hints";
import { usePadHandler } from "../nav/input";
import { Button, Empty, LoadBar, Rail, SectionHead } from "../ui";
import { api } from "./api";
import { StoreCard, useLoad } from "./parts";
import { useStoreFresh } from "./state";
import type { StoreGame } from "./types";

const SHELF_LIMIT = 24;

export function Store() {
  useStoreFresh();
  useEffect(() => {
    // Dev and screenshots: ?storegame=<slug> opens that game's page.
    const sg = new URLSearchParams(location.search).get("storegame");
    if (sg) setTimeout(() => useApp.getState().open({ kind: "storegame", slug: sg }), 300);
  }, []);
  const [q, setQ] = useState<string | null>(() => new URLSearchParams(location.search).get("storeq"));
  const [retry, setRetry] = useState(0);
  const featured = useLoad(`featured:${retry}`, () => api.games("featured", { limit: SHELF_LIMIT }));
  const fresh = useLoad(`new:${retry}`, () => api.games("new", { limit: SHELF_LIMIT }));
  const top = useLoad(`top:${retry}`, () => api.games("top", { limit: SHELF_LIMIT }));
  const sale = useLoad(`sale:${retry}`, () => api.games("sale", { limit: SHELF_LIMIT }));
  const results = useLoad(q ? `search:${q}` : null, () => api.search(q!));
  const root = useRef<HTMLDivElement>(null);

  const search = async () => {
    const t = (await askText("Search the store", q ?? "", { placeholder: "Game, studio or genre" }))?.trim();
    if (t) setQ(t);
  };

  usePadHandler((b) => {
    const app = useApp.getState();
    if (app.overlay || app.tab !== "store") return false;
    if (b === "x") {
      feedback("open");
      void search();
      return true;
    }
    if (b === "b" && q) {
      setQ(null);
      feedback("back");
      return true;
    }
    return false;
  });
  const overlay = useApp((s) => !!s.overlay);
  useHints([{ b: "x", label: "Search store" }], !overlay);

  // Results or shelves arriving: land focus on the first tile once.
  const ready = q ? !!results.data : !!featured.data;
  useEffect(() => {
    if (ready && root.current && !root.current.contains(document.activeElement)) setTimeout(() => root.current && focusFirst(root.current), 30);
  }, [ready, q]);

  const loading = featured.loading || fresh.loading || top.loading || sale.loading || results.loading;
  const failed = featured.error && fresh.error && top.error;
  const feat = featured.data?.items ?? [];

  return (
    <div ref={root} data-scroll className="relative h-full overflow-y-auto pb-[2.5rem] pt-[0.5rem]">
      <LoadBar on={loading} />
      <div className="mb-[1.25rem] flex items-end gap-[1rem] px-[2.5rem]">
        <div>
          <h1 className="text-title font-semibold">Store</h1>
          <p className="text-small text-muted">Games for Rare OS. Licences bought here live in your wallet on Sui.</p>
        </div>
        <button
          data-nav
          data-hint-a="Search"
          onClick={() => void search()}
          className="ml-auto flex h-[2.75rem] w-[22rem] items-center gap-[0.5rem] rounded-chip border border-border-light px-[0.9rem] text-left text-small text-muted"
        >
          <Search size="1rem" className="shrink-0" />
          <span className="truncate">{q ? q : "Search the store"}</span>
        </button>
      </div>

      {q ? (
        <div className="px-[2.5rem]">
          <SectionHead title={`Results for "${q}"`} note={results.data ? `${results.data.length}` : undefined} className="mb-[0.9rem]" />
          {results.data && results.data.length === 0 ? (
            <Empty title="No games match that search" body="Try a studio name or a genre.">
              <Button variant="primary" def onClick={() => void search()}>
                Search again
              </Button>
            </Empty>
          ) : results.error ? (
            <Empty title="Couldn't reach the store" body="Check the connection and try again." />
          ) : (
            <Grid id="store-results" games={results.data ?? []} />
          )}
        </div>
      ) : failed ? (
        <div className="px-[2.5rem]">
          <Empty title="Couldn't reach the store" body="Check the connection. The store needs rare.market to show games.">
            <Button variant="primary" def onClick={() => setRetry((n) => n + 1)}>
              Try again
            </Button>
          </Empty>
        </div>
      ) : (
        <>
          {feat.length > 0 && (
            <div className="px-[2.5rem]">
              <SectionHead title="Featured" className="mb-[0.9rem]" />
              <Grid id="store-featured" games={feat.slice(0, 6)} />
            </div>
          )}
          <Shelf id="store-new" title="New" games={fresh.data?.items} />
          <Shelf id="store-top" title="Top sellers" note="30 days" games={top.data?.items} />
          <Shelf id="store-sale" title="On sale" games={sale.data?.items} />
        </>
      )}
    </div>
  );
}

function Grid({ id, games }: { id: string; games: StoreGame[] }) {
  return (
    <div data-nav-group={id} className="grid grid-cols-3 gap-[1rem]">
      {games.map((g, i) => (
        <StoreCard key={g.slug} game={g} wide def={i === 0} />
      ))}
    </div>
  );
}

/** A shelf shows only when it has games (an empty "On sale" stays out of the way). */
function Shelf({ id, title, note, games }: { id: string; title: string; note?: string; games: StoreGame[] | undefined }) {
  if (!games?.length) return null;
  return (
    <Rail id={id} title={title} note={note} className="mt-[2.25rem]">
      {games.map((g) => (
        <StoreCard key={g.slug} game={g} />
      ))}
    </Rail>
  );
}
