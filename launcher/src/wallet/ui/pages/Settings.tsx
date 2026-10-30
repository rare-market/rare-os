// Settings: lock and auto-lock, PIN, recovery phrase and key, connected games, hardware help,
// and removing this account or every wallet from the device.

import { Gamepad2, KeyRound, Lock, Plug, ShieldAlert, Trash2, Eye, FileText } from "lucide-react";
import { useEffect, useState } from "react";
import { useApp } from "../../../lib/store";
import { keystore } from "../../index";
import { ListButton, Page, Panel, Segmented } from "../kit";
import { useWallet, type WalletView } from "../store";
import { AUTOLOCK_OPTIONS, getAutoLock, setAutoLock } from "../home/autoLock";
import { ChangePinSheet, HardwareHelpSheet, RemoveAccountSheet, RemoveAllSheet, RevealSheet } from "../home/Sheets";
import { useHints } from "../home/lib";
import { approvedOrigins, hostOf, removeOrigin } from "../home/origins";

type Open = null | "pin" | "phrase" | "key" | "hw" | "remove" | "removeAll";

export function SettingsPage({ view, active }: { view: Extract<WalletView, { kind: "settings" }>; active: boolean }) {
  void view;
  const account = useWallet((s) => s.current);
  const vault = useWallet((s) => s.vault);
  const toast = useApp((s) => s.toast);
  const [open, setOpen] = useState<Open>(null);
  const [auto, setAuto] = useState(getAutoLock());
  const [hasPhrase, setHasPhrase] = useState(false);
  const [origins, setOrigins] = useState<string[]>(approvedOrigins());
  useHints(active, [{ b: "a", label: "Select" }, { b: "b", label: "Back" }]);

  useEffect(() => {
    setHasPhrase(false);
    if (account) void keystore.hasPhrase(account.address).then(setHasPhrase);
  }, [account?.address]); // eslint-disable-line react-hooks/exhaustive-deps

  const hasVault = !!vault?.exists;
  const noun = vault?.kind === "passphrase" ? "passphrase" : "PIN";
  const hasKey = account?.kind === "native";
  const lockNow = async () => {
    await keystore.lock().catch(() => {});
    await useWallet.getState().refresh();
    toast("Wallet locked");
  };
  const label = (m: number) => (m === 0 ? "Off" : `${m} min`);
  const close = () => setOpen(null);

  const row = "divide-y divide-border-primary";
  return (
    <Page active={active} ready={account?.address ?? 0} title="Settings" subtitle={account ? `${account.label}` : undefined}>
      <div className="grid grid-cols-2 items-start gap-6">
        <div className="flex min-w-0 flex-col gap-6">
          <Panel title="Security" className="pb-2">
            <div className={`mt-2 ${row}`}>
              {hasVault && <ListButton def icon={<Lock size="1.3rem" />} title="Lock now" body="Ask for your PIN next time" onClick={() => void lockNow()} />}
              {hasVault && (
                <div className="flex items-center gap-4 px-4 py-3">
                  <span className="flex w-[1.3rem] shrink-0 justify-center text-icon-primary">
                    <Plug size="1.3rem" />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block text-base font-medium">Auto-lock</span>
                    <span className="block text-sm text-secondary">Lock after being idle</span>
                  </span>
                  <Segmented
                    items={AUTOLOCK_OPTIONS.map((m) => ({ value: String(m), label: label(m) }))}
                    value={String(auto)}
                    onChange={(v) => {
                      setAuto(Number(v));
                      setAutoLock(Number(v));
                    }}
                  />
                </div>
              )}
              {hasVault && <ListButton icon={<KeyRound size="1.3rem" />} title={`Change ${noun}`} onClick={() => setOpen("pin")} />}
              {hasPhrase && <ListButton icon={<FileText size="1.3rem" />} title="Show recovery phrase" body="Needs your PIN" onClick={() => setOpen("phrase")} />}
              {hasKey && <ListButton icon={<Eye size="1.3rem" />} title="Show private key" body="Needs your PIN" onClick={() => setOpen("key")} />}
            </div>
            <p className="px-5 pb-3 pt-3 text-sm text-secondary">
              {vault?.backend === "keyring" ? "Keys are stored in your system keyring." : vault?.backend === "file" ? "Keys are stored encrypted on this device." : "No keys are stored on this device yet."}
            </p>
          </Panel>

          <Panel title="Connected games" className="pb-2">
            <div className={`mt-2 ${row}`}>
              {origins.length === 0 && (
                <p className="flex items-center gap-4 px-4 py-3.5 text-sm text-secondary">
                  <Gamepad2 size="1.3rem" /> No games are connected to this wallet.
                </p>
              )}
              {origins.map((o) => (
                <ListButton
                  key={o}
                  icon={<Gamepad2 size="1.3rem" />}
                  title={hostOf(o)}
                  right="Remove"
                  onClick={() => {
                    void removeOrigin(o).then(() => setOrigins(approvedOrigins()));
                  }}
                />
              ))}
            </div>
          </Panel>
        </div>

        <div className="flex min-w-0 flex-col gap-6">
          <Panel title="Hardware wallets" className="pb-2">
            <div className="mt-2">
              <ListButton icon={<Plug size="1.3rem" />} title="Ledger and Keystone help" body="Fix USB permission and check your device" onClick={() => setOpen("hw")} />
            </div>
          </Panel>
          <Panel title="Remove" className="pb-2">
            <div className={`mt-2 ${row}`}>
              {account && <ListButton icon={<Trash2 size="1.3rem" />} title="Remove this account" body={account.label} onClick={() => setOpen("remove")} />}
              <ListButton icon={<ShieldAlert size="1.3rem" />} title="Remove all wallets from this device" body="Erases every account and key here" onClick={() => setOpen("removeAll")} />
            </div>
          </Panel>
        </div>
      </div>

      {open === "pin" && <ChangePinSheet onClose={close} />}
      {(open === "phrase" || open === "key") && account && <RevealSheet account={account} what={open} onClose={close} />}
      {open === "hw" && <HardwareHelpSheet onClose={close} />}
      {open === "remove" && account && <RemoveAccountSheet account={account} onClose={close} onDone={() => useWallet.getState().refresh()} />}
      {open === "removeAll" && <RemoveAllSheet onClose={close} />}
    </Page>
  );
}
