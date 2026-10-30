// The cross-game inventory, as the Collectibles tab of Rare Wallet: one list of everything the
// player holds across Sui games, a section per game with its floor value, each item with its
// price (or Not listed), floor and last sale. A opens the item sheet.

import { useEffect } from "react";
import { fmtInt, fmtSui } from "../marketplace/format";
import { ItemCardView } from "../marketplace/parts";
import type { ItemCard } from "../marketplace/api";
import { Sui, Thumb } from "../ui";
import { Empty } from "../wallet/ui/kit";
import { LoadBar } from "../wallet/ui/home/lib";
import { cn } from "../wallet/ui/kit";
import { openItemSheet, refreshInventory, useInventory } from "./store";
import { applyPending, isStaked, totalFloorMist, unstakeLine } from "./logic";
import type { InvItem } from "./types";

export function InventoryView({ address, epoch, selecting, selected, onToggle }: { address: string; epoch: number; selecting: boolean; selected: Set<string>; onToggle: (id: string) => void }) {
  const base = useInventory((s) => s.base);
  const pending = useInventory((s) => s.pending);
  const loading = useInventory((s) => s.loading);
  const error = useInventory((s) => s.error);
  const owner = useInventory((s) => s.address);

  useEffect(() => {
    void refreshInventory(address);
  }, [address, epoch]);

  const inv = base && owner === address ? applyPending(base, pending) : null;
  let idx = 0;
  return (
    <div className="pt-4">
      <LoadBar on={loading} />
      {inv && inv.total > 0 && (
        <p className="mb-4 text-sm text-secondary">
          {fmtInt(inv.total)} {inv.total === 1 ? "item" : "items"}, floor value <span className="tabular-nums text-primary">{fmtSui(totalFloorMist(inv), { dp: 2, trim: true })} SUI</span>
        </p>
      )}
      {inv?.groups.map((g) => (
        <section key={g.game?.slug ?? "other"} className="mb-8">
          <div className="mb-3 flex items-center gap-3">
            {g.game && <Thumb src={g.game.logoUrl || null} label={g.game.name} className="size-8" />}
            <h2 className="text-base font-semibold">{g.game?.name ?? "Other collectibles"}</h2>
            <span className="text-sm text-secondary">
              {g.items.length} {g.items.length === 1 ? "item" : "items"}
              {BigInt(g.floorValueMist) > 0n && <>, floor value <Sui mist={g.floorValueMist} dp={2} /></>}
            </span>
          </div>
          <div data-nav-group={`inv-${g.game?.slug ?? "other"}`} className="grid grid-cols-6 gap-3">
            {g.items.map((it: InvItem) => {
              const on = selected.has(it.id);
              const i = idx++;
              return (
                <div key={it.id} className="min-w-0">
                  <ItemCardView
                    card={it as ItemCard}
                    idx={i}
                    def={i === 0}
                    className={cn("w-full", on && "outline outline-2 -outline-offset-2 outline-primary")}
                    onOpen={() => (selecting && !isStaked(it) ? onToggle(it.id) : selecting ? undefined : openItemSheet(it.id))}
                  />
                  <p className="mt-1 truncate px-1 text-label text-secondary">Floor {it.floorMist ? fmtSui(it.floorMist, { dp: 2, trim: true }) : "--"}</p>
                  {isStaked(it) && <p data-testid="staked-hint" className="px-1 text-label text-secondary">{unstakeLine(it)}</p>}
                </div>
              );
            })}
          </div>
        </section>
      ))}
      {inv && inv.total === 0 && <Empty title="No collectibles in this wallet">Items from Sui games show up here, grouped by game.</Empty>}
      {error && !inv && <Empty title="Could not load your inventory">Check the connection and try again.</Empty>}
    </div>
  );
}
