// One past transaction: what moved, who with, fee, status, time and digest, plus a QR of the
// explorer link so the person can open it on their phone.

import { ChevronDown, ChevronUp } from "lucide-react";
import { useState } from "react";
import { native } from "../../../lib/bridge";
import { fmtUsd } from "../../../marketplace/format";
import { activityDetail, type TxDetail } from "../../data/txdetail";
import { Addr, Avatar, Btn, Page, Panel, Qr, Row } from "../kit";
import { useWallet, type WalletView } from "../store";
import { CopyValue, useCoinInfo } from "../home/bits";
import { LoadBar, fmtAmount, knownActivity, shortDigest, unitsToNumber, useHints, useLoad } from "../home/lib";

const TITLE = { send: "Sent", receive: "Received", swap: "Swapped", other: "Transaction" } as const;

export function ActivityDetailPage({ view, active }: { view: Extract<WalletView, { kind: "activity" }>; active: boolean }) {
  const address = useWallet((s) => s.current?.address);
  const item = knownActivity(view.digest);
  const q = useLoad<TxDetail | null>(address ? `tx:${view.digest}:${address}` : null, () => activityDetail(view.digest, address!), 0);
  const [open, setOpen] = useState(false);
  const d = q.data;
  const url = `https://suiscan.xyz/mainnet/tx/${view.digest}`;
  useHints(active, [{ b: "a", label: "Select" }, { b: "b", label: "Back" }]);

  // what moved: the chain's view when we have it, else the list's summary changes
  const flows = d ? d.flows : (item?.changes ?? []).map((c) => ({ coinType: c.coinType, amount: c.amount }));
  const types = [...flows.map((f) => f.coinType), "0x2::sui::SUI"];
  const coin = useCoinInfo(types, item?.changes ?? []);
  const info = (t: string) => coin[t] ?? { symbol: t.split("::").pop() ?? "?", decimals: 9, logo: null, price: null, known: false };
  const out = flows.filter((f) => f.amount.startsWith("-"));
  const inn = flows.filter((f) => !f.amount.startsWith("-"));
  const kind = item?.kind ?? (out.length && inn.length ? "swap" : out.length ? "send" : inn.length ? "receive" : "other");
  const counterparty = kind === "swap" ? undefined : (d?.counterparty ?? item?.counterparty);
  const failed = d ? d.status === "failure" : item?.status === "failed";
  const ts = d?.timestampMs ?? item?.timestamp ?? null;
  const fee = d ? BigInt(d.feeMist) : 0n;
  const sui = info("0x2::sui::SUI");
  const feeNum = Number(fee < 0n ? -fee : fee) / 1e9;

  const list = (title: string, rows: typeof flows, sign: "+" | "-") =>
    rows.length > 0 && (
      <Panel title={title} className="pb-2">
        <div className="mt-2 px-2">
          {rows.map((f) => {
            const c = info(f.coinType);
            const n = Math.abs(unitsToNumber(f.amount, c.decimals));
            return (
              <div key={f.coinType} className="flex h-14 items-center gap-3 px-3">
                <Avatar src={c.logo} label={c.symbol} size="2rem" />
                <span className="min-w-0 flex-1">
                  <span className="block text-base font-semibold">{c.symbol}</span>
                  <span className="block text-xs text-secondary">{c.price != null ? fmtUsd(c.price) : "No price"}</span>
                </span>
                <span className="text-right tabular-nums">
                  <span className={sign === "+" ? "block text-base font-semibold text-positive" : "block text-base font-semibold"}>
                    {sign}
                    {c.known ? fmtAmount(n) : "?"} {c.symbol}
                  </span>
                  {c.price != null && c.known && <span className="block text-xs text-secondary">{fmtUsd(n * c.price)}</span>}
                </span>
              </div>
            );
          })}
        </div>
      </Panel>
    );

  return (
    <Page active={active} ready={!!d || q.error} title={TITLE[kind]} subtitle={ts ? new Date(ts).toLocaleString([], { dateStyle: "medium", timeStyle: "short" }) : undefined}>
      <LoadBar on={q.loading} />
      <div className="mt-3 grid grid-cols-[minmax(0,1fr)_16rem] items-start gap-8">
        <div className="flex min-w-0 flex-col gap-4">
          <Panel className="flex flex-col gap-2 px-5 py-4">
            <Row label="Status">
              <span className={failed ? "text-negative" : undefined}>{failed ? "Failed" : d || item ? "Success" : "--"}</span>
            </Row>
            {d?.error && <p className="text-sm text-negative">{d.error}</p>}
          </Panel>
          {d === null && <p className="px-1 text-sm text-secondary">The network no longer keeps the full details of this transaction. This is what the activity list recorded.</p>}
          {list("You sent", out, "-")}
          {list("You received", inn, "+")}
          <Panel className="flex flex-col gap-1 px-5 py-4">
            {counterparty && (
              <Row label={kind === "receive" ? "From" : "To"}>
                <CopyValue value={counterparty} shown={`${counterparty.slice(0, 8)}...${counterparty.slice(-6)}`} label="Copy address" />
              </Row>
            )}
            {d && (
              <Row label="Network fee">
                {!d.feePaid ? (
                  "Paid by sponsor"
                ) : (
                  <span className={fee < 0n ? "text-positive" : undefined}>
                    {fee < 0n ? "Refund " : ""}
                    {fmtAmount(feeNum, 6)} SUI
                    {sui.price != null && <span className="ml-2 text-secondary">{fmtUsd(feeNum * sui.price)}</span>}
                  </span>
                )}
              </Row>
            )}
            <Row label="Digest">
              <CopyValue value={view.digest} shown={shortDigest(view.digest)} label="Copy digest" />
            </Row>
            {d && d.sender && (
              <Row label="Sender">
                <Addr address={d.sender} />
              </Row>
            )}
          </Panel>
          {d && (d.calls.length > 0 || d.objects.length > 0) && (
            <Panel>
              <button data-nav onClick={() => setOpen((v) => !v)} aria-expanded={open} className="flex h-14 w-full items-center justify-between rounded-lg px-5 text-base font-semibold">
                Details
                {open ? <ChevronUp size="1.1rem" /> : <ChevronDown size="1.1rem" />}
              </button>
              {open && (
                <div className="flex flex-col gap-1 px-5 pb-4 text-sm">
                  {d.calls.map((c, i) => (
                    <p key={i} className="font-mono text-secondary">
                      {c.module}::{c.function}
                    </p>
                  ))}
                  {d.objects.map((o) => (
                    <p key={o.id} className="text-secondary">
                      {o.action === "sent" ? "Sent" : o.action === "created" ? "Created" : o.action === "deleted" ? "Deleted" : "Received"} <span className="font-mono">{shortDigest(o.id)}</span>
                      {o.type && <span className="ml-2 font-mono text-xs">{o.type.split("::").slice(-1)[0]}</span>}
                    </p>
                  ))}
                </div>
              )}
            </Panel>
          )}
        </div>
        <Panel className="flex flex-col items-center p-5 text-center">
          <Qr text={url} className="size-[12rem]" />
          <p className="mt-3 text-sm font-semibold">Open on your phone</p>
          <p className="mt-1 text-xs text-secondary">Scan to see this transaction on Suiscan.</p>
          <Btn size="md" className="mt-3 w-full" onClick={() => void native.openWeb(url)}>
            View on Suiscan
          </Btn>
        </Panel>
      </div>
    </Page>
  );
}

