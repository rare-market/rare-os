// Move assets out of a Playtron (GameOS) wallet while its sign-in still works: the same
// step-by-step as the first-run step (src/onboarding/move): your address, the Playtron address,
// a read-only scan, the move plan in signing order, and check again.

import { MoveAssets } from "../../../onboarding/move/MoveAssets";
import { H, useHints } from "../onboard/flow";
import { useWalletNav, type WalletView } from "../store";

export function PlaytronPage({ view, active }: { view: Extract<WalletView, { kind: "playtron" }>; active: boolean }) {
  void view;
  useHints(active, [H.select, H.back]);
  const nav = useWalletNav.getState();
  return (
    <div className="h-full px-[2.5rem]">
      <MoveAssets active={active} onExit={() => nav.pop()} onNeedWallet={() => nav.replace({ kind: "add" })} />
    </div>
  );
}
