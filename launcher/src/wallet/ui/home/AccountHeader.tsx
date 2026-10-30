// Profile header (market/web/components/profile/header.tsx): picture, name, address and kind on the
// left; total value with the 24h move on the right.

import { Check, Copy } from "lucide-react";
import type { Portfolio } from "../../data/balances";
import type { RareAccount } from "../../index";
import { fmtUsd } from "../../../marketplace/format";
import { Delta, WalletAvatar, cn, useCopy } from "../kit";
import { Addr } from "../kit";
import { fmtAmount, isSuiType, kindLabel } from "./lib";

export function CopyAddress({ address, className }: { address: string; className?: string }) {
  const [done, copy] = useCopy();
  return (
    <button data-nav onClick={() => copy(address)} aria-label="Copy address" className={cn("inline-flex h-8 items-center gap-1.5 rounded px-2 text-sm text-secondary transition-colors focus-visible:text-primary", className)}>
      <Addr address={address} />
      {done ? <Check size="0.9rem" /> : <Copy size="0.9rem" />}
      {done && <span>Copied</span>}
    </button>
  );
}

export function AccountHeader({ account, pf }: { account: RareAccount; pf: Portfolio | undefined }) {
  const total = pf?.totalUsd ?? null;
  const change = pf?.change24hUsd ?? null;
  const before = total != null && change != null ? total - change : null;
  const pct = before && before > 0 && change != null ? (change / before) * 100 : null;
  const sui = pf?.tokens.find((t) => isSuiType(t.coinType));
  return (
    <div className="mb-5 flex items-center gap-5">
      <WalletAvatar address={account.address} size="4.5rem" />
      <div className="min-w-0">
        <h1 className="truncate text-2xl font-bold leading-[1.3]">{account.label}</h1>
        <div className="-ml-2 mt-0.5 flex items-center gap-3">
          <CopyAddress address={account.address} />
          <span className="text-sm text-secondary">{kindLabel(account)}</span>
        </div>
      </div>
      <div className="ml-auto text-right">
        <div className="text-4xl font-bold leading-[1.2] tabular-nums">{pf ? (total != null ? fmtUsd(total) : sui ? `${fmtAmount(sui.amount, 4)} SUI` : "$0.00") : "--"}</div>
        <div className="mt-1 flex items-center justify-end gap-3 text-sm tabular-nums">
          {change != null && pct != null && (
            <span className={cn("inline-flex items-center gap-2", change > 0 ? "text-positive" : change < 0 ? "text-negative" : "text-secondary")}>
              <span className="font-medium">
                {change > 0 ? "+" : change < 0 ? "-" : ""}
                {fmtUsd(Math.abs(change))}
              </span>
              <Delta value={pct} dp={2} />
              <span className="text-secondary">24h</span>
            </span>
          )}
          {sui && total != null && <span className="text-secondary">{fmtAmount(sui.amount, 4)} SUI</span>}
          {total == null && pf?.priced === false && <span className="text-secondary">Prices unavailable</span>}
        </div>
      </div>
    </div>
  );
}

