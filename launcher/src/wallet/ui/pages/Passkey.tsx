// Sign in with a passkey, or create an account. The passkey lives on a phone (scan a QR, talk over
// Bluetooth) or on a USB security key. A passkey account is a 1-of-2 multisig of the passkey and a
// key on this device: the passkey can always sign, and this device signs everyday transactions
// with its PIN.

import { useState } from "react";
import { Page } from "../kit";
import { H, finishAdd, useHints } from "../onboard/flow";
import { PasskeyFlow, type PasskeyStep } from "../onboard/PasskeyFlow";
import type { WalletView } from "../store";

export function PasskeyPage({ view, active }: { view: Extract<WalletView, { kind: "passkey" }>; active: boolean }) {
  const create = view.mode === "create";
  const [step, setStep] = useState<PasskeyStep>("choose");
  useHints(active, step === "pin" ? [H.select, H.del, H.ok, H.back] : [H.select, H.back]);
  return (
    <Page
      active={active}
      narrow
      title={step === "pin" || step === "device" ? undefined : create ? "Create account" : "Sign in with a passkey"}
      subtitle={step === "choose" ? (create ? "Make a new wallet protected by a passkey. There are no words to write down." : "Use the passkey you made before to get your wallet back.") : undefined}
      ready={step}
    >
      <PasskeyFlow create={create} active={active} onStep={setStep} onFinish={(a) => void finishAdd(a)} />
    </Page>
  );
}
