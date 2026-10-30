// Marketplace search: the on-screen keyboard for the term, then collections and wallets.

import { Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { askText } from "../components/Osk";
import { paths, useRare, type SearchResult } from "./api";
import { shortAddr } from "./format";
import { useMarketNav } from "./nav";
import { H, useLandingFocus, usePageButtons, usePageHints } from "./page";
import { Avatar, Empty, Sui, Verified, WalletAvatar } from "./parts";

export function SearchPage({ initial, frame, active }: { initial: string; frame: number; active: boolean }) {
  const push = useMarketNav((s) => s.push);
  const pop = useMarketNav((s) => s.pop);
  const [q, setQ] = useState(initial);
  const root = useRef<HTMLDivElement>(null);
  const res = useRare<SearchResult>(q ? paths.search(q) : null, 60_000);

  const edit = (first = false) =>
    void askText("Search Rare Marketplace", q, { placeholder: "Collection name, wallet or name.sui" }).then((v) => {
      if (v === null) {
        if (first && !q) pop();
        return;
      }
      setQ(v.trim());
    });

  useEffect(() => {
    if (!initial) edit(true);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useLandingFocus(root, active, res.data);
  usePageButtons(frame, (b) => {
    if (b === "y") {
      edit();
      return true;
    }
    return false;
  });
  usePageHints(active, [H.open, { b: "y", label: "Edit search" }, H.back]);

  const cols = res.data?.collections ?? [];
  const accts = res.data?.accounts ?? [];
  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto px-10 pb-10 pt-2">
      <button data-nav onClick={() => edit()} className="flex h-12 w-full items-center gap-2 rounded border border-border-light bg-transparent px-3 text-left text-base transition-colors focus-visible:border-border-interactive-focus">
        <Search size="1.1rem" className="text-icon-secondary" />
        {q || <span className="text-secondary">Search collections</span>}
      </button>
      {q && res.data && cols.length === 0 && accts.length === 0 && <Empty title={`No results for "${q}"`} />}
      {accts.length > 0 && (
        <section className="mt-6">
          <h2 className="mb-2 text-xs uppercase text-secondary">Wallets</h2>
          {accts.map((a) => (
            <button key={a.address} data-nav onClick={() => push({ kind: "account", address: a.address, label: a.name ?? shortAddr(a.address) })} className="flex h-14 w-full items-center gap-3 rounded-lg px-3 text-left text-sm transition-colors focus-visible:bg-layer-03">
              <WalletAvatar address={a.address} size="2rem" />
              <span className="font-semibold">{a.name ?? shortAddr(a.address)}</span>
              <span className="font-mono text-secondary">{shortAddr(a.address)}</span>
            </button>
          ))}
        </section>
      )}
      {cols.length > 0 && (
        <section className="mt-6">
          <h2 className="mb-2 text-xs uppercase text-secondary">Collections</h2>
          {cols.map((c, i) => (
            <button key={c.slug} data-nav data-nav-default={i === 0 ? "" : undefined} onClick={() => push({ kind: "collection", slug: c.slug })} className="flex h-16 w-full items-center gap-3 rounded-lg px-3 text-left text-sm transition-colors focus-visible:bg-layer-03">
              <Avatar src={c.imageUrl} label={c.name} />
              <span className="flex min-w-0 flex-1 items-center gap-1.5">
                <span className="truncate font-semibold">{c.name}</span>
                {c.verified && <Verified size="0.95rem" />}
              </span>
              <span className="w-[10rem] text-right">
                <span className="block text-xs text-secondary">Floor</span>
                <Sui mist={c.floorMist} dp={2} className="font-semibold" />
              </span>
              <span className="w-[10rem] text-right">
                <span className="block text-xs text-secondary">Volume 7d</span>
                <Sui mist={c.volumeMist} dp={2} compact className="font-semibold" />
              </span>
            </button>
          ))}
        </section>
      )}
    </div>
  );
}
