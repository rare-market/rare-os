// The sheets a game (Local API, rare:platform) or a `rare://` link opens: sell, buy and claim. They
// reuse the overlay's item sheet (sell into the best offer, or list at floor, +10% or any price) and
// the Marketplace's BuySheet; claim is a one-line confirm. Anything signed goes through Rare Wallet's
// review sheet and PIN as always. B (or Not now) answers the game "cancelled".

import { useEffect, useState } from "react";
import { useLive } from "../lib/live";
import { BuySheet } from "../marketplace/BuySheet";
import { shortName } from "../marketplace/format";
import { usePadHandler } from "../nav/input";
import { feedback } from "../nav/feedback";
import { ItemDetail, type ItemDetailResult } from "../overlay/Overlay";
import { Button, Dot } from "../ui";
import { keystore, type RareAccount } from "../wallet";
import { SignRejected } from "../wallet/market";
import { Sheet } from "../wallet/ui/kit";
import { runClaim } from "./claim";
import { ApiFail } from "./methods";
import { finishSheet, useGameSheet, type ActiveSheet } from "./sheets";

function useAccount(): RareAccount | null | undefined {
  const [a, setA] = useState<RareAccount | null | undefined>(undefined);
  useEffect(() => {
    void keystore.current().then(setA, () => setA(null));
  }, []);
  return a;
}

export function GameSheets() {
  const sheet = useGameSheet((s) => s.sheet);
  if (!sheet) return null;
  return <ActiveGameSheet key={`${sheet.kind}:${sheet.reqId ?? "link"}`} sheet={sheet} />;
}

/** A game's sheet: its gas budget may pay the fee (game:<slug>). A link's sheet: the player or Rare pays. */
const payerOf = (s: ActiveSheet) => (s.reqId !== null && s.data.game ? { slug: s.data.game.slug, name: s.data.game.title } : undefined);

/** Who asked, for the sheet's subtitle: the game, or nobody (a link the player opened). */
const askedBy = (s: ActiveSheet) => (s.reqId !== null && s.data.game?.title ? `${s.data.game.title} asked Rare` : null);

function ActiveGameSheet({ sheet }: { sheet: ActiveSheet }) {
  const account = useAccount();
  // While the sheet is up the launcher's tab and panel buttons do nothing; B is the sheet's.
  usePadHandler((b) => ["lb", "rb", "lt", "rt", "x", "y", "select", "start", "qam"].includes(b));
  useEffect(() => {
    feedback("open");
  }, []);
  if (account === undefined) return null;
  switch (sheet.kind) {
    case "sell":
      return <SellSheet sheet={sheet} account={account} />;
    case "buy":
      return <BuyGameSheet sheet={sheet} account={account} />;
    case "claim":
      return <ClaimSheet sheet={sheet} account={account} />;
  }
}

function SellSheet({ sheet, account }: { sheet: Extract<ActiveSheet, { kind: "sell" }>; account: RareAccount | null }) {
  const { item, priceMist, game } = sheet.data;
  const [result, setResult] = useState<ItemDetailResult | null>(null);
  useEffect(() => {
    if (!result) return;
    // The player sees "Sold" or "Listed" for a moment, then the game gets the answer and the screen.
    const t = setTimeout(() => finishSheet(result), 1600);
    return () => clearTimeout(t);
  }, [result]);
  return (
    <Sheet title={`Sell ${shortName(item)}`} subtitle={askedBy(sheet) ?? item.collection?.name ?? game.title} width="34rem" onClose={() => finishSheet(result ?? { status: "cancelled" })} ready={result ? "done" : "open"}>
      <div className="py-[0.6rem]">
        <ItemDetail item={item} account={account} floors={{}} onDone={() => {}} onResult={setResult} initialPrice={priceMist} payer={payerOf(sheet)} />
      </div>
    </Sheet>
  );
}

function BuyGameSheet({ sheet, account }: { sheet: Extract<ActiveSheet, { kind: "buy" }>; account: RareAccount | null }) {
  const suiUsd = useLive().suiUsd ?? null;
  const [bought, setBought] = useState<string | null>(null);
  const { item, listing } = sheet.data;
  usePadHandler((b) => {
    if (b !== "b") return false;
    finishSheet(bought !== null ? { status: "bought", digest: bought, itemId: item.id } : { status: "cancelled" });
    return true;
  });
  return (
    <BuySheet
      item={item}
      listing={listing}
      account={account}
      suiUsd={suiUsd}
      onBought={setBought}
      payer={payerOf(sheet)}
      onClose={() => finishSheet(bought !== null ? { status: "bought", digest: bought, itemId: item.id } : { status: "cancelled" })}
    />
  );
}

type ClaimPhase = { at: "idle" } | { at: "working" } | { at: "error"; message: string } | { at: "done"; digest: string };

function ClaimSheet({ sheet, account }: { sheet: Extract<ActiveSheet, { kind: "claim" }>; account: RareAccount | null }) {
  const { voucher, title, game } = sheet.data;
  const [phase, setPhase] = useState<ClaimPhase>({ at: "idle" });
  const close = () => finishSheet(phase.at === "done" ? { status: "claimed", digest: phase.digest } : { status: "cancelled" });
  useEffect(() => {
    if (phase.at !== "done") return;
    const t = setTimeout(() => finishSheet({ status: "claimed", digest: phase.digest }), 1600);
    return () => clearTimeout(t);
  }, [phase]);
  const claim = async () => {
    if (!account || phase.at === "working" || phase.at === "done") return;
    setPhase({ at: "working" });
    try {
      const r = await runClaim(sheet.data, account.address, { name: game?.title ?? "Rare" });
      setPhase({ at: "done", digest: r.digest });
      feedback("notify");
    } catch (e) {
      if (e instanceof SignRejected) return setPhase({ at: "idle" });
      if (e instanceof ApiFail && e.code === "unavailable") return finishSheet({ status: "unavailable", reason: e.message });
      setPhase({ at: "error", message: (e as Error)?.message || "The claim didn't go through" });
    }
  };
  const what =
    voucher.type === "trophy"
      ? `Trophy ${voucher.trophyId.toString()}`
      : voucher.type === "item"
        ? `Item kind ${voucher.kind.toString()}${voucher.keys.length ? ` with ${voucher.keys.map((k, i) => `${k} ${voucher.vals[i]}`).join(", ")}` : ""}`
        : voucher.keys.map((k, i) => `${k} becomes ${voucher.vals[i]}`).join(", ");
  return (
    <Sheet
      title={phase.at === "done" ? "Claimed" : title}
      subtitle={askedBy(sheet) ?? "From a Rare link"}
      width="34rem"
      onClose={close}
      ready={phase.at}
      footer={
        phase.at === "done" ? null : (
          <div className="flex gap-[0.6rem]">
            <Button variant="primary" size="md" def className="flex-1" disabled={phase.at === "working" || !account} hint="Claim" onClick={() => void claim()}>
              {phase.at === "working" ? "Waiting for approval" : phase.at === "error" ? "Try again" : "Claim"}
            </Button>
            <Button size="md" onClick={close}>
              Not now
            </Button>
          </div>
        )
      }
    >
      <div className="flex flex-col gap-[0.8rem] px-8 py-6 text-small">
        <p className="text-body">{what}</p>
        <p className="text-secondary">{voucher.type === "trophy" ? "Goes to your Rare profile. Rare covers the network fee when your wallet is short." : "Goes to your Rare Wallet. You review it before anything is signed."}</p>
        {phase.at === "done" && (
          <p className="flex items-center gap-[0.6rem]">
            <Dot tone="positive" /> Done. It can take a minute to show.
          </p>
        )}
        {phase.at === "error" && <p className="text-negative">{phase.message}</p>}
        {!account && <p className="text-secondary">Add a Rare Wallet account to claim.</p>}
      </div>
    </Sheet>
  );
}

