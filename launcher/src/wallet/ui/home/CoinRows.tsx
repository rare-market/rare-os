// Coin table in the profile portfolio's style: rows are focusable buttons.

import type { PortfolioToken } from "../../data/balances";
import { fmtUsd } from "../../../marketplace/format";
import { Avatar, Delta, cn } from "../kit";
import { fmtAmount } from "./lib";

export const COIN_COLS = "grid-cols-[minmax(0,1fr)_12rem_9rem_7rem]";

export function CoinHead() {
  return (
    <div className={cn("trow grid h-10 items-center px-3 text-right text-sm text-secondary", COIN_COLS)}>
      <span className="text-left font-medium">Coin</span>
      <span className="px-3 font-medium">Amount</span>
      <span className="px-3 font-medium">Value</span>
      <span className="px-3 pr-4 font-medium">24h</span>
    </div>
  );
}

export function CoinRow({ t, onOpen, idx }: { t: PortfolioToken; onOpen: () => void; idx?: number }) {
  return (
    <button data-nav data-idx={idx} onClick={onOpen} className={cn("trow grid h-16 w-full items-center rounded-lg px-3 text-right text-sm transition-colors focus-visible:bg-layer-03", COIN_COLS)}>
      <span className="flex min-w-0 items-center gap-3 text-left">
        <Avatar src={t.iconUrl} label={t.symbol} />
        <span className="min-w-0">
          <span className="block truncate text-base font-semibold">{t.symbol}</span>
          {t.name && <span className="block truncate text-xs text-secondary">{t.name}</span>}
        </span>
      </span>
      <span className="px-3 tabular-nums">{fmtAmount(t.amount)}</span>
      <span className="px-3 font-semibold tabular-nums">{t.valueUsd != null ? fmtUsd(t.valueUsd) : null}</span>
      <span className="px-3 pr-4 font-medium">
        <Delta value={t.change24h} />
      </span>
    </button>
  );
}
