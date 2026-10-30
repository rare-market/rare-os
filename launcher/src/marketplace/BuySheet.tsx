// Native confirm sheet for a purchase, in rare.market's modal style (ui/modal.tsx, the item buy
// box's fee details). With an account Rare Wallet can sign for, "Buy for X SUI" builds the
// purchase (buy.ts) and hands it to the wallet's review sheet, which asks for the PIN or the
// device and submits. "Buy on your phone" shows a QR of the rare.market item page instead.
// Watch accounts get the QR straight away.

import { Check, X } from "lucide-react";
import QRCode from "qrcode";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { native } from "../lib/bridge";
import { keystore, type RareAccount } from "../wallet";
import { signAndExecute, SignRejected } from "../wallet/market";
import { signAndExecuteForGame } from "../wallet/sign";
import { load, paths, type ItemCard, type Listing } from "./api";
import { buildBuyTransaction, canBuyInApp, quoteOf } from "./buy";
import { fmtSui, fmtUsd, itemUrl, mistToUsd, shortName, venueName } from "./format";
import { MImg } from "./MImg";
import { useLandingFocus } from "./page";
import { Btn, VenueMark } from "./parts";
import { AddPasskey } from "../wallet/ui/AddPasskey";

export function Qr({ text, className = "" }: { text: string; className?: string }) {
  const [svg, setSvg] = useState("");
  useEffect(() => {
    let live = true;
    void QRCode.toString(text, { type: "svg", margin: 1, errorCorrectionLevel: "M", color: { dark: "#100C18", light: "#ffffff" } }).then((s) => live && setSvg(s));
    return () => {
      live = false;
    };
  }, [text]);
  return <div className={`overflow-hidden rounded-md bg-white p-2 [&>svg]:block [&>svg]:size-full ${className}`} dangerouslySetInnerHTML={{ __html: svg }} />;
}

function Line({ label, mist, royalty }: { label: string; mist: bigint; royalty?: boolean }) {
  return (
    <div className={`flex justify-between ${royalty ? "text-royalties" : ""}`}>
      <span className={royalty ? "" : "text-secondary"}>{label}</span>
      <span className="font-mono">{fmtSui(mist, { dp: 4 })} SUI</span>
    </div>
  );
}

type Phase = { at: "idle" } | { at: "building" } | { at: "signing" } | { at: "error"; message: string } | { at: "done"; digest: string };

/** Build and network errors in plain words. */
function plain(e: unknown): string {
  const m = (e as Error)?.message ?? String(e);
  if (/^rare\.market \d+|failed to fetch|networkerror|load failed/i.test(m)) return "Couldn't reach rare.market. Check the connection and try again.";
  return m || "That didn't go through. Try again.";
}

const shortDigest = (d: string) => (d.length > 20 ? `${d.slice(0, 10)}...${d.slice(-8)}` : d);

export function BuySheet({
  item,
  listing,
  account: given,
  suiUsd,
  onClose,
  onBought,
  payer,
}: {
  item: ItemCard;
  listing: Listing;
  account: RareAccount | null;
  suiUsd: number | null;
  onClose: () => void;
  /** The purchase went through (the Local API's buy answer). */
  onBought?: (digest: string) => void;
  /** A game asked (Local API): its gas budget pays the fee when it covers this. */
  payer?: { slug: string | null; name: string };
}) {
  const root = useRef<HTMLDivElement>(null);
  // Read the selected account again: the page's copy can be from before an account was added.
  const [fresh, setFresh] = useState<RareAccount | null | undefined>(undefined);
  useEffect(() => {
    void keystore.current().then(setFresh, () => setFresh(null));
  }, []);
  const account = fresh === undefined ? given : fresh ?? given;
  const q = quoteOf(listing);
  const url = itemUrl(item.id);
  const inApp = canBuyInApp(account);
  const [phase, setPhase] = useState<Phase>({ at: "idle" });
  const [qrAsked, setQr] = useState(false);
  const qr = qrAsked || !inApp;
  const live = useRef(true);
  useEffect(() => {
    live.current = true; // StrictMode runs effects twice in dev
    return () => {
      live.current = false;
    };
  }, []);
  useLandingFocus(root, true, `${phase.at}${qr}`);

  const busy = phase.at === "building" || phase.at === "signing";
  const name = shortName(item);

  const buy = async () => {
    if (!account || busy || phase.at === "done") return;
    setPhase({ at: "building" });
    try {
      const tx = await buildBuyTransaction(item, listing, account.address);
      if (!live.current) return;
      setPhase({ at: "signing" });
      const opts = { account: account.address, origin: { name: "Rare Marketplace", kind: "market" as const }, title: `Buy ${item.name}` };
      const res = payer ? await signAndExecuteForGame(tx, payer, opts) : await signAndExecute(tx, opts);
      if (res.status === "failure") throw new Error("The network rejected this purchase.");
      const digest = res.digest ?? "";
      onBought?.(digest);
      // Refresh the item page behind the sheet: now, and again once rare.market has indexed the sale.
      const refresh = () => {
        void load(paths.item(item.id)).catch(() => undefined);
        void load(paths.events({ item: item.id }, 12)).catch(() => undefined);
      };
      refresh();
      setTimeout(refresh, 4000);
      if (live.current) setPhase({ at: "done", digest });
    } catch (e) {
      if (!live.current) return;
      if (e instanceof SignRejected) setPhase({ at: "idle" });
      else setPhase({ at: "error", message: plain(e) });
    }
  };

  const total = `${fmtSui(q.totalMist, { dp: 4, trim: true })} SUI`;

  return createPortal(
    <div className="fixed inset-x-0 bottom-[3rem] top-0 z-40 flex items-center justify-center p-6">
      <div className="absolute inset-0 bg-background/80" aria-hidden />
      <div ref={root} data-nav-scope className="fade-in relative flex w-full max-w-[52rem] flex-col overflow-hidden rounded-modal border border-border-light bg-background">
        <div className="flex shrink-0 items-start justify-between gap-4 border-b border-border-primary px-8 pb-4 pt-6">
          <div className="flex min-w-0 items-center gap-3">
            <span className="relative size-12 shrink-0 overflow-hidden rounded-md bg-layer-03">
              <MImg src={item.imageUrl} label={item.name} width={120} className="absolute inset-0 size-full" />
            </span>
            <div className="min-w-0">
              <h2 className="text-xl font-bold leading-[1.55]">{phase.at === "done" ? "Bought" : `Buy ${name}`}</h2>
              <p className="truncate text-sm text-secondary">{item.collection?.name}</p>
            </div>
          </div>
          <button data-nav onClick={onClose} aria-label="Close" className="-mr-1 mt-1 flex size-10 items-center justify-center rounded text-icon-primary">
            <X size="1.25rem" />
          </button>
        </div>

        {phase.at === "done" ? (
          <>
            <div className="flex items-center gap-5 px-8 py-8">
              <span className="flex size-14 shrink-0 items-center justify-center rounded-full bg-layer-03 text-primary">
                <Check size="1.75rem" />
              </span>
              <div className="min-w-0">
                <p className="text-lg font-semibold">
                  {item.name} is yours for {total}
                </p>
                <p className="mt-1 text-sm text-secondary">It's in your Rare Wallet. It can take a minute to show on rare.market.</p>
                <p className="mt-3 text-xs text-secondary">
                  Transaction <span className="font-mono text-primary">{shortDigest(phase.digest)}</span>
                </p>
              </div>
            </div>
            <div className="flex shrink-0 gap-2 border-t border-border-primary px-8 py-4">
              <Btn variant="primary" def className="flex-1" onClick={onClose}>
                Done
              </Btn>
            </div>
          </>
        ) : (
          <>
            <div className={`grid gap-8 px-8 py-6 ${qr ? "grid-cols-[minmax(0,1fr)_auto]" : "grid-cols-1"}`}>
              <div>
                <p className="text-base">Total price</p>
                <p className="mt-1 flex items-baseline gap-1.5 text-4xl font-bold tabular-nums">
                  {fmtSui(q.totalMist, { dp: 4 })}
                  <span className="text-xl font-semibold">SUI</span>
                </p>
                <span className="text-base text-secondary tabular-nums">({fmtUsd(mistToUsd(q.totalMist, suiUsd))})</span>
                <div className="mt-4 flex flex-col gap-1.5 rounded border border-border-primary px-3 py-3 text-sm">
                  <Line label="List price" mist={q.priceMist} />
                  {q.feeMist > 0n && <Line label={`${venueName(q.venue)} fee`} mist={q.feeMist} />}
                  {q.royaltyMist > 0n && <Line label="Creator royalty" mist={q.royaltyMist} royalty />}
                  {q.commissionMist > 0n && <Line label="Rare fee" mist={q.commissionMist} />}
                  <div className="flex justify-between border-t border-border-primary pt-1.5 font-semibold">
                    <span>Total</span>
                    <span className="font-mono">{fmtSui(q.totalMist, { dp: 4 })} SUI</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-secondary">Listed on</span>
                    <span className="flex items-center gap-1.5">
                      <VenueMark venue={q.venue} /> {venueName(q.venue)}
                    </span>
                  </div>
                </div>
                <p className="mt-2 text-xs text-secondary">Network gas is extra.</p>
              </div>
              {qr && (
                <div className="flex w-[14rem] flex-col items-center text-center">
                  <Qr text={url} className="size-[12rem]" />
                  <p className="mt-3 text-sm font-semibold">{inApp ? "Buy on your phone" : "Finish on your phone"}</p>
                  <p className="mt-1 text-xs text-secondary">Scan to buy on rare.market.</p>
                  {!inApp && <AddPasskey need="buy" account={account} className="mt-3 items-center text-center" />}
                </div>
              )}
            </div>
            {inApp && phase.at !== "idle" && (
              <p role="status" className={`px-8 pb-4 text-sm ${phase.at === "error" ? "text-negative" : "text-secondary"}`}>
                {phase.at === "building" ? "Getting the purchase ready..." : phase.at === "signing" ? "Review and approve in Rare Wallet." : phase.message}
              </p>
            )}
            <div className="flex shrink-0 gap-2 border-t border-border-primary px-8 py-4">
              {inApp ? (
                <>
                  <Btn variant="primary" def className="flex-1" onClick={() => void buy()}>
                    {phase.at === "building" ? "Getting ready..." : phase.at === "signing" ? "Waiting for approval" : phase.at === "error" ? "Try again" : `Buy for ${total}`}
                  </Btn>
                  {qr ? <Btn onClick={() => void native.openWeb(url)}>Open on rare.market</Btn> : <Btn onClick={() => setQr(true)}>Buy on your phone</Btn>}
                </>
              ) : (
                <>
                  <Btn variant="primary" def className="flex-1" onClick={onClose}>
                    Done
                  </Btn>
                  <Btn onClick={() => void native.openWeb(url)}>Open on rare.market</Btn>
                </>
              )}
            </div>
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}
