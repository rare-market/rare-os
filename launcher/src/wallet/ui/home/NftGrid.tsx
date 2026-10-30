// Collectibles as rare.market's item grid. In select mode A toggles items (to send several).

import { Check } from "lucide-react";
import type { ItemCard } from "../../data/nfts";
import type { ItemCard as MarketCard } from "../../../marketplace/api";
import { ItemCardView, nearEnd } from "../../../marketplace/parts";
import { cn } from "../kit";

export function NftGrid({ items, selecting, selected, onToggle, onOpen, cols = 6, more }: { items: ItemCard[]; selecting: boolean; selected: Set<string>; onToggle: (id: string) => void; onOpen: (id: string) => void; cols?: number; more?: () => void }) {
  return (
    <div data-nav-group="wallet-nfts" className="grid grid-cols-6 gap-3" onFocus={more ? nearEnd(items.length, cols, more) : undefined}>
      {items.map((it, i) => {
        const on = selected.has(it.id);
        return (
          <div key={it.id} className="relative">
            <ItemCardView
              card={it as unknown as MarketCard}
              idx={i}
              showCollection
              className={cn("w-full", on && "outline outline-2 -outline-offset-2 outline-primary")}
              onOpen={() => (selecting ? onToggle(it.id) : onOpen(it.id))}
            />
            {selecting && (
              <span className={cn("pointer-events-none absolute left-2 top-2 flex size-6 items-center justify-center rounded-full border", on ? "border-primary bg-primary text-background" : "border-border-light bg-background/70")}>
                {on && <Check size="0.95rem" strokeWidth={3} />}
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
}
