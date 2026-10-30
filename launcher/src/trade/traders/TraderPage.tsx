// A trader's page: who they are (funding links, ledger labels, Rareling), their realized P/L curve,
// what they hold now, and their latest buys and sells as they happen. Follow keeps them on this
// device; A on a trade mirrors it at this account's size.

import { useEffect, useRef, useState } from "react";
import { HeadRow, MiniCurve, Panel, Row, SectionHead, Dot, type LinePoint } from "../../ui";
import { useApi } from "../api";
import { H, useLandingFocus, usePageButtons, usePageHints, useTradeNav } from "../nav";
import { Action, Chip, CoinLogo, Num, Quiet, StatLine, TfChips, cn } from "../parts";
import { useTradeAccount } from "../wallet";
import { RarelingLabel } from "../../perks/ui";
import { walletHoldsRareling } from "../../perks/store";
import { useRareName } from "../../friends/names";
import { traderNameWith, traderPaths, type BoardWindow, type LinkedPage, type PnlPoint, type PublicProfile, type TradesPage, type TraderStats, type TraderTrade } from "./api";
import { isFollowing, useFollows } from "./follow";
import { BUY_SIZES, SELL_SIZES, useMirrorSize } from "./mirror";
import { MirrorSheet } from "./MirrorSheet";
import { TraderAvatar, ago, labelWords, pct } from "./parts";

const WINDOWS: Array<{ value: BoardWindow; label: string }> = [
  { value: "7d", label: "7 days" },
  { value: "30d", label: "30 days" },
];
const TRADE_COLS = "grid-cols-[3rem_3.5rem_minmax(0,1fr)_6.5rem_6.5rem] text-right";

export function TraderPage({ address, frame, active }: { address: string; frame: number; active: boolean }) {
  const root = useRef<HTMLDivElement>(null);
  const push = useTradeNav((s) => s.push);
  const { account } = useTradeAccount();
  const [win, setWin] = useState<BoardWindow>("30d");
  const [mirroring, setMirroring] = useState<TraderTrade | null>(null);
  const [rareling, setRareling] = useState(false);
  const follows = useFollows((s) => s.list);
  const size = useMirrorSize((s) => s.size);
  useEffect(() => {
    useFollows.getState().use(account?.address ?? null);
    useMirrorSize.getState().use(account?.address ?? null);
  }, [account?.address]);
  useEffect(() => {
    void walletHoldsRareling(address).then(setRareling);
  }, [address]);

  const profile = useApi<PublicProfile>(traderPaths.profile(address), 5 * 60_000);
  const stats = useApi<TraderStats>(traderPaths.stats(address, win), 60_000, 60_000);
  const pnl = useApi<{ points: PnlPoint[] }>(traderPaths.pnl(address, win), 5 * 60_000);
  // Latest buys and sells, live.
  const trades = useApi<TradesPage>(traderPaths.trades(address, 25), 8_000, 8_000);
  const linked = useApi<LinkedPage>(traderPaths.linked(address), 10 * 60_000);

  const user = { address, username: profile.data?.username ?? null, displayName: profile.data?.displayName ?? null, avatarUrl: profile.data?.avatarUrl ?? null };
  const name = traderNameWith(useRareName(address), user);
  const following = isFollowing(follows, address);
  const s = stats.data;
  const points: LinePoint[] = (pnl.data?.points ?? []).map((p) => ({ t: p.time, v: p.pnlUsd }));
  const items = trades.data?.items ?? [];
  const holdings = (s?.holdings ?? []).filter((h) => (h.valueUsd ?? 0) >= 1).sort((a, b) => (b.valueUsd ?? 0) - (a.valueUsd ?? 0));
  const words = [...labelWords(s?.label), ...(linked.data?.total ? [`Linked to ${linked.data.total} ${linked.data.total === 1 ? "wallet" : "wallets"}`] : [])];

  useLandingFocus(root, active && !mirroring, !!profile.data || !!s);
  usePageButtons(frame, (b) => {
    if (mirroring) return false;
    if (b === "x") return useFollows.getState().toggle({ address, name }), true;
    return false;
  });
  usePageHints(active, [H.select, { b: "x", label: following ? "Unfollow" : "Follow" }, H.back]);

  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto pb-[2.5rem]">
      <header className="flex items-end gap-[1.25rem] px-[2.5rem]">
        <TraderAvatar user={user} size="4.5rem" />
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-title font-semibold tracking-[-0.02em]">{name}</h1>
          <div className="num truncate text-meta text-muted">{address}</div>
          <div className="mt-[0.35rem] flex flex-wrap items-center gap-x-[1rem] gap-y-[0.25rem] text-label text-secondary">
            {rareling && <RarelingLabel />}
            {words.map((w) => (
              <span key={w}>{w}</span>
            ))}
            {s?.bot && <span>Trades like a bot</span>}
          </div>
        </div>
        <div data-nav-group={`tr-trader-actions-${frame}`} data-nav-remember className="flex gap-[0.5rem]">
          <Action def size="md" tone={following ? "secondary" : "primary"} hint={following ? "Unfollow" : "Follow"} onClick={() => useFollows.getState().toggle({ address, name })}>
            {following ? "Following" : "Follow"}
          </Action>
        </div>
      </header>

      <Panel className="mx-[2.5rem] mt-[1.25rem] px-[1.25rem] py-[1rem]">
        <StatLine
          size="lg"
          items={[
            { label: "Realized P/L", value: s ? <Num kind="pnl" value={s.realized} /> : null },
            { label: "Unrealized", value: s ? <Num kind="pnl" value={s.unrealized} /> : null },
            { label: "Win rate", value: s?.winRate != null ? pct(s.winRate) : null },
            { label: "Trades", value: s ? s.trades.toLocaleString("en-US") : null },
            { label: "Volume", value: s ? <Num kind="market" value={s.volume} /> : null },
            { label: "Holdings", value: s?.holdingsUsd != null ? <Num kind="money" value={s.holdingsUsd} /> : null },
          ]}
        />
      </Panel>

      <div className="mx-[2.5rem] mt-[1.25rem] grid grid-cols-[minmax(0,1fr)_24rem] gap-[1.25rem]">
        <div className="flex min-w-0 flex-col gap-[1.25rem]">
          <Panel className="p-[1.25rem]">
            <div className="flex items-center gap-[1rem]">
              <SectionHead title="P/L" note={points.length > 1 ? undefined : pnl.error ? "Unavailable" : ""} />
              <div className="ml-auto">
                <TfChips items={WINDOWS} value={win} onChange={setWin} group={`tr-trader-win-${frame}`} />
              </div>
            </div>
            <div className="mt-[0.75rem] h-[11rem]">{points.length > 1 && <MiniCurve points={points} className="h-full w-full" />}</div>
          </Panel>

          <Panel className="p-[1rem]">
            <SectionHead
              title="Latest trades"
              right={
                <span className="inline-flex items-center gap-[0.4rem]">
                  <Dot tone="brand" live />
                  Live
                </span>
              }
              className="mb-[0.5rem] px-[0.75rem] pt-[0.25rem]"
            />
            <HeadRow cols={TRADE_COLS}>
              <span className="text-left">Age</span>
              <span className="text-left">Side</span>
              <span className="text-left">Coin</span>
              <span>Value</span>
              <span>Price</span>
            </HeadRow>
            <div data-nav-group={`tr-trader-trades-${frame}`}>
              {items.map((t, i) => {
                const tok = trades.data?.tokens?.[t.coin] ?? null;
                return (
                  <Row key={t.id} cols={TRADE_COLS} idx={i} hint={`Mirror ${t.buy ? "buy" : "sell"}`} onClick={() => setMirroring(t)} className="cv-row">
                    <span className="text-left tabular-nums text-secondary">{ago(t.timeMs)}</span>
                    <span className={cn("text-left font-semibold", t.buy ? "text-positive" : "text-negative")}>{t.buy ? "Buy" : "Sell"}</span>
                    <span className="flex min-w-0 items-center gap-[0.6rem] text-left">
                      <CoinLogo src={tok?.logo} symbol={tok?.symbol ?? "?"} size="1.75rem" />
                      <span className="truncate font-semibold">{tok?.symbol ?? t.coin.split("::").pop()}</span>
                    </span>
                    <Num kind="money" value={t.usd} />
                    <Num kind="price" value={t.priceUsd} className="text-secondary" />
                  </Row>
                );
              })}
            </div>
            {!items.length && <Quiet>{trades.error ? "Could not load trades. This page refreshes on its own." : trades.data ? "No trades yet." : ""}</Quiet>}
          </Panel>
        </div>

        <div className="flex min-w-0 flex-col gap-[1.25rem]">
          <Panel className="p-[1.25rem]">
            <SectionHead title="Mirror size" />
            <p className="mt-[0.25rem] text-small text-secondary">A on a trade builds the same swap at your size. You review it before anything is signed.</p>
            <p className="mt-[0.9rem] text-label text-secondary">Each buy, in SUI</p>
            <div data-nav-group={`tr-mirror-buy-${frame}`} data-nav-remember className="mt-[0.4rem] flex gap-[0.4rem]">
              {BUY_SIZES.map((v) => (
                <Chip key={v} on={size.buySui === v} onClick={() => useMirrorSize.getState().set({ buySui: v })}>
                  {v}
                </Chip>
              ))}
            </div>
            <p className="mt-[0.9rem] text-label text-secondary">Each sell, of what you hold</p>
            <div data-nav-group={`tr-mirror-sell-${frame}`} data-nav-remember className="mt-[0.4rem] flex gap-[0.4rem]">
              {SELL_SIZES.map((v) => (
                <Chip key={v} on={size.sellPct === v} onClick={() => useMirrorSize.getState().set({ sellPct: v })}>
                  {`${v}%`}
                </Chip>
              ))}
            </div>
          </Panel>

          <Panel className="p-[1rem]">
            <SectionHead title="Holdings" note={holdings.length ? String(holdings.length) : undefined} className="mb-[0.5rem] px-[0.75rem] pt-[0.25rem]" />
            <div data-nav-group={`tr-trader-holdings-${frame}`}>
              {holdings.slice(0, 12).map((h) => (
                <button key={h.coin} data-nav data-hint-a="Trade" onClick={() => push({ kind: "token", coinType: h.coin })} className="cv-row flex h-[3.25rem] w-full items-center gap-[0.75rem] rounded-card px-[0.75rem] text-left">
                  <CoinLogo src={h.token?.logo} symbol={h.token?.symbol ?? "?"} size="2rem" />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-body font-semibold">{h.token?.symbol ?? h.coin.split("::").pop()}</span>
                    {h.unrealized != null && Math.abs(h.unrealized) >= 0.01 && <Num kind="pnl" value={h.unrealized} className="block text-label" />}
                  </span>
                  <Num kind="money" value={h.valueUsd} className="text-small font-medium" />
                </button>
              ))}
            </div>
            {!holdings.length && <Quiet className="py-[1rem]">{s ? "Holds nothing priced right now." : stats.error ? "Holdings are unavailable right now." : ""}</Quiet>}
          </Panel>
        </div>
      </div>

      {mirroring && <MirrorSheet trade={mirroring} token={trades.data?.tokens?.[mirroring.coin] ?? null} traderName={name} size={size} account={account} onClose={() => setMirroring(null)} />}
    </div>
  );
}
