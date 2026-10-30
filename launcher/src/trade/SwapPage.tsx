// Swap any coin for any coin through the router: pay and receive boxes (A picks the coin), LT/RT
// step the amount, X flips the pair, A on the button opens the review.

import { ArrowDownUp } from "lucide-react";
import { useRef, useState } from "react";
import { Glyph, Panel, numText } from "../ui";
import { restoreLater, spot, type Spot } from "../nav/focus";
import { SUI_TYPE, USDC_TYPE, baseToNumber, fromBase, minOut, normalizeType } from "./amount";
import { invalidate, tokenPaths, useApi, type Portfolio, type TokenRow } from "./api";
import { H, useLandingFocus, usePageButtons, usePageHints, useSheetPad } from "./nav";
import { Action, Chip, CoinLogo, Line, Num, Quiet, Sheet, cn } from "./parts";
import { useCoinSearch } from "./CoinSearch";
import { IconSearch } from "../components/icons";
import { askText } from "../components/Osk";
import { ReviewSheet } from "./ReviewSheet";
import { buildSwap, quoteSwap, venueName } from "./swap";
import { useTicket, type Coin } from "./ticket";
import { useTradeAccount } from "./wallet";

const asCoin = (t: TokenRow): Coin => ({ coinType: normalizeType(t.coinType), symbol: t.symbol, decimals: t.decimals, logo: t.logo, priceUsd: t.priceUsd });
const trim = (v: number) => Number(v.toPrecision(6)).toLocaleString("en-US", { maximumFractionDigits: 6 });

export function SwapPage({ from: from0, to: to0, frame, active }: { from?: string; to?: string; frame: number; active: boolean }) {
  const root = useRef<HTMLDivElement>(null);
  const { account } = useTradeAccount();
  const list = useApi<{ tokens: TokenRow[] }>(tokenPaths.pickList(60), 60_000);
  const tokens = list.data?.tokens ?? [];
  const find = (t: string | undefined) => (t ? tokens.find((x) => normalizeType(x.coinType) === normalizeType(t)) : undefined);
  const [fromType, setFrom] = useState(normalizeType(from0 ?? SUI_TYPE));
  const [toType, setTo] = useState(normalizeType(to0 ?? USDC_TYPE));
  const [picking, setPickingState] = useState<"from" | "to" | null>(null);
  // The coin button that opened the picker gets focus back when it closes.
  const opener = useRef<Spot | null>(null);
  const setPicking = (v: "from" | "to" | null) => {
    if (v) opener.current = spot();
    else restoreLater(opener.current);
    setPickingState(v);
  };
  const [reviewing, setReviewing] = useState(false);
  const fromRow = find(fromType);
  const toRow = find(toType);
  const extra = useApi<TokenRow>(!fromRow && tokens.length ? tokenPaths.lean(fromType) : null, 60_000);
  const extraTo = useApi<TokenRow>(!toRow && tokens.length ? tokenPaths.lean(toType) : null, 60_000);
  const from = fromRow ? asCoin(fromRow) : extra.data ? asCoin(extra.data) : null;
  const to = toRow ? asCoin(toRow) : extraTo.data ? asCoin(extraTo.data) : null;
  const t = useTicket({ from, to, owner: account?.address ?? null, mode: "units", initial: fromType === SUI_TYPE ? 5 : 10 });

  const flip = () => {
    setFrom(toType);
    setTo(fromType);
  };
  const busy = !!picking || reviewing;
  useLandingFocus(root, active && !busy, !!from && !!to);
  usePageButtons(frame, (b) => {
    if (busy) return false;
    if (b === "lt" || b === "rt") {
      t.bump(b === "rt" ? 1 : -1);
      return true;
    }
    if (b === "x") {
      flip();
      return true;
    }
    return false;
  });
  usePageHints(active, [H.select, { b: "lt", label: "Less" }, { b: "rt", label: "More" }, { b: "x", label: "Flip" }, H.back]);

  const out = t.quote && to ? baseToNumber(t.quote.amountOut, to.decimals) : null;
  const inUsd = from?.priceUsd ? baseToNumber(t.amountIn, from.decimals) * from.priceUsd : null;
  const outUsd = out != null && to?.priceUsd ? out * to.priceUsd : null;
  const blocked = !from || !to || t.amountIn <= 0n || t.short || !t.quote;
  const why = !account ? "Add a wallet first" : t.short ? "Not enough balance" : t.quoteError ?? undefined;

  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto pb-[2.5rem]">
      <div className="mx-auto w-[34rem]">
        <h1 className="text-title font-semibold">Swap</h1>
        <Panel className="mt-[1rem] p-[1.25rem]">
          <p className="text-small text-secondary">You pay</p>
          <div className="mt-[0.5rem] flex items-center gap-[0.75rem]">
            <CoinButton coin={from} onClick={() => setPicking("from")} />
            <Glyph b="lt" />
            <p className="min-w-0 flex-1 truncate text-right text-display font-semibold tabular-nums">{trim(t.value)}</p>
            <Glyph b="rt" />
          </div>
          <p className="mt-[0.25rem] flex justify-between text-small text-secondary">
            <span>{from && t.balance != null ? `Balance ${numText("amount", baseToNumber(t.balance, from.decimals))} ${from.symbol}` : ""}</span>
            <span>{inUsd != null ? numText("money", inUsd) : ""}</span>
          </p>
          <div data-nav-group="tr-swap-presets" data-nav-remember className="mt-[0.75rem] flex gap-[0.5rem]">
            {t.presets.map((v) => (
              <Chip key={v} on={Math.abs(t.value - v) < 1e-9} onClick={() => t.set(v)}>
                {trim(v)}
              </Chip>
            ))}
            {t.max !== Infinity && t.max > 0 && (
              <Chip on={Math.abs(t.value - t.max) < 1e-9} onClick={() => t.set(Number(t.max.toPrecision(8)))}>
                Max
              </Chip>
            )}
          </div>
        </Panel>

        <div className="relative z-10 -my-[0.75rem] flex justify-center">
          <button data-nav data-hint-a="Flip" onClick={flip} aria-label="Flip" className="flex size-[2.75rem] items-center justify-center rounded-full border-[0.25rem] border-background bg-l3">
            <ArrowDownUp size="1.1rem" />
          </button>
        </div>

        <Panel className="p-[1.25rem]">
          <p className="text-small text-secondary">You get about</p>
          <div className="mt-[0.5rem] flex items-center gap-[0.75rem]">
            <CoinButton coin={to} onClick={() => setPicking("to")} />
            <p className={cn("min-w-0 flex-1 truncate text-right text-display font-semibold tabular-nums", out == null && "text-secondary")}>{out != null ? numText("amount", out) : t.quoting ? "..." : ""}</p>
          </div>
          <p className="mt-[0.25rem] text-right text-small text-secondary">{outUsd != null ? numText("money", outUsd) : t.quoteError ? "No route for this amount" : ""}</p>
        </Panel>

        {t.quote && to && (
          <div className="mt-[1rem] px-[0.25rem]">
            <Line label="Minimum received">{`${numText("amount", Number(fromBase(minOut(t.quote.amountOut), to.decimals)))} ${to.symbol}`}</Line>
            <Line label="Route">{`${t.quote.hops} ${t.quote.hops === 1 ? "hop" : "hops"}${t.quote.venues.length ? ` via ${[...new Set(t.quote.venues.map(venueName))].slice(0, 3).join(", ")}` : ""}`}</Line>
            <Line label="Fee">{`${(t.quote.feeBps / 100).toFixed(2)}%`}</Line>
          </div>
        )}

        <Action def tone="primary" disabled={blocked} onClick={() => setReviewing(true)} hint={blocked ? why : "Review"} className="mt-[1rem] w-full">
          {from && to ? `Review swap` : "Swap"}
        </Action>
        {why && !t.quoting && <p className="mt-[0.5rem] text-center text-label text-secondary">{why}</p>}
      </div>

      {picking && (
        <TokenPicker
          title={picking === "from" ? "Pay with" : "Receive"}
          tokens={tokens}
          owner={account?.address ?? null}
          onPick={(c) => {
            const other = picking === "from" ? toType : fromType;
            if (normalizeType(c) === other) flip();
            else if (picking === "from") setFrom(normalizeType(c));
            else setTo(normalizeType(c));
            setPicking(null);
          }}
          onClose={() => setPicking(null)}
        />
      )}
      {reviewing && from && to && (
        <ReviewSheet
          title={`Swap ${from.symbol} to ${to.symbol}`}
          account={account}
          action="Confirm swap"
          sponsor="trade"
          build={async (sender, o) => {
            const q = t.quote && t.quote.expiresAt - Date.now() > 8_000 ? t.quote : await quoteSwap({ coinIn: from.coinType, coinOut: to.coinType, amountIn: t.amountIn, owner: sender });
            return (await buildSwap(q, sender, undefined, undefined, { sponsored: o.sponsored })).tx;
          }}
          onClose={() => setReviewing(false)}
          onSent={() => invalidate("/ext/portfolio")}
        >
          <Line label="You pay">{`${numText("amount", baseToNumber(t.amountIn, from.decimals))} ${from.symbol}`}</Line>
          {t.quote && (
            <>
              <Line label="You get about" strong>{`${numText("amount", baseToNumber(t.quote.amountOut, to.decimals))} ${to.symbol}`}</Line>
              <Line label="Minimum received">{`${numText("amount", Number(fromBase(minOut(t.quote.amountOut), to.decimals)))} ${to.symbol}`}</Line>
              <Line label="Fee">{`${(t.quote.feeBps / 100).toFixed(2)}%`}</Line>
              <Line label="Slippage">1%</Line>
            </>
          )}
        </ReviewSheet>
      )}
    </div>
  );
}

function CoinButton({ coin, onClick }: { coin: Coin | null; onClick: () => void }) {
  return (
    <button data-nav data-hint-a="Change coin" onClick={onClick} className={`flex h-[3rem] shrink-0 items-center gap-[0.5rem] rounded-full bg-l3 pr-[1rem] ${coin ? "pl-[0.375rem]" : "pl-[1rem]"}`}>
      {coin && <CoinLogo src={coin.logo} symbol={coin.symbol} size="2.25rem" />}
      <span className="text-body font-semibold">{coin?.symbol ?? "Choose a coin"}</span>
    </button>
  );
}

/** Coins to pick from: what the wallet holds first, then the most traded. */
function TokenPicker({ title, tokens, owner, onPick, onClose }: { title: string; tokens: TokenRow[]; owner: string | null; onPick: (coinType: string) => void; onClose: () => void }) {
  const root = useRef<HTMLDivElement | null>(null);
  const portfolio = useApi<Portfolio>(owner ? tokenPaths.portfolio(owner) : null, 30_000);
  useSheetPad(onClose, [{ b: "a", label: "Choose" }, { b: "b", label: "Close" }]);
  useLandingFocus(root, true, tokens.length);
  const held = new Map((portfolio.data?.holdings ?? []).map((h) => [normalizeType(h.coinType), h]));
  const mine = (portfolio.data?.holdings ?? []).filter((h) => h.valueUsd >= 0.01).map((h): TokenRow => ({ ...h, change24h: h.change24h }));
  const seen = new Set(mine.map((m) => normalizeType(m.coinType)));
  const [q, setQ] = useState("");
  const found = useCoinSearch(q);
  const rows = q.trim() ? (found.data ?? []) : [...mine, ...tokens.filter((t) => !seen.has(normalizeType(t.coinType)))];
  return (
    <Sheet title={title} onRef={(el) => void (root.current = el)}>
      <div data-nav-group="tr-picker-search" className="mb-[0.75rem]">
        <button data-nav data-hint-a="Search coins" onClick={() => void askText("Search any coin by name, ticker or coin type", q, { placeholder: "Name, ticker or 0x..::mod::NAME" }).then((v) => v !== null && setQ(v.trim()))} className="flex h-[3rem] w-full items-center gap-[0.6rem] rounded-chip border border-border-light bg-l1 px-[0.9rem] text-left text-body">
          <IconSearch size="1.2rem" className="text-icon-2" />
          {q || <span className="text-muted">Search any coin by name, ticker or coin type</span>}
        </button>
      </div>
      {q.trim() && rows.length === 0 && <Quiet className="py-[1rem]">{found.loading || !found.data ? (found.error ? "Could not search coins right now." : "Searching") : "No coin matches. Paste a full coin type to use one that is not listed."}</Quiet>}
      <div data-nav-group="tr-picker" className="-mx-[0.75rem]">
        {rows.map((r, i) => {
          const h = held.get(normalizeType(r.coinType));
          return (
            <button key={r.coinType} data-nav data-nav-default={i === 0 ? "" : undefined} onClick={() => onPick(r.coinType)} className="cv-row flex h-[3.5rem] w-full items-center gap-[0.75rem] rounded-card px-[0.75rem] text-left">
              <CoinLogo src={r.logo} symbol={r.symbol} />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-body font-semibold">{r.symbol}</span>
                <span className="block truncate text-label text-secondary">{r.name}</span>
              </span>
              <span className="text-right text-small">
                {h ? (
                  <>
                    <span className="block tabular-nums">{numText("amount", h.amount)}</span>
                    <Num kind="money" value={h.valueUsd} className="block text-label text-secondary" />
                  </>
                ) : (
                  <Num kind="price" value={r.priceUsd} className="block tabular-nums text-secondary" />
                )}
              </span>
            </button>
          );
        })}
      </div>
          </Sheet>
  );
}
