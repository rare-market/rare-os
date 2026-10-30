// Small shared pieces: copy-a-value button, coin info lookups for detail pages, secret errors.

import { Check, Copy } from "lucide-react";
import { normalizeStructTag } from "@mysten/sui/utils";
import { useEffect, useState } from "react";
import { grpcClient } from "../../grpc";
import { trade } from "../../data/prices";
import { NativeError } from "../../native";
import { cn, useCopy } from "../kit";

export const normType = (t: string) => {
  try {
    return normalizeStructTag(t);
  } catch {
    return t;
  }
};

/** A value in mono with a copy button (digests, addresses). */
export function CopyValue({ value, shown, label = "Copy" }: { value: string; shown?: string; label?: string }) {
  const [done, copy] = useCopy();
  return (
    <button data-nav onClick={() => copy(value)} aria-label={label} className={cn("inline-flex h-8 items-center gap-1.5 rounded px-2 font-mono text-sm transition-colors focus-visible:bg-layer-03")}>
      <span>{shown ?? value}</span>
      {done ? <Check size="0.9rem" /> : <Copy size="0.9rem" className="text-secondary" />}
    </button>
  );
}

export interface CoinInfo {
  symbol: string;
  decimals: number;
  logo: string | null;
  price: number | null;
  known: boolean;
}

/** Symbol, decimals, logo and USD price for coin types: the Trade API first, then the chain's metadata. */
export function useCoinInfo(types: string[], hints: Array<{ coinType: string; symbol?: string; decimals?: number; logo?: string }> = []): Record<string, CoinInfo> {
  const key = [...new Set(types)].sort().join(",");
  const [info, setInfo] = useState<Record<string, CoinInfo>>({});
  useEffect(() => {
    let live = true;
    const list = key ? key.split(",") : [];
    void (async () => {
      const out: Record<string, CoinInfo> = {};
      const y = await trade.tokens(list).catch(() => ({}) as Awaited<ReturnType<typeof trade.tokens>>);
      await Promise.all(
        list.map(async (t) => {
          const m = y[t];
          const h = hints.find((x) => normType(x.coinType) === normType(t));
          if (m) {
            out[t] = { symbol: m.symbol, decimals: m.decimals, logo: m.logo ?? h?.logo ?? null, price: m.priceUsd, known: true };
            return;
          }
          if (h?.decimals != null) {
            out[t] = { symbol: h.symbol ?? t.split("::").pop() ?? "?", decimals: h.decimals, logo: h.logo ?? null, price: null, known: true };
            return;
          }
          const meta = await grpcClient.core.getCoinMetadata({ coinType: t }).then((r) => r.coinMetadata).catch(() => null);
          out[t] = { symbol: meta?.symbol ?? t.split("::").pop() ?? "?", decimals: meta?.decimals ?? 9, logo: meta?.iconUrl ?? null, price: null, known: !!meta };
        }),
      );
      if (live) setInfo(out);
    })();
    return () => {
      live = false;
    };
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
  return info;
}

/** Plain message for a wrong or throttled PIN. */
export function secretError(e: unknown, noun = "PIN"): string {
  if (e instanceof NativeError) {
    if (e.code === "wrong") {
      const ms = Number(e.data?.retryAfterMs ?? 0);
      return ms > 1000 ? `Wrong ${noun}. Wait ${Math.ceil(ms / 1000)} seconds and try again.` : `Wrong ${noun}. Try again.`;
    }
    if (e.code === "throttled") return `Too many tries. Wait ${Math.ceil(Number(e.data?.retryAfterMs ?? 0) / 1000)} seconds.`;
    if (e.code === "locked") return "The wallet is locked.";
  }
  return e instanceof Error && e.message.length < 140 ? e.message : "That didn't work. Try again.";
}
