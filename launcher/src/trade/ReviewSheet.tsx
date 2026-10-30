// Review before signing: what the trade does, then the unsigned transaction is built and checked
// with a gRPC dry run (nothing is signed or sent). Confirm hands it to Rare Wallet, whose own
// review sheet asks for the approval. B closes.
//
// With `sponsor`, Rare covers the fee when the wallet has too little SUI for it and rare.market
// offers that category to this address (a first swap). Any "no" from the sponsor falls back to
// the wallet paying its own fee.

import type { Transaction } from "@mysten/sui/transactions";
import { fromBase64 } from "@mysten/sui/utils";
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { RareAccount } from "../wallet";
import { SUI_TYPE, fromBase } from "./amount";
import { coinBalance, dryRun, plainChainError, type DryRun } from "./chain";
import { useLandingFocus, useSheetPad } from "./nav";
import { LoadBar } from "../ui";
import { failed } from "../lib/fail";
import { Action, Line, Sheet } from "./parts";
import { canSign, isRejection, signAndExecute, signAndExecuteCovered } from "./wallet";
import { sponsorOffer, sponsorTransaction, type SponsorCategory, type Sponsorship } from "../wallet/sponsor";
import { AddPasskey } from "../wallet/ui/AddPasskey";

type Phase = { kind: "checking" } | { kind: "ready"; tx: Transaction; run: DryRun; covered?: Sponsorship } | { kind: "failed"; message: string } | { kind: "signing" } | { kind: "sent"; digest?: string; simulated: boolean };

export function ReviewSheet({
  title,
  sub,
  children,
  build,
  account,
  action,
  tone = "primary",
  onClose,
  onSent,
  sponsor,
}: {
  title: string;
  sub?: ReactNode;
  /** the summary lines */
  children: ReactNode;
  /** builds the unsigned transaction for the account; `sponsored` builds it for a fee Rare covers
   * (never touching the gas coin) */
  build: (sender: string, o: { sponsored: boolean }) => Promise<Transaction>;
  /** offer "Rare covers the fee" for this kind of action when the wallet is short of SUI */
  sponsor?: SponsorCategory;
  account: RareAccount | null;
  /** "Confirm buy", "Open long" */
  action: string;
  tone?: "buy" | "sell" | "primary";
  onClose: () => void;
  onSent?: () => void;
}) {
  const root = useRef<HTMLDivElement | null>(null);
  const [phase, setPhase] = useState<Phase>({ kind: "checking" });
  const sender = account?.address ?? null;
  const signer = canSign(account);
  useLandingFocus(root, true, phase.kind);
  // While Rare Wallet's own review sheet is up, it has the pad and the screen.
  useSheetPad(onClose, [{ b: "a", label: "Select" }, { b: "b", label: "Cancel" }], phase.kind !== "signing");

  useEffect(() => {
    if (!sender) return;
    let live = true;
    setPhase({ kind: "checking" });
    (async () => {
      // Rare covers the fee when the wallet can't and the sponsor offers it; otherwise as usual
      if (sponsor && (await sponsorOffer(sender, sponsor, { suiBalanceMist: () => coinBalance(sender, SUI_TYPE) }))) {
        try {
          const tx = await build(sender, { sponsored: true });
          const covered = await sponsorTransaction(tx, sender, sponsor);
          if (covered.ok) {
            const run = await dryRun(fromBase64(covered.bytes), sender);
            if (!live) return;
            if (run.ok) return setPhase({ kind: "ready", tx, run, covered });
          }
        } catch {
          /* the sponsor is never in the way: the wallet pays as usual */
        }
      }
      if (!live) return;
      const tx = await build(sender, { sponsored: false });
      const run = await dryRun(tx, sender);
      if (!live) return;
      setPhase(run.ok ? { kind: "ready", tx, run } : { kind: "failed", message: `The trade didn't go through: ${run.error ?? "it would fail right now"}` });
    })().catch((e: unknown) => live && setPhase({ kind: "failed", message: failed("Couldn't build this trade.", e) }));
    return () => {
      live = false;
    };
  }, [sender]); // eslint-disable-line react-hooks/exhaustive-deps

  const confirm = async () => {
    if (phase.kind !== "ready" || !sender) return;
    const { tx } = phase;
    setPhase({ kind: "signing" });
    try {
      const r = phase.covered ? await signAndExecuteCovered(tx, phase.covered, sponsor!, { account: sender, title }) : await signAndExecute(tx, { account: sender, title });
      setPhase({ kind: "sent", digest: r.digest, simulated: !!r.dryRun });
      if (!r.dryRun) onSent?.();
    } catch (e) {
      if (isRejection(e)) setPhase(phase);
      else setPhase({ kind: "failed", message: `The trade didn't go through: ${plainChainError(e)}` });
    }
  };

  const gas = phase.kind === "ready" ? phase.run.gasMist : null;
  const covered = phase.kind === "ready" && !!phase.covered;
  if (phase.kind === "signing") return null;
  const ready = phase.kind === "ready";
  return (
    <Sheet
      title={title}
      sub={sub}
      onRef={(el) => void (root.current = el)}
      footer={
        phase.kind === "sent" ? (
          <Action def tone="secondary" className="flex-1" onClick={onClose}>
            Done
          </Action>
        ) : (
          <>
            <Action def={ready && signer} tone={tone} disabled={!ready || !signer} className="flex-1" onClick={() => void confirm()} hint={ready && signer ? "Confirm" : undefined}>
              {action}
            </Action>
            <Action def={!ready || !signer} tone="secondary" onClick={onClose}>
              Cancel
            </Action>
          </>
        )
      }
    >
      <div className="relative">
        <LoadBar on={phase.kind === "checking"} className="-top-[1.25rem]" />
        <div>
          {children}
          {covered && <Line label="Network gas">Covered by Rare</Line>}
          {!covered && gas != null && <Line label="Network gas">{`about ${Number(fromBase(gas > 0n ? gas : 0n, 9)).toPrecision(2)} SUI`}</Line>}
        </div>
        <div className="mt-[1rem] min-h-[2.5rem] text-small">
          {!sender ? (
            <AddPasskey need="trade" account={null} />
          ) : phase.kind === "checking" ? (
            <span className="text-secondary">Checking the trade...</span>
          ) : phase.kind === "ready" ? (
            signer ? (
              <span className="text-secondary">Checked against the network just now. Rare Wallet shows the final details before anything is signed.</span>
            ) : (
              <AddPasskey need="trade" account={account} />
            )
          ) : phase.kind === "failed" ? (
            <span className="text-negative">{phase.message}</span>
          ) : phase.kind === "sent" && phase.simulated ? (
            <span className="text-secondary">Signed as a test and checked against the network. Nothing was sent.</span>
          ) : (
            <span>Sent. It shows in Positions once the network has it.</span>
          )}
        </div>
      </div>
    </Sheet>
  );
}
