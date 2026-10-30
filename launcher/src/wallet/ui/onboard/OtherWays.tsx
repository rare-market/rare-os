// "Other ways to sign in": the same list on Welcome, the Add account page and the launcher's
// sign-in overlay (which opens the wallet tab on the page it picks).

import { Eye, FileText, KeyRound, ShieldCheck, Usb } from "lucide-react";
import { ListButton, Panel } from "../kit";
import type { WalletView } from "../store";

export const OTHER_WAYS: Array<{ view: WalletView; title: string; body: string; icon: typeof Eye }> = [
  { view: { kind: "import", what: "phrase" }, title: "Recovery phrase", body: "Use the 12 or 24 words from another wallet", icon: FileText },
  { view: { kind: "import", what: "key" }, title: "Private key", body: "Paste a suiprivkey... key", icon: KeyRound },
  { view: { kind: "ledger" }, title: "Ledger", body: "Nano S Plus, Nano X, Stax and Flex over USB or Bluetooth", icon: Usb },
  { view: { kind: "keystone" }, title: "Keystone", body: "Keystone 3 Pro over a USB-C cable", icon: ShieldCheck },
  { view: { kind: "watch" }, title: "Watch an address", body: "Follow any wallet. You cannot send from it", icon: Eye },
];

export function OtherWays({ onPick, firstDefault = false }: { onPick: (v: WalletView) => void; firstDefault?: boolean }) {
  return (
    <Panel className="divide-y divide-border-primary p-1">
      {OTHER_WAYS.map((w, i) => (
        <ListButton key={w.title} def={firstDefault && i === 0} icon={<w.icon size="1.4rem" />} title={w.title} body={w.body} onClick={() => onPick(w.view)} />
      ))}
    </Panel>
  );
}
