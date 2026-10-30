// The global transaction / message review, in rare.market's modal (like the Marketplace BuySheet).
// Shows requests[0] from the signing queue: who is asking, what leaves and arrives, the fee and
// any warning in plain words. Approve asks for the PIN when the account needs it, then signs;
// hardware wallets and the passkey phone show their live state in place of the footer.

import { ChevronDown } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { MImg } from "../../../marketplace/MImg";
import { paths, useRare, type ItemDetail } from "../../../marketplace/api";
import { passkeyNative } from "../../native";
import { callLabel, normType, reviewTransaction, shortId, splitFees, SUI_TYPE, type Review, type ReviewRow, type Risk } from "../../review";
import { knownSponsors } from "../../sponsor";
import { approveRequest, rejectRequest, useSignRequests, type SignRequest, type SignResult } from "../../sign";
import { formatAmount, toNumber, usd } from "../../tx";
import { Addr, Avatar, Btn, Row, Sheet, ToggleRow, cn, useCopy } from "../kit";
import { useWallet } from "../store";
import { ensureUnlockedFor } from "../unlock";
import { KeystonePanel, LedgerPanel, PasskeyPanel } from "./HardwarePanels";
import { cancelKeystone, cancelLedger, useDevHw, usePasskeyState } from "./prompts";
import { recents } from "./recents";
import { decodeMessage, hostOf, siteRisks } from "./site";

type Phase = "review" | "working" | "done" | "error";
interface Flow {
  req: SignRequest;
  phase: Phase;
  error?: string;
  /** the request is gone from the queue (the network refused it): only Done is left */
  final?: boolean;
  result?: SignResult;
}

const hw = (kind?: string): "ledger" | "keystone" | "passkey" | null => (kind === "ledger" || kind === "keystone" || kind === "passkey" ? kind : null);

// ---------------------------------------------------------------- review data

type Load = { s: "loading" } | { s: "ready"; review: Review } | { s: "unavailable" };

function useTxReview(req: SignRequest): Load {
  const [st, setSt] = useState<Load>({ s: "loading" });
  const accounts = useWallet((w) => w.accounts);
  useEffect(() => {
    if (req.kind !== "transaction" || !req.tx) return;
    let live = true;
    setSt({ s: "loading" });
    const sentTo = recents().map((r) => r.address);
    const contacts = (accounts ?? []).map((a) => ({ address: a.address, name: a.label }));
    const sponsors = [...knownSponsors(), ...(req.sponsored ? [req.sponsored.sponsor] : [])];
    reviewTransaction(req.tx, req.account, { addresses: { sentTo, receivedFrom: [], contacts }, sponsors, paidBy: req.sponsored?.payer })
      .then((review) => live && setSt({ s: "ready", review }))
      .catch(() => live && setSt({ s: "unavailable" }));
    return () => {
      live = false;
    };
  }, [req.id]); // eslint-disable-line react-hooks/exhaustive-deps
  return st;
}

// ---------------------------------------------------------------- body pieces

const money = (n: number | null) => (n == null ? null : n > 0 && n < 0.01 ? "<$0.01" : usd(n));

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mt-6 first:mt-0" aria-label={title}>
      <h3 className="mb-2 text-sm text-secondary">{title}</h3>
      <div className="divide-y divide-border-primary rounded border border-border-primary">{children}</div>
    </section>
  );
}

function CoinRow({ row, review }: { row: ReviewRow; review: Review }) {
  const meta = review.coins[normType(row.coinType ?? SUI_TYPE)];
  const dec = meta?.decimals ?? 9;
  const amount = row.amount ?? 0n;
  const value = meta?.priceUsd != null ? toNumber(amount, dec) * meta.priceUsd : null;
  const out = row.kind === "send";
  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <Avatar src={row.iconUrl ?? meta?.iconUrl} label={meta?.symbol ?? "?"} size="2rem" />
      <div className="min-w-0 flex-1">
        <p className="truncate text-base font-medium">{meta?.symbol}</p>
        <p className="truncate text-xs text-secondary">{meta?.priceUsd != null ? usd(meta.priceUsd) : "No price"}</p>
      </div>
      <div className="text-right">
        <p className={cn("text-base font-medium tabular-nums", !out && "text-positive")}>
          {out ? "-" : "+"}
          {formatAmount(amount, dec)} {meta?.symbol}
        </p>
        <p className="h-4 text-xs text-secondary tabular-nums">{money(value)}</p>
      </div>
    </div>
  );
}

function ObjectRow({ row }: { row: ReviewRow }) {
  // rare.market knows NFTs by object id: name and art when the chain display has none
  const q = useRare<ItemDetail>(row.objectId ? paths.item(row.objectId) : null, 60_000);
  const name = q.data?.name || row.text;
  const image = row.image ?? q.data?.imageUrl ?? null;
  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <span className="relative size-12 shrink-0 overflow-hidden rounded-md bg-layer-03">
        <MImg src={image} label={name} width={120} className="absolute inset-0 size-full" />
      </span>
      <div className="min-w-0 flex-1">
        <p className="truncate text-base font-medium">{name}</p>
        {(row.sub || q.data?.collection?.name) && <p className="truncate text-xs text-secondary">{row.sub ?? q.data?.collection?.name}</p>}
      </div>
    </div>
  );
}

function Notes({ risks }: { risks: Risk[] }) {
  if (!risks.length) return null;
  return (
    <ul className="mt-6 flex flex-col gap-1.5 text-sm" role={risks.some((r) => r.danger) ? "alert" : undefined}>
      {risks.map((r) => (
        <li key={r.id} className={r.danger ? "text-negative" : "text-secondary"}>
          {r.text}
        </li>
      ))}
    </ul>
  );
}

function Details({ review, open: start }: { review: Review; open: boolean }) {
  const [open, setOpen] = useState(start);
  const ins = review.insights;
  if (!ins.calls.length && !ins.shared.length) return null;
  return (
    <section className="mt-6" aria-label="Details">
      <button data-nav onClick={() => setOpen((v) => !v)} aria-expanded={open} className="flex w-full items-center justify-between rounded px-1 py-2 text-sm text-secondary">
        <span>Details</span>
        <ChevronDown size="1rem" className={open ? "rotate-180" : ""} />
      </button>
      {open && (
        <div className="mt-1 flex flex-col gap-2 text-sm">
          {ins.calls.map((c, i) => {
            const l = callLabel(c, review.simulation.packages);
            return (
              <p key={i} className="flex justify-between gap-4">
                <code className="font-mono text-xs">{l.title}</code>
                <span className="text-secondary">{l.sub}</span>
              </p>
            );
          })}
          {ins.shared.map((s) => (
            <p key={s.objectId} className="flex justify-between gap-4">
              <code className="font-mono text-xs">{s.label}</code>
              <span className="font-mono text-xs text-secondary">{shortId(s.objectId)}</span>
            </p>
          ))}
        </div>
      )}
    </section>
  );
}

function TxBody({ load, req, acked, onAck, site }: { load: Load; req: SignRequest; acked: boolean; onAck: (v: boolean) => void; site: Risk[] }) {
  if (load.s === "loading")
    return (
      <>
        <div className="load-bar pointer-events-none absolute inset-x-0 top-0 h-[2px]" role="progressbar" aria-label="Checking" />
        <p className="py-10 text-center text-sm text-secondary">Checking what this does</p>
      </>
    );
  if (load.s === "unavailable")
    return (
      <>
        <p className="text-sm text-secondary">Could not preview this transaction. Approve only if you trust who is asking.</p>
        <Notes risks={site} />
      </>
    );
  const { review } = load;
  const ins = review.insights;
  const failed = !review.ok;
  const sends = review.rows.filter((r) => r.kind === "send");
  const gets = review.rows.filter((r) => r.kind === "receive");
  const fee = review.rows.find((r) => r.kind === "fee");
  const feeUsd = fee?.amount != null && review.coins[SUI_TYPE]?.priceUsd != null ? (Number(fee.amount < 0n ? -fee.amount : fee.amount) / 1e9) * (review.coins[SUI_TYPE].priceUsd ?? 0) : null;
  const risks = [...review.risks.filter((r) => !(failed && r.id === "failed")), ...site].sort((a, b) => Number(!!b.danger) - Number(!!a.danger));
  const needsAck = review.risks.some((r) => r.requiresConfirm);
  const nothing = review.ok && !sends.length && !gets.length;
  // Rare's own fee wallets (swap fee) are a fee line, not a "Goes to" recipient
  const { fees: rareFees, others } = splitFees(ins);
  const draw = (r: ReviewRow, i: number) => (r.coinType ? <CoinRow key={i} row={r} review={review} /> : <ObjectRow key={i} row={r} />);
  return (
    <>
      {failed && (
        <div className="mb-6 rounded border border-negative/60 px-4 py-3" role="alert">
          <p className="text-base font-semibold text-negative">This would fail</p>
          <p className="mt-1 text-sm">{ins.reason}</p>
        </div>
      )}
      {sends.length > 0 && <Section title="You send">{sends.map(draw)}</Section>}
      {gets.length > 0 && <Section title="You receive">{gets.map(draw)}</Section>}
      {nothing && <p className="text-sm text-secondary">No coins or objects move in or out of your wallet.</p>}
      {others.length > 0 && (
        <Section title="Goes to">
          {others.map((o, i) => {
            const m = review.coins[normType(o.coinType)];
            return (
              <div key={i} className="flex items-center justify-between gap-4 px-4 py-3 text-sm">
                <Addr address={o.address} />
                <span className="tabular-nums">
                  {formatAmount(o.amount, m?.decimals ?? 9)} {m?.symbol}
                </span>
              </div>
            );
          })}
        </Section>
      )}
      <div className="mt-6 flex flex-col gap-1.5 rounded border border-border-primary px-4 py-3">
        <Row label="Account">
          <AccountLine address={req.account} />
        </Row>
        {fee && (
          <Row label="Network fee">
            <span className={cn("tabular-nums", fee.amount != null && fee.amount < 0n && "text-positive")}>{fee.text}</span>
            {feeUsd != null && <span className="ml-2 text-secondary tabular-nums">{money(feeUsd)}</span>}
          </Row>
        )}
        {fee?.sub && <p className="text-right text-xs text-secondary">{fee.sub}</p>}
        {req.feeNote && <p className="text-right text-xs text-secondary">{req.feeNote}</p>}
        {rareFees.map((f, i) => {
          const m = review.coins[normType(f.coinType)];
          return (
            <Row key={i} label={f.pct ? `${f.label} ${f.pct}` : f.label}>
              <span className="tabular-nums">
                {formatAmount(f.amount, m?.decimals ?? 9)} {m?.symbol}
              </span>
            </Row>
          );
        })}
      </div>
      <Notes risks={risks} />
      {needsAck && (
        <div className="mt-4 -mx-4">
          <ToggleRow label="I checked the full address and it is right" checked={acked} onChange={onAck} />
        </div>
      )}
      <Details review={review} open={failed} />
    </>
  );
}

function AccountLine({ address }: { address: string }) {
  const a = useWallet((w) => w.accounts?.find((x) => x.address === address));
  return (
    <span>
      {a?.label ?? "Account"} <Addr address={address} className="ml-1 text-secondary" />
    </span>
  );
}

function MessageBody({ req, site }: { req: SignRequest; site: Risk[] }) {
  const m = useMemo(() => decodeMessage(req.message ?? new Uint8Array()), [req.message]);
  return (
    <>
      <h3 className="mb-2 text-sm text-secondary">{m.text !== null ? "Message" : "Message (hex)"}</h3>
      <pre data-testid="message-text" className={cn("max-h-[14rem] overflow-y-auto whitespace-pre-wrap break-all rounded border border-border-primary bg-layer-01 px-4 py-3 text-sm", m.text === null && "font-mono text-xs")}>
        {m.text ?? m.hex}
      </pre>
      <p className="mt-4 text-sm text-secondary">Signing proves you own this account. It cannot move funds by itself, but a site can use it to sign you in or approve actions, so only sign messages you expect.</p>
      <div className="mt-6 rounded border border-border-primary px-4 py-3">
        <Row label="Account">
          <AccountLine address={req.account} />
        </Row>
      </div>
      <Notes risks={site} />
    </>
  );
}

function Done({ req, result, review }: { req: SignRequest; result: SignResult; review: Load }) {
  const [copied, copy] = useCopy();
  const id = result.digest;
  const message = req.kind === "message";
  return (
    <div className="flex flex-col items-start gap-4">
      <p className="text-base">{message ? "Your signature is ready." : req.execute ? "Your transaction went through." : "Your signature is ready."}</p>
      {review.s === "ready" && review.review.rows.filter((r) => r.kind !== "fee").length > 0 && (
        <div className="w-full rounded border border-border-primary px-4 py-3 text-sm">
          {review.review.rows
            .filter((r) => r.kind !== "fee")
            .map((r, i) => (
              <Row key={i} label={r.label}>
                {r.text}
              </Row>
            ))}
        </div>
      )}
      {id && (
        <div className="w-full">
          <p className="mb-1.5 text-sm text-secondary">Transaction ID</p>
          <p className="break-all font-mono text-sm">{id}</p>
        </div>
      )}
      {result.dryRun && <p className="text-sm text-secondary">Test run. Nothing was sent to the network.</p>}
      {id && (
        <Btn size="md" onClick={() => copy(id)}>
          {copied ? "Copied" : "Copy ID"}
        </Btn>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- the sheet

function ReviewSheet({
  req,
  flow,
  onApprove,
  onCancel,
  onDone,
}: {
  req: SignRequest;
  flow: Flow | null;
  onApprove: (bytes?: Uint8Array) => void;
  onCancel: () => void;
  onDone: () => void;
}) {
  const account = useWallet((w) => w.accounts?.find((a) => a.address === req.account));
  const force = useDevHw((s) => s.force);
  const kind = force ?? hw(account?.kind);
  const pk = usePasskeyState();
  const load = useTxReview(req);
  const [acked, setAcked] = useState(false);
  const [anyway, setAnyway] = useState(false);
  const phase: Phase = flow?.phase ?? "review";
  const isMsg = req.kind === "message";
  const site = useMemo(() => siteRisks(req.origin.url), [req.origin.url]);

  const review = load.s === "ready" ? load.review : null;
  const failed = !!review && !review.ok;
  const needsAck = !!review?.risks.some((r) => r.requiresConfirm);
  const loading = !isMsg && load.s === "loading";
  const blocked = loading || (failed && !anyway) || (needsAck && !acked);

  const title =
    phase === "done" ? (isMsg || !req.execute ? "Signed" : "Sent") : req.title ?? (isMsg ? "Sign message" : req.execute ? "Approve transaction" : "Sign transaction");
  const originLine = req.origin.kind === "game" ? `${req.origin.name} · ${hostOf(req.origin.url)}` : req.origin.name;
  const approve = () => onApprove(review?.bytes ?? undefined);

  const icon =
    req.origin.kind === "game" ? (
      <Avatar src={req.origin.icon} label={req.origin.name} size="3rem" square />
    ) : (
      <span className="relative size-12 shrink-0 overflow-hidden rounded-md bg-layer-03">
        <img src="/brand/mark.png" alt="" draggable={false} className="absolute inset-0 size-full object-contain p-1.5" />
      </span>
    );

  let footer: React.ReactNode;
  if (phase === "done") {
    footer = (
      <Btn variant="primary" def className="flex-1" onClick={onDone}>
        Done
      </Btn>
    );
  } else if (phase === "error" && flow?.final) {
    footer = (
      <>
        <p className="mr-auto self-center text-sm text-negative">{flow.error}</p>
        <Btn variant="primary" def onClick={onDone}>
          Done
        </Btn>
      </>
    );
  } else if (phase === "working" && kind && (kind !== "passkey" || force || pk.kind !== "idle")) {
    footer =
      kind === "ledger" ? (
        <LedgerPanel onRetry={approve} onCancel={onCancel} />
      ) : kind === "keystone" ? (
        <KeystonePanel onRetry={approve} onCancel={onCancel} />
      ) : (
        <PasskeyPanel onRetry={approve} onCancel={onCancel} />
      );
  } else if (phase === "working") {
    footer = (
      <>
        <Btn variant="dimmed" className="flex-1">
          Signing
        </Btn>
        <Btn def onClick={onCancel}>
          Cancel
        </Btn>
      </>
    );
  } else {
    footer = (
      <div className="flex w-full flex-col gap-3">
        {phase === "error" && <p className="text-sm text-negative">{flow?.error}</p>}
        {failed && !anyway && (
          <p className="text-sm text-secondary">
            Approve is off because this would fail.{" "}
            <button data-nav onClick={() => setAnyway(true)} className="rounded px-1 text-primary underline">
              Approve anyway
            </button>
          </p>
        )}
        {failed && anyway && <p className="text-sm text-secondary">It will likely fail and still cost the network fee.</p>}
        <div className="flex gap-2">
          {blocked ? (
            <Btn variant="dimmed" className="flex-1">
              {loading ? "Checking" : isMsg ? "Sign" : "Approve"}
            </Btn>
          ) : (
            <Btn variant="primary" def className="flex-1" onClick={approve}>
              {phase === "error" ? "Retry" : isMsg ? "Sign" : "Approve"}
            </Btn>
          )}
          <Btn def={blocked} onClick={onCancel}>
            Cancel
          </Btn>
        </div>
      </div>
    );
  }

  return (
    <Sheet
      title={title}
      subtitle={phase === "done" ? undefined : `${originLine} · ${account?.label ?? "Account"}`}
      icon={icon}
      footer={footer}
      onClose={phase === "working" ? undefined : phase === "done" || flow?.final ? onDone : onCancel}
      width="44rem"
      ready={`${load.s}${phase}`}
    >
      {phase === "done" && flow?.result ? <Done req={req} result={flow.result} review={load} /> : isMsg ? <MessageBody req={req} site={site} /> : <TxBody load={load} req={req} acked={acked} onAck={setAcked} site={site} />}
    </Sheet>
  );
}

// ---------------------------------------------------------------- host

/** Mounted once (WalletHost): shows the oldest pending sign request, runs Approve. */
export function ReviewHost() {
  const head = useSignRequests((s) => s.requests[0]);
  const [flow, setFlow] = useState<Flow | null>(null);
  const req = flow?.req ?? head;
  if (!req) return null;

  const stillQueued = () => useSignRequests.getState().requests.some((r) => r.id === req.id);

  const approve = async (bytes?: Uint8Array) => {
    setFlow({ req, phase: "working" });
    // screenshots: a forced device kind just holds on its waiting panel
    if (useDevHw.getState().force) return;
    try {
      if (!(await ensureUnlockedFor(req.account, "Enter your PIN to approve"))) {
        setFlow(null);
        return;
      }
      const res = await approveRequest(req.id, bytes);
      if (res.status === "failure") {
        setFlow({ req, phase: "error", error: "The network rejected this transaction.", final: true });
        return;
      }
      useWallet.getState().bump();
      // the Marketplace and games show their own result (Bought, back in the game): no second Done
      if (req.origin.kind !== "wallet") {
        setFlow(null);
        return;
      }
      setFlow({ req, phase: "done", result: res });
    } catch (e) {
      // cancelled from the sheet: the request is already gone
      if (!stillQueued()) setFlow(null);
      else setFlow({ req, phase: "error", error: e instanceof Error ? e.message : "Could not sign. Try again." });
    }
  };

  const cancel = () => {
    cancelLedger();
    cancelKeystone();
    void passkeyNative.cancel().catch(() => {});
    setFlow(null);
    rejectRequest(req.id);
  };

  return <ReviewSheet key={req.id} req={req} flow={flow} onApprove={(b) => void approve(b)} onCancel={cancel} onDone={() => setFlow(null)} />;
}
