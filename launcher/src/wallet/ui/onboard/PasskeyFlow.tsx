// The passkey flow itself (choose phone or key, device PIN, live ceremony, landed note), shared by
// the Passkey page and the launcher's first-run step. The host supplies the frame and what to do
// when an account lands.

import { Smartphone, Usb } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { keystore } from "../../index";
import { NativeError, passkeyNative } from "../../native";
import { Btn, cn } from "../kit";
import { plainError, udevFix, useStepBack, vaultGate } from "./flow";
import { CeremonyView } from "./Ceremony";
import { HwError } from "./HwParts";
import { createPasskeyAccount, signInWithPasskey, useCeremony, usePasskeyPrefs } from "./passkeyApi";
import { PinSetup } from "./PinSetup";
import { BluetoothStep, bluetoothReady } from "./BluetoothStep";
import type { PinOptions } from "../../core/keys/keystore";

export type PasskeyStep = "choose" | "pin" | "bluetooth" | "run" | "device";

function Choice({ on, icon, title, body, onClick, def }: { on: boolean; icon: React.ReactNode; title: string; body: string; onClick: () => void; def?: boolean }) {
  return (
    <button
      data-nav
      data-nav-default={def ? "" : undefined}
      role="radio"
      aria-checked={on}
      onClick={onClick}
      className={cn("flex w-full items-center gap-4 rounded border px-5 py-4 text-left transition-colors", on ? "border-border-light bg-layer-02" : "border-border-primary bg-layer-01")}
    >
      <span className="text-icon-primary">{icon}</span>
      <span className="min-w-0 flex-1">
        <span className="block text-base font-medium">{title}</span>
        <span className="mt-0.5 block text-sm text-secondary">{body}</span>
      </span>
      <span className={cn("flex size-5 shrink-0 items-center justify-center rounded-full border", on ? "border-primary" : "border-border-light")}>
        {on && <span className="size-2.5 rounded-full bg-primary" />}
      </span>
    </button>
  );
}


export function PasskeyFlow({
  create,
  active,
  onFinish,
  onStep,
  onExit,
}: {
  create: boolean;
  active: boolean;
  /** an account landed (selected by the caller) */
  onFinish: (address: string) => void;
  onStep?: (s: PasskeyStep) => void;
  /** B on the first step; without it B falls through to the host */
  onExit?: () => void;
}) {
  const via = usePasskeyPrefs((s) => s.via);
  const pick = usePasskeyPrefs((s) => s.setVia);
  const [step, setStepState] = useState<PasskeyStep>("choose");
  const [error, setError] = useState<{ message: string; fix: string | null } | null>(null);
  const [landed, setLanded] = useState<string | null>(null);
  const run = useRef(0);
  // remembers the PIN (create) across the "Turn on Bluetooth" step
  const pendingPin = useRef<PinOptions | undefined>(undefined);
  const btNote = useRef<string | undefined>(undefined);
  const { state, qrUrl } = useCeremony();
  const setStep = (s: PasskeyStep) => setStepState(s);
  useEffect(() => onStep?.(step), [step]); // eslint-disable-line react-hooks/exhaustive-deps

  const cancel = () => {
    run.current++;
    void passkeyNative.cancel();
    setStep("choose");
  };

  const start = async (pin?: PinOptions) => {
    const id = ++run.current;
    setError(null);
    setStep("run");
    try {
      const acct = create ? await createPasskeyAccount({ via, pin }) : await signInWithPasskey({ via });
      if (id !== run.current) return;
      if (!create && !(await keystore.hasDeviceKey(acct.address).catch(() => false))) {
        setLanded(acct.address);
        setStep("device");
        return;
      }
      onFinish(acct.address);
    } catch (e) {
      if (id !== run.current) return;
      if (e instanceof NativeError && e.code === "cancelled") {
        setStep("choose");
        return;
      }
      // Bluetooth was off or got turned off mid-flow: offer to turn it back on.
      if (via === "phone" && e instanceof NativeError && e.code === "ble-off") {
        btNote.current = "Turn Bluetooth back on to finish with your phone.";
        setStep("bluetooth");
        return;
      }
      const none = e instanceof NativeError && e.code === "notfound";
      setError({
        message: none ? (via === "usb" ? "No security key found. Insert it and try again." : "No Rare passkey found on that phone.") : plainError(e, "Could not use the passkey. Try again."),
        fix: udevFix(e),
      });
      setStep("choose");
    }
  };

  // The phone passkey talks over Bluetooth; make sure the adapter is on before the ceremony.
  const proceed = async (pin?: PinOptions) => {
    pendingPin.current = pin;
    if (via === "phone" && !(await bluetoothReady())) {
      btNote.current = undefined;
      setStep("bluetooth");
      return;
    }
    void start(pin);
  };

  const next = async () => {
    if (!create) return void proceed();
    const gate = await vaultGate("Enter your PIN to add this account");
    if (gate === "pin") setStep("pin");
    else if (gate === "ready") void proceed();
  };

  useStepBack(active, step === "pin" || step === "bluetooth" || step === "run" || (step === "choose" && !!onExit), () =>
    step === "run" ? cancel() : step === "pin" || step === "bluetooth" ? setStep("choose") : onExit?.(),
  );

  return (
    <>
      {step === "choose" && (
        <div className="flex flex-col gap-4">
          <div role="radiogroup" className="flex flex-col gap-2">
            <Choice def on={via === "phone"} icon={<Smartphone size="1.5rem" />} title={create ? "Use your phone" : "Sign in with your phone"} body="Scan a code with your phone's camera, then use Face ID or your fingerprint" onClick={() => pick("phone")} />
            <Choice on={via === "usb"} icon={<Usb size="1.5rem" />} title="Use a security key" body="A USB or NFC key such as a YubiKey. No camera needed." onClick={() => pick("usb")} />
          </div>
          {create && <p className="text-sm text-secondary">Your passkey can always sign. This device signs everyday transactions with your PIN.</p>}
          {error && <HwError message={error.message} fix={error.fix} />}
          <Btn variant="primary" className="w-full" onClick={() => void next()}>
            Continue
          </Btn>
        </div>
      )}

      {step === "pin" && (
        <div className="pt-4">
          <PinSetup onDone={(pin) => void start(pin)} title="Set a device PIN" body="This device signs everyday transactions with it. Your passkey can always sign too." />
        </div>
      )}

      {step === "bluetooth" && <BluetoothStep note={btNote.current} onReady={() => void start(pendingPin.current)} onCancel={() => setStep("choose")} />}

      {step === "run" && (
        <div className="pt-2">
          <CeremonyView via={via} state={state} qrUrl={qrUrl} onCancel={cancel} />
        </div>
      )}

      {step === "device" && (
        <div className="flex flex-col items-center gap-4 py-10 text-center">
          <h2 className="text-xl font-bold">You are signed in</h2>
          <p className="max-w-[28rem] text-secondary">This device will ask your phone to approve each transaction.</p>
          <Btn variant="primary" def className="mt-2 w-64" onClick={() => landed && onFinish(landed)}>
            Continue
          </Btn>
        </div>
      )}
    </>
  );
}
