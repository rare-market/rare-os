// Sets the PIN (or a passphrase) that protects keys on this device: enter it, then enter it again.
// Used by every flow that creates the vault: passkey accounts, new or imported phrases and keys.

import { useState } from "react";
import { Btn, Field, PinPad, cn } from "../kit";
import type { PinOptions } from "../../core/keys/keystore";

export const PIN_LENGTH = 6;

export function PinSetup({
  onDone,
  title = "Set a PIN",
  body = "It unlocks your keys on this device. It never leaves it.",
  busy = false,
}: {
  onDone: (pin: PinOptions) => void;
  title?: string;
  body?: string;
  busy?: boolean;
}) {
  const [mode, setMode] = useState<"pin" | "passphrase">("pin");
  const [first, setFirst] = useState<string | null>(null);
  const [value, setValue] = useState("");
  const [pass, setPass] = useState("");
  const [pass2, setPass2] = useState("");
  const [error, setError] = useState("");

  const submitPin = (v: string) => {
    if (first === null) {
      setFirst(v);
      setValue("");
      setError("");
    } else if (v === first) {
      onDone({ kind: "pin", secret: v });
    } else {
      setFirst(null);
      setValue("");
      setError("Those PINs did not match. Start again.");
    }
  };

  const switchTo = (m: "pin" | "passphrase") => {
    setMode(m);
    setFirst(null);
    setValue("");
    setPass("");
    setPass2("");
    setError("");
  };

  return (
    <div className="flex flex-col items-center text-center">
      <h2 className="text-xl font-bold">{mode === "pin" ? (first === null ? title : "Enter it again") : "Choose a passphrase"}</h2>
      <p className="mt-1 max-w-[28rem] text-sm text-secondary">
        {mode === "pin" ? (first === null ? body : "Type the same 6 digits to confirm.") : "At least 8 characters. It unlocks your keys on this device."}
      </p>
      <div className="mt-6 w-full">
        {mode === "pin" ? (
          <PinPad key={first === null ? "a" : "b"} length={PIN_LENGTH} value={value} onChange={setValue} onSubmit={submitPin} disabled={busy} />
        ) : (
          <div className="mx-auto flex max-w-[28rem] flex-col gap-4 text-left">
            <Field def label="Passphrase" secret value={pass} onChange={setPass} placeholder="At least 8 characters" />
            <Field label="Confirm passphrase" secret value={pass2} onChange={setPass2} placeholder="Type it again" error={pass2 && pass !== pass2 ? "These do not match" : undefined} />
            <Btn variant="primary" onClick={() => onDone({ kind: "passphrase", secret: pass })} className={cn("w-full", (pass.length < 8 || pass !== pass2 || busy) && "pointer-events-none opacity-40")}>
              Continue
            </Btn>
          </div>
        )}
      </div>
      {error && <p className="mt-4 text-sm text-negative">{error}</p>}
      <div className="mt-6">
        <Btn variant="ghost" size="md" onClick={() => switchTo(mode === "pin" ? "passphrase" : "pin")}>
          {mode === "pin" ? "Use a passphrase instead" : "Use a 6 digit PIN instead"}
        </Btn>
      </div>
    </div>
  );
}
