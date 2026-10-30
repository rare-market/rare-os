// Top traders: the ledger's 7 and 30 day boards by realized P/L or win rate, with a minimum of
// activity and wash or bot-like wallets left out. Followed traders and their latest trades sit on
// the right. X switches 7 and 30 days; A opens a trader.

import { useEffect, useMemo, useRef } from "react";
import { create } from "zustand";
import { Dot, HeadRow, Panel, Row, SectionHead, Spark } from "../../ui";
import { useApi } from "../api";
import { useLandingFocus, useSectionButtons, useTradeNav } from "../nav";
import { CoinLogo, Num, Quiet, TfChips, cn } from "../parts";
import { useTradeAccount } from "../wallet";
import { RarelingLabel } from "../../perks/ui";
import { useRarelingHolders } from "../../perks/store";
import { traderPaths, type BoardExtras, type BoardWindow, type LeaderRow } from "./api";
import { MIN_TRADES, MIN_VOLUME_USD, linkedSizes, styleWords, topTraders, type BoardSort } from "./board";
import { isFollowing, useFollows } from "./follow";
import { useFollowFeed } from "./feed";
import { TraderAvatar, TraderLabel, ago, pct } from "./parts";

export const useBoardPrefs = create<{ window: BoardWindow; sort: BoardSort }>(() => ({ window: "7d", sort: "pnl" }));

const WINDOWS: Array<{ value: BoardWindow; label: string }> = [
  { value: "7d", label: "7 days" },
  { value: "30d", label: "30 days" },
];
const SORTS: Array<{ value: BoardSort; label: string }> = [
  { value: "pnl", label: "Realized P/L" },
  { value: "winrate", label: "Win rate" },
];
const COLS = "grid-cols-[2rem_minmax(0,1fr)_7.5rem_5rem_4.5rem_6rem_5.5rem] text-right";

export function TradersSection({ frame, active }: { frame: number; active: boolean }) {
  const push = useTradeNav((s) => s.push);
  const { account } = useTradeAccount();
  const root = useRef<HTMLDivElement>(null);
  const { window: win, sort } = useBoardPrefs();
  const follows = useFollows((s) => s.list);
  const recent = useFollowFeed((s) => s.recent);
  const holders = useRarelingHolders((s) => s.held);
  useEffect(() => useFollows.getState().use(account?.address ?? null), [account?.address]);

  const board = useApi<{ rows: LeaderRow[] }>(traderPaths.board(win), 60_000, 60_000);
  const extras = useApi<BoardExtras>(traderPaths.extras(win), 5 * 60_000);
  const teams = useApi<{ rows: LeaderRow[] }>(traderPaths.teams(), 10 * 60_000);
  const rows = useMemo(() => topTraders(board.data?.rows ?? [], extras.data, { sort, teams: linkedSizes(teams.data?.rows) }), [board.data, extras.data, teams.data, sort]);
  const sig = rows.slice(0, 25).map((r) => r.address).join(",");
  useEffect(() => {
    if (sig) void useRarelingHolders.getState().scan(sig.split(","));
  }, [sig]);

  useLandingFocus(root, active, rows.length > 0);
  useSectionButtons((b) => {
    if (b !== "x") return false;
    useBoardPrefs.setState({ window: win === "7d" ? "30d" : "7d" });
    return true;
  });

  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto pb-[2.5rem]">
      <div className="mx-[2.5rem] mb-[1rem] flex items-center gap-[1rem]">
        <TfChips items={WINDOWS} value={win} onChange={(v) => useBoardPrefs.setState({ window: v })} group={`tr-traders-win-${frame}`} />
        <TfChips items={SORTS} value={sort} onChange={(v) => useBoardPrefs.setState({ sort: v })} group={`tr-traders-sort-${frame}`} />
        <p className="ml-auto text-label text-secondary">{`At least ${MIN_TRADES} trades and $${(MIN_VOLUME_USD / 1000).toFixed(0)}K volume. Wash and bot-like wallets left out.`}</p>
      </div>

      <div className="mx-[2.5rem] grid grid-cols-[minmax(0,1fr)_22rem] gap-[1.25rem]">
        <Panel className="min-w-0 p-[1rem]">
          <HeadRow cols={COLS}>
            <span className="text-left">#</span>
            <span className="text-left">Trader</span>
            <span>Realized P/L</span>
            <span>Win rate</span>
            <span>Trades</span>
            <span>Volume</span>
            <span>30 days</span>
          </HeadRow>
          <div data-nav-group={`tr-traders-${frame}`}>
            {rows.map((t, i) => {
              const r = t.row;
              const words = [...styleWords(t.tags).slice(0, 2), ...(t.linked ? [`Linked to ${t.linked - 1} ${t.linked === 2 ? "wallet" : "wallets"}`] : [])];
              return (
                <Row key={t.address} cols={COLS} idx={i} hint="Open" onClick={() => push({ kind: "trader", address: t.address })} className="cv-row !h-[3.75rem]">
                  <span className="text-left tabular-nums text-secondary">{i + 1}</span>
                  <span className="flex min-w-0 items-center gap-[0.75rem] text-left">
                    <TraderAvatar user={r.user} size="2.25rem" />
                    <span className="min-w-0">
                      <span className="flex items-center gap-[0.5rem]">
                        <span className="truncate text-body font-semibold"><TraderLabel user={r.user} /></span>
                        {isFollowing(follows, t.address) && <Dot tone="primary" />}
                      </span>
                      <span className="flex min-w-0 items-center gap-[0.75rem] truncate text-label text-secondary">
                        {holders.has(t.address) && <RarelingLabel />}
                        {words.length ? <span className="truncate">{words.join(", ")}</span> : !holders.has(t.address) && <TopCoins row={r} />}
                      </span>
                    </span>
                  </span>
                  <Num kind="pnl" value={r.pnlUsd} className="font-semibold" />
                  <span className="tabular-nums">{pct(r.winRate)}</span>
                  <span className="tabular-nums text-secondary">{r.trades.toLocaleString("en-US")}</span>
                  <Num kind="market" value={r.volumeUsd} className="text-secondary" />
                  <span className="flex justify-end">{t.curve.length > 1 && <Spark points={t.curve.map((v, k) => ({ t: k, v }))} className="h-[1.5rem] w-[5rem]" />}</span>
                </Row>
              );
            })}
          </div>
          {!rows.length && <Quiet>{board.error ? "Top traders are unavailable right now. This page refreshes on its own." : board.data ? "No trader passes the activity minimum yet." : ""}</Quiet>}
        </Panel>

        <div className="flex min-w-0 flex-col gap-[1.25rem]">
          <Panel className="p-[1rem]">
            <SectionHead title="Following" note={follows.length ? String(follows.length) : undefined} className="mb-[0.5rem] px-[0.75rem] pt-[0.25rem]" />
            {follows.length ? (
              <div data-nav-group={`tr-following-${frame}`}>
                {follows.map((f) => (
                  <button key={f.address} data-nav data-hint-a="Open" onClick={() => push({ kind: "trader", address: f.address })} className="cv-row flex h-[3rem] w-full items-center gap-[0.75rem] rounded-card px-[0.75rem] text-left">
                    <TraderAvatar user={{ address: f.address, avatarUrl: null, username: null, displayName: null }} size="1.75rem" />
                    <span className="min-w-0 flex-1 truncate text-body font-semibold">{f.name}</span>
                  </button>
                ))}
              </div>
            ) : (
              <p className="px-[0.75rem] pb-[0.5rem] text-small text-secondary">Follow a trader to get their trades as they happen.</p>
            )}
          </Panel>
          {recent.length > 0 && (
            <Panel className="p-[1rem]">
              <SectionHead title="Their latest trades" className="mb-[0.5rem] px-[0.75rem] pt-[0.25rem]" />
              <div className="px-[0.75rem]">
                {recent.slice(0, 8).map((e) => (
                  <div key={e.trade.id} className="flex h-[2.75rem] items-center gap-[0.6rem] border-b border-hair text-small last:border-b-0">
                    <CoinLogo src={e.token?.logo} symbol={e.token?.symbol ?? "?"} size="1.5rem" />
                    <span className="min-w-0 flex-1 truncate">
                      <span className="font-semibold">{e.trader.name}</span> <span className={cn(e.trade.buy ? "text-positive" : "text-negative")}>{e.trade.buy ? "bought" : "sold"}</span> {e.token?.symbol ?? ""}
                    </span>
                    <Num kind="money" value={e.trade.usd} className="tabular-nums" />
                    <span className="w-[2.25rem] text-right text-label text-secondary">{ago(e.trade.timeMs)}</span>
                  </div>
                ))}
              </div>
            </Panel>
          )}
        </div>
      </div>
    </div>
  );
}

/** The coins that made a trader the most, as small logos (when the ledger has no style words). */
function TopCoins({ row }: { row: LeaderRow }) {
  if (!row.topTokens.length) return null;
  return (
    <span className="flex items-center gap-[0.35rem]">
      {row.topTokens.slice(0, 3).map((t) => (
        <span key={t.coinType} className="inline-flex items-center gap-[0.25rem]">
          <CoinLogo src={t.logo} symbol={t.symbol} size="1rem" />
          {t.symbol}
        </span>
      ))}
    </span>
  );
}
