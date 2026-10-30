// Search results for coins: icon, name, ticker, price and 24h change per row, recent searches when
// the box is empty. Used by the launcher's search (Y), the Trade home field and the swap picker.

import { useEffect, useState } from "react";
import { focusEl } from "../nav/focus";
import { SectionHead } from "../ui";
import { useAsync, type TokenRow } from "./api";
import { Quiet, TokenListRow } from "./parts";
import { recentCoins, rememberCoin, searchCoins } from "./search";

/** Results for `q`; nothing is asked for an empty box. */
export function useCoinSearch(q: string) {
  const key = q.trim() ? `coin-search:${q.trim()}` : null;
  return useAsync<TokenRow[]>(key, () => searchCoins(q));
}

export function CoinResults({ q, onOpen, group, limit = 12, first }: { q: string; onOpen: (t: TokenRow) => void; group: string; limit?: number; first?: boolean }) {
  const res = useCoinSearch(q);
  const [recents] = useState(recentCoins);
  const open = (t: TokenRow) => {
    rememberCoin(t);
    onOpen(t);
  };
  // Results landing while focus still sits on the search box: move it to the first result.
  useEffect(() => {
    if (!first || !res.data?.length) return;
    const a = document.activeElement as HTMLElement | null;
    if (a && a.dataset.hintA === "Edit search") {
      const row = document.querySelector<HTMLElement>(`[data-nav-group="${group}"] [data-nav]`);
      if (row) focusEl(row, { scroll: false });
    }
  }, [res.data]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!q.trim()) {
    if (!recents.length) return null;
    return (
      <section>
        <SectionHead title="Recent" className="mb-[0.5rem]" />
        <div data-nav-group={group}>
          {recents.map((r, i) => (
            <TokenListRow key={r.coinType} idx={i} def={first && i === 0} t={{ ...r, decimals: 9, priceUsd: null }} meta={r.name} onOpen={() => open({ ...r, decimals: 9, priceUsd: null })} />
          ))}
        </div>
      </section>
    );
  }
  const rows = (res.data ?? []).slice(0, limit);
  return (
    <section>
      <SectionHead title="Coins" note={rows.length ? String(rows.length) : undefined} className="mb-[0.5rem]" />
      {rows.length > 0 ? (
        <div data-nav-group={group}>
          {rows.map((t, i) => (
            <TokenListRow key={t.coinType} idx={i} def={first && i === 0} t={t} onOpen={() => open(t)} />
          ))}
        </div>
      ) : (
        <Quiet className="py-[1rem] text-left">{res.loading || !res.data ? (res.error ? "Could not search coins right now." : "Searching") : "No coin matches. Paste a full coin type to open one that is not listed."}</Quiet>
      )}
    </section>
  );
}
