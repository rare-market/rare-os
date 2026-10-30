// Receive: big QR of the address, the full address, Copy, and a note for how this account signs.

import { Check, Copy } from "lucide-react";
import { useState } from "react";
import { Btn, Page, Panel, Qr, useCopy } from "../kit";
import { useWallet, type WalletView } from "../store";
import { kindLabel, useHints } from "../home/lib";
import { verifyOnLedger, type LedgerCheck } from "../home/hw";

export function ReceivePage({ view, active }: { view: Extract<WalletView, { kind: "receive" }>; active: boolean }) {
  void view;
  const account = useWallet((s) => s.current);
  const [done, copy] = useCopy();
  const [check, setCheck] = useState<LedgerCheck | "busy" | null>(null);
  useHints(active, [{ b: "a", label: "Select" }, { b: "b", label: "Back" }]);
  if (!account) return null;

  const verify = async () => {
    setCheck("busy");
    setCheck(await verifyOnLedger(account.address, account.ledger?.path));
  };

  return (
    <Page active={active} ready={account.address} title="Receive" subtitle={`${account.label}, ${kindLabel(account).toLowerCase()}`}>
      <div className="mt-2 grid grid-cols-[auto_minmax(0,1fr)] items-center gap-12">
        <Qr text={account.address} className="size-[20rem]" />
        <div className="min-w-0">
          <div className="text-sm text-secondary">Your address</div>
          <Panel className="mt-2 px-5 py-4">
            <p className="break-all font-mono text-lg leading-snug">{account.address}</p>
          </Panel>
          <div className="mt-4 flex flex-wrap gap-2">
            <Btn variant="primary" def onClick={() => copy(account.address)}>
              {done ? <Check size="1.1rem" /> : <Copy size="1.1rem" />}
              {done ? "Copied" : "Copy address"}
            </Btn>
            {account.kind === "ledger" && (
              <Btn onClick={() => void verify()}>{check === "busy" ? "Check your Ledger" : "Verify on Ledger"}</Btn>
            )}
          </div>

          {check && check !== "busy" && (
            <div className="mt-3 text-sm">
              {check.state === "ok" && <p className="text-secondary">Your Ledger shows the same address.</p>}
              {check.state === "different" && <p className="text-negative">Your Ledger shows a different address. Don't use this one.</p>}
              {check.state === "error" && (
                <>
                  <p className="text-negative">{check.message}</p>
                  {check.fix && <p className="mt-2 break-all rounded border border-border-primary bg-layer-01 p-3 font-mono text-xs text-secondary">{check.fix}</p>}
                </>
              )}
            </div>
          )}

          <ul className="mt-6 flex flex-col gap-1.5 text-sm text-secondary">
            <li>Only send Sui network assets to this address.</li>
            {account.kind === "watch" && <li>This wallet is watch only. You can receive here, but you can't send from it.</li>}
            {account.kind === "ledger" && <li>Verify on Ledger shows this address on the device so you can match it before sharing.</li>}
            {account.kind === "keystone" && <li>This address belongs to your Keystone. Sending needs the device.</li>}
          </ul>
        </div>
      </div>
    </Page>
  );
}
