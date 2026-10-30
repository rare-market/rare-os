// A wallet's NFTs (Your NFTs, holders, search results), in rare.market's item card grid.

import { useRef } from "react";
import { native } from "../lib/bridge";
import { RARE } from "../lib/rare";
import { paths, usePaged, type ItemCard } from "./api";
import { fmtInt, shortAddr } from "./format";
import { useMarketNav } from "./nav";
import { H, useLandingFocus, usePageHints } from "./page";
import { Btn, Empty, ItemCardView, WalletAvatar, nearEnd } from "./parts";

export function AccountPage({ address, label, frame, active }: { address: string; label: string; frame: number; active: boolean }) {
  const push = useMarketNav((s) => s.push);
  const root = useRef<HTMLDivElement>(null);
  const items = usePaged<ItemCard>(paths.accountItems(address));
  useLandingFocus(root, active, items.ready);
  usePageHints(active, [H.open, H.search, H.back]);
  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto px-10 pb-10 pt-2">
      <div className="mb-6 flex items-center gap-3">
        <WalletAvatar address={address} size="3.5rem" />
        <div className="min-w-0">
          <h1 className="truncate text-xl font-semibold">{label}</h1>
          <p className="font-mono text-sm text-secondary">
            {shortAddr(address)}
            {items.total != null && <span className="font-sans">, {fmtInt(items.total)} items</span>}
          </p>
        </div>
        <div className="ml-auto">
          <Btn size="md" onClick={() => void native.openWeb(`${RARE}/u/${address}`)}>
            Open on rare.market
          </Btn>
        </div>
      </div>
      <div data-nav-group={`mk-acct-${frame}`} className="grid grid-cols-6 gap-3" onFocus={nearEnd(items.items.length, 6, items.more)}>
        {items.items.map((it, i) => (
          <ItemCardView key={it.id} card={it} idx={i} def={i === 0} showCollection className="cv-cell" onOpen={() => push({ kind: "item", id: it.id, card: it, thumb: 400 })} />
        ))}
      </div>
      {items.ready && items.items.length === 0 && <Empty title="No NFTs in this wallet" />}
      {items.error && !items.ready && <Empty title="Could not load this wallet" />}
    </div>
  );
}
