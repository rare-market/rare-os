// What the person sees while a passkey ceremony runs: a QR for the phone (hybrid / caBLE over
// Bluetooth), or "insert your security key" with a PIN pad when the key asks for its PIN.

import { useState } from "react";
import { MarketLoadBar } from "../../../marketplace/parts";
import { passkeyNative, type PasskeyVia } from "../../native";
import { Btn, PinPad, Qr, Sheet } from "../kit";
import type { Ceremony } from "./passkeyApi";

export function CeremonyView({ via, state, qrUrl, onCancel }: { via: PasskeyVia; state: Ceremony; qrUrl: string | null; onCancel: () => void }) {
  const [pin, setPin] = useState("");

  if (state.kind === "pin") {
    const submit = (v: string) => {
      setPin("");
      void passkeyNative.pin(v);
    };
    return (
      <Sheet
        title="Security key PIN"
        subtitle={state.attemptsLeft != null ? `${state.attemptsLeft} tries left` : "Enter the PIN you set on your key"}
        onClose={() => {
          setPin("");
          void passkeyNative.pin(null);
        }}
        width="30rem"
      >
        <PinPad value={pin} onChange={setPin} onSubmit={submit} minLength={4} maxLength={12} />
      </Sheet>
    );
  }

  // The QR stays until the phone has scanned it (the key exchange moves past "waiting" and "bluetooth").
  const scanned = state.kind === "status" && (state.stage === "connecting" || state.stage === "touch" || state.stage === "processing");
  const showQr = via === "phone" && !!qrUrl && !scanned;
  const status = state.kind === "status" ? state.message : null;

  return (
    <div className="flex flex-col items-center gap-5 text-center">
      {state.kind === "idle" && <MarketLoadBar />}
      {via === "phone" ? (
        showQr ? (
          <>
            <Qr text={qrUrl!} className="size-[19rem]" />
            <p className="max-w-[30rem] text-base">Scan this code with your phone's camera, then approve with Face ID or your fingerprint.</p>
            <p className="max-w-[30rem] text-sm text-secondary">Your phone connects to this SuiPlay over Bluetooth, so keep Bluetooth on for both.</p>
            {status && <p className="text-sm text-secondary">{status}</p>}
          </>
        ) : (
          <div className="py-14">
            <p className="text-xl font-semibold">{scanned ? "Approve on your phone" : "Getting ready"}</p>
            <p className="mt-2 text-sm text-secondary">{status ?? "Starting the connection"}</p>
          </div>
        )
      ) : (
        <div className="py-12">
          <p className="text-xl font-semibold">Insert your security key and touch it</p>
          <p className="mt-2 text-sm text-secondary">{status ?? "Plug it into the USB port. Touch it when it blinks."}</p>
        </div>
      )}
      <Btn variant="ghost" size="md" onClick={onCancel}>
        Cancel
      </Btn>
    </div>
  );
}
