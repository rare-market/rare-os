// Unified search: installed games from every store, Sui games, and Rare Marketplace
// collections and accounts, in one list. Opens with Y.

import { useEffect, useRef, useState } from "react";
import { native } from "../lib/bridge";
import { loadGames, type SuiGame } from "../lib/rare";
import { useMarketArea } from "../market/area";
import { useApp } from "../lib/store";
import { SOURCE_LABEL, type Game } from "../lib/types";
import { focusFirst } from "../nav/focus";
import { Art } from "./Art";
import { askText } from "./Osk";
import { openInMarket } from "../marketplace/nav";
import { IconSearch } from "./icons";
import { Avatar } from "../marketplace/parts";
import { SectionHead } from "../ui/controls";
import { CoinResults } from "../trade/CoinSearch";
import { openInTrade } from "../trade/nav";

interface Collection {
  slug: string;
  name: string;
  imageUrl: string | null;
}

export function Search() {
  const close = useApp((s) => s.close);
  const open = useApp((s) => s.open);
  const library = useApp((s) => s.library);
  const onTrade = useApp((s) => s.tab === "market" && useMarketArea.getState().area === "trade");
  const [q, setQ] = useState("");
  const [sui, setSui] = useState<SuiGame[]>([]);
  const [cols, setCols] = useState<Collection[]>([]);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void loadGames().then((r) => setSui(r.games));
    void askText(onTrade ? "Search any coin by name, ticker or coin type" : "Search games, Sui games, collections and coins", "", { placeholder: onTrade ? "Name, ticker or 0x..::mod::NAME" : "Type a name" }).then((v) => {
      if (v === null) close();
      else setQ(v);
    });
  }, [close]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!q) return;
    if (!onTrade)
      void native
        .rareGet<{ collections: Collection[] }>(`/v1/search?q=${encodeURIComponent(q)}`)
        .then((r) => setCols((r.collections ?? []).slice(0, 8)))
        .catch(() => setCols([]));
    setTimeout(() => root.current && focusFirst(root.current), 50);
  }, [q]);

  const needle = q.toLowerCase();
  const coins = <CoinResults q={q} group="search-coins" first={onTrade} onOpen={(t) => openInTrade({ kind: "token", coinType: t.coinType })} />;
  const games: Game[] = (library?.games ?? []).filter((g) => g.title.toLowerCase().includes(needle)).slice(0, 12);
  const suiHits = sui.filter((g) => `${g.name} ${g.studio} ${g.genre}`.toLowerCase().includes(needle)).slice(0, 8);

  return (
    <div ref={root} data-nav-scope className="fade fixed inset-0 z-30 overflow-y-auto bg-bg px-[2.5rem] py-[2rem]" data-scroll>
      <button data-nav data-hint-a="Edit search" onClick={() => void askText("Search", q).then((v) => v !== null && setQ(v))} className="mb-[1.75rem] flex h-[3.25rem] w-full items-center gap-[0.8rem] rounded-card border border-border-light bg-l1 px-[1.1rem] text-left text-row">
        <IconSearch size="1.3rem" className="text-icon-2" />
        {q || <span className="text-muted">{onTrade ? "Search any coin by name, ticker or coin type" : "Search games, Sui games, collections and coins"}</span>}
      </button>
      {onTrade && <div className="flex flex-col gap-[1.6rem]">{coins}</div>}
      {!onTrade && q && (
        <div className="flex flex-col gap-[1.6rem]">
          <Group title="Your games" empty="No installed game matches">
            {games.map((g) => (
              <button key={g.id} data-nav onClick={() => open({ kind: "game", id: g.id })} className="tile relative aspect-[2/3] w-[8rem] shrink-0 overflow-hidden rounded-card bg-l3">
                <Art sources={g.art.cover} title={g.title} variant="cover" className="absolute inset-0 h-full w-full object-cover" />
                <span className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/85 to-transparent px-[0.5rem] pb-[0.4rem] pt-[1.2rem] text-left text-label text-white/85">{SOURCE_LABEL[g.source]}</span>
              </button>
            ))}
          </Group>
          <Group title="Sui Games" empty="No Sui game matches">
            {suiHits.map((g) => (
              <button key={g.slug} data-nav onClick={() => open({ kind: "suigame", slug: g.slug })} className="tile flex shrink-0 items-center gap-[0.75rem] rounded-card border border-hair bg-l1 py-[0.6rem] pl-[0.6rem] pr-[1.1rem] text-left">
                <Avatar src={g.logoUrl} label={g.name} size="2.75rem" square />
                <span>
                  <span className="block text-body font-semibold">{g.name}</span>
                  <span className="block text-meta text-muted">{[g.studio, g.genre].filter(Boolean).join(" · ")}</span>
                </span>
              </button>
            ))}
          </Group>
          <Group title="Rare Marketplace" empty="No collection matches">
            {cols.map((c) => (
              <button key={c.slug} data-nav onClick={() => openInMarket({ kind: "collection", slug: c.slug })} className="tile flex shrink-0 items-center gap-[0.75rem] rounded-card border border-hair bg-l1 py-[0.6rem] pl-[0.6rem] pr-[1.1rem]">
                <Avatar src={c.imageUrl} label={c.name} size="2.75rem" />
                <span className="text-body font-semibold">{c.name}</span>
              </button>
            ))}
          </Group>
          {coins}
        </div>
      )}
    </div>
  );
}

function Group({ title, empty, children }: { title: string; empty: string; children: React.ReactNode[] }) {
  return (
    <section>
      <SectionHead title={title} note={children.length ? String(children.length) : undefined} className="mb-[0.8rem]" />
      {children.length ? <div data-rail data-nav-group={`search-${title}`} className="flex gap-[0.9rem] overflow-x-auto">{children}</div> : <div className="text-small text-muted">{empty}</div>}
    </section>
  );
}
