// The PIN prompt as a sheet: shows why, takes the PIN on the kit pad (or a passphrase on the
// keyboard when the vault uses one), and counts wrong tries and waits in plain words.

import { useEffect, useState } from "react";
import { keystore } from "../../index";
import { NativeError } from "../../native";
import { Btn, Field, PinPad, Sheet } from "../kit";
import { useWallet } from "../store";
import { finishUnlock, useUnlock } from "../unlock";
import { waitText, wrongMessage } from "./unlockCopy";

function UnlockBody({ reason }: { reason: string }) {
  const kind = useWallet((w) => w.vault?.kind) ?? "pin";
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [until, setUntil] = useState(0);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (!until) return;
    const t = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(t);
  }, [until]);
  const wait = Math.max(0, until - now);
  useEffect(() => {
    if (until && wait === 0) {
      setUntil(0);
      setError(null);
    }
  }, [wait, until]);

  const submit = async (secret: string) => {
    if (busy || wait > 0) return;
    setBusy(true);
    setError(null);
    try {
      await keystore.unlock(secret);
      void useWallet.getState().refresh();
      finishUnlock(true);
    } catch (e) {
      setValue("");
      if (e instanceof NativeError && e.code === "wrong") {
        const retry = Number(e.data?.retryAfterMs ?? 0);
        if (retry > 0) {
          setUntil(Date.now() + retry);
          setNow(Date.now());
        } else setError(wrongMessage(e.data?.failures, kind === "passphrase" ? "passphrase" : "PIN"));
      } else if (e instanceof NativeError && e.code === "throttled") {
        setUntil(Date.now() + Number(e.data?.retryAfterMs ?? 5000));
        setNow(Date.now());
      } else setError(e instanceof Error ? e.message : "Could not unlock");
    } finally {
      setBusy(false);
    }
  };

  const waiting = wait > 0;
  return (
    <div className="flex flex-col items-center gap-5">
      <p className="text-center text-base text-secondary">{reason}</p>
      {kind === "passphrase" ? (
        <div className="w-full">
          <Field label="Passphrase" value={value} onChange={(v) => { setValue(v); if (v) void submit(v); }} secret def />
        </div>
      ) : (
        <PinPad value={value} onChange={setValue} onSubmit={(v) => void submit(v)} disabled={busy || waiting} />
      )}
      <p className="h-5 text-center text-sm" role="status">
        {waiting ? <span className="text-secondary">Too many tries. Try again in {waitText(wait)}.</span> : error ? <span className="text-negative">{error}</span> : null}
      </p>
    </div>
  );
}

export function UnlockSheet() {
  const pending = useUnlock((s) => s.pending);
  if (!pending) return null;
  return (
    <Sheet
      title="Unlock Rare Wallet"
      footer={
        <Btn className="flex-1" onClick={() => finishUnlock(false)}>
          Cancel
        </Btn>
      }
      onClose={() => finishUnlock(false)}
      width="28rem"
    >
      <UnlockBody reason={pending.reason} />
    </Sheet>
  );
}
