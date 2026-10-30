// One coin: balance card with Send and Receive, and this coin's recent activity.

import { ArrowDownLeft, ArrowUpRight, UserPlus } from "lucide-react";
import { fmtUsd } from "../../../marketplace/format";
import { canSignAccount } from "../dev";
import { Avatar, Btn, Delta, Empty, Page, Panel, Row } from "../kit";
import { useWallet, useWalletNav, type WalletView } from "../store";
import { normType } from "../home/bits";
import { ActivityRow } from "../home/ActivityList";
import { LoadBar, fmtAmount, useActivity, useHints, usePortfolio } from "../home/lib";

export function CoinPage({ view, active }: { view: Extract<WalletView, { kind: "coin" }>; active: boolean }) {
  const account = useWallet((s) => s.current);
  const epoch = useWallet((s) => s.epoch);
  const push = useWalletNav((s) => s.push);
  const address = account?.address;
  const pf = usePortfolio(address, epoch);
  const act = useActivity(address, epoch);
  const type = normType(view.coinType);
  const t = pf.data?.tokens.find((x) => normType(x.coinType) === type);
  const symbol = t?.symbol ?? view.coinType.split("::").pop() ?? "Coin";
  const canSign = canSignAccount(account);
  const recent = act.items.filter((a) => a.changes.some((c) => normType(c.coinType) === type)).slice(0, 8);

  useHints(active, [{ b: "a", label: "Open" }, { b: "b", label: "Back" }]);

  return (
    <Page
      active={active}
      ready={!!pf.data}
      title={
        <span className="flex items-center gap-3">
          <Avatar src={t?.iconUrl} label={symbol} size="2.25rem" />
          {symbol}
          {t?.name && <span className="text-base font-normal text-secondary">{t.name}</span>}
        </span>
      }
    >
      <LoadBar on={pf.loading || act.loading} />
      <div className="mt-4 grid grid-cols-[minmax(0,26rem)_minmax(0,1fr)] gap-8">
        <div className="flex flex-col gap-4 self-start">
          <Panel className="p-6">
            <div className="text-sm text-secondary">Your balance</div>
            <div className="mt-1 text-4xl font-bold tabular-nums">
              {fmtAmount(t?.amount ?? 0)} <span className="text-xl font-medium text-secondary">{symbol}</span>
            </div>
            <div className="mt-1 text-lg tabular-nums text-secondary">{t?.valueUsd != null ? fmtUsd(t.valueUsd) : pf.data ? "No price" : "--"}</div>
            {t?.priceUsd != null && (
              <div className="mt-5 flex flex-col gap-2 border-t border-border-primary pt-4">
                <Row label="Price">
                  <span className="tabular-nums">{t.priceUsd >= 1 ? fmtUsd(t.priceUsd) : `$${t.priceUsd.toPrecision(3)}`}</span>
                </Row>
                <Row label="24h">
                  <Delta value={t.change24h} dp={2} />
                </Row>
              </div>
            )}
          </Panel>
          <div className="flex gap-2">
            {canSign ? (
              <Btn variant="primary" def className="flex-1" onClick={() => push({ kind: "send", coinType: view.coinType })}>
                <ArrowUpRight size="1.1rem" /> Send
              </Btn>
            ) : (
              <Btn variant="primary" def className="flex-1" onClick={() => push({ kind: "add" })}>
                <UserPlus size="1.1rem" /> Add a signing account
              </Btn>
            )}
            <Btn className="flex-1" onClick={() => push({ kind: "receive" })}>
              <ArrowDownLeft size="1.1rem" /> Receive
            </Btn>
          </div>
        </div>

        <div className="min-w-0">
          <h2 className="mb-2 text-xl font-bold">Recent activity</h2>
          <div data-nav-group="wallet-coin-activity">
            {recent.map((a, i) => (
              <ActivityRow key={a.digest} a={a} idx={i} onOpen={() => push({ kind: "activity", digest: a.digest })} />
            ))}
          </div>
          {act.ready && recent.length === 0 && <Empty compact title={`No ${symbol} activity yet`} />}
          {act.error && !act.ready && <Empty compact title="Could not load activity" />}
        </div>
      </div>
    </Page>
  );
}
