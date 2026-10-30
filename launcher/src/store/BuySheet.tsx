// Confirm sheet for a Store purchase, in the Marketplace buy sheet's shape: the price, the coin to
// pay with (USDC, or SUI at the live Pyth price when the studio takes it), then Rare Wallet's
// review sheet signs. B closes the sheet.

import { X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { feedback } from "../nav/feedback";
import { usePadHandler } from "../nav/input";
import { useLandingFocus } from "../marketplace/page";
import { GameLogo } from "../screens/SuiGames";
import { Button, fmtSui } from "../ui";
import { SignRejected } from "../wallet/market";
import type { RareAccount } from "../wallet";
import { buy, plain, type Coin } from "./actions";
import { api } from "./api";
import { priceNow, usdText } from "./logic";
import { logoOf } from "./parts";
import { suiQuote } from "./tx";
import type { StoreGame } from "./types";

type Phase = { at: "idle" } | { at: "working"; coin: Coin } | { at: "error"; message: string };

export function BuySheet({ game, account, onClose, onBought }: { game: StoreGame; account: RareAccount; onClose: () => void; onBought: () => void }) {
  const root = useRef<HTMLDivElement>(null);
  const price = priceNow(game) ?? 0n;
  const [phase, setPhase] = useState<Phase>({ at: "idle" });
  const [suiMist, setSuiMist] = useState<bigint | null>(null);
  const acceptSui = !!game.price?.accept_sui;
  useEffect(() => {
    if (!acceptSui || price === 0n) return;
    let live = true;
    void api.suiPrice().then(
      (p) => live && setSuiMist(suiQuote(price, p).mist),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [acceptSui, price]);
  useLandingFocus(root, true, phase.at);
  usePadHandler((b) => {
    if (b !== "b") return false;
    if (phase.at !== "working") {
      feedback("back");
      onClose();
    }
    return true;
  });

  const go = async (coin: Coin) => {
    if (phase.at === "working") return;
    setPhase({ at: "working", coin });
    try {
      await buy(game, coin, account);
      feedback("notify");
      onBought();
    } catch (e) {
      setPhase(e instanceof SignRejected ? { at: "idle" } : { at: "error", message: plain(e) });
    }
  };

  const working = phase.at === "working";
  return createPortal(
    <div className="fixed inset-x-0 bottom-[3rem] top-0 z-40 flex items-center justify-center p-[1.5rem]">
      <div className="absolute inset-0 bg-bg/80" aria-hidden />
      <div ref={root} data-nav-scope className="fade-in relative flex w-full max-w-[40rem] flex-col overflow-hidden rounded-sheet border border-border-light bg-bg">
        <div className="flex items-start justify-between gap-[1rem] border-b border-hair px-[2rem] pb-[1rem] pt-[1.5rem]">
          <div className="flex min-w-0 items-center gap-[0.75rem]">
            <GameLogo game={logoOf(game)} size="3rem" />
            <div className="min-w-0">
              <h2 className="text-row font-semibold">Buy {game.name}</h2>
              <p className="truncate text-small text-muted">{game.studio.name}</p>
            </div>
          </div>
          <button data-nav onClick={onClose} aria-label="Close" className="-mr-[0.25rem] mt-[0.25rem] flex size-[2.5rem] items-center justify-center rounded-chip text-icon-primary">
            <X size="1.25rem" />
          </button>
        </div>
        <div className="px-[2rem] py-[1.5rem]">
          <p className="text-small text-muted">Price</p>
          <p className="mt-[0.25rem] text-display font-semibold tabular-nums">{usdText(price)}</p>
          {acceptSui && suiMist != null && <p className="text-small text-muted tabular-nums">or {fmtSui(suiMist, { dp: 2, trim: true })} SUI at today's price</p>}
          <p className="mt-[1rem] text-small text-muted">The licence goes to your wallet. You can refund it in full within 48 hours. Network gas is extra.</p>
          {phase.at !== "idle" && (
            <p role="status" className={`mt-[0.75rem] text-small ${phase.at === "error" ? "text-negative" : "text-fg/80"}`}>
              {phase.at === "working" ? "Review and approve in Rare Wallet." : phase.message}
            </p>
          )}
        </div>
        <div className="flex gap-[0.5rem] border-t border-hair px-[2rem] py-[1rem]">
          <Button variant="primary" def className="flex-1" disabled={working} onClick={() => void go("usdc")}>
            {phase.at === "working" && phase.coin === "usdc" ? "Waiting for approval" : `Pay ${usdText(price)} in USDC`}
          </Button>
          {acceptSui && (
            <Button className="flex-1" disabled={working || suiMist == null} onClick={() => void go("sui")}>
              {phase.at === "working" && phase.coin === "sui" ? "Waiting for approval" : suiMist != null ? `Pay ${fmtSui(suiMist, { dp: 2, trim: true })} SUI` : "Pay in SUI"}
            </Button>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}
