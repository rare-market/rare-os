// Sheets used by Settings and Accounts: PIN entry, reveal recovery phrase or key, change PIN,
// remove an account, remove everything, hardware wallet help.

import { useEffect, useState } from "react";
import { keystore, type RareAccount } from "../../index";
import { useApp } from "../../../lib/store";
import { Btn, Field, PinPad, Sheet, useCopy } from "../kit";
import { useWallet, useWalletNav } from "../store";
import { ensureUnlockedFor } from "../unlock";
import { secretError } from "./bits";
import { checkLedger, type LedgerCheck } from "./hw";
import { kindLabel } from "./lib";

/** PIN pad (or passphrase field) that hands the typed secret to `onSubmit`; a string back is the error to show. */
export function SecretEntry({ kind, prompt, onSubmit, okLabel = "OK" }: { kind: "pin" | "passphrase" | null; prompt: string; onSubmit: (secret: string) => Promise<string | null>; okLabel?: string }) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (v: string) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    const err = await onSubmit(v);
    setBusy(false);
    if (err) {
      setError(err);
      setValue("");
    }
  };
  return (
    <div className="flex flex-col items-center gap-5">
      <p className="text-center text-base text-secondary">{prompt}</p>
      {kind === "passphrase" ? (
        <div className="flex w-full max-w-[26rem] flex-col gap-3">
          <Field secret def value={value} onChange={setValue} placeholder="Passphrase" prompt="Passphrase" />
          <Btn variant="primary" onClick={() => void submit(value)}>
            {okLabel}
          </Btn>
        </div>
      ) : (
        <PinPad value={value} onChange={setValue} onSubmit={(v) => void submit(v)} disabled={busy} okLabel={okLabel} />
      )}
      {error && <p className="text-center text-sm text-negative">{error}</p>}
    </div>
  );
}

export function RevealSheet({ account, what, onClose }: { account: RareAccount; what: "phrase" | "key"; onClose: () => void }) {
  const kind = useWallet((s) => s.vault?.kind ?? "pin");
  const [secret, setSecret] = useState<string | null>(null);
  const [done, copy] = useCopy();
  // the secret leaves memory when the sheet goes
  useEffect(() => () => setSecret(null), []);
  const words = what === "phrase" && secret ? secret.split(/\s+/) : [];
  const it = what === "phrase" ? "them" : "it";
  return (
    <Sheet
      title={what === "phrase" ? "Recovery phrase" : "Private key"}
      subtitle={account.label}
      onClose={onClose}
      width={secret ? "56rem" : "36rem"}
      ready={secret ? 1 : 0}
      footer={
        secret ? (
          <>
            <Btn variant="primary" def className="flex-1" onClick={onClose}>
              Done
            </Btn>
            {what === "key" && <Btn onClick={() => copy(secret)}>{done ? "Copied" : "Copy key"}</Btn>}
          </>
        ) : undefined
      }
    >
      {!secret ? (
        <SecretEntry
          kind={kind}
          prompt={kind === "passphrase" ? "Enter your passphrase to continue" : "Enter your PIN to continue"}
          onSubmit={async (s) => {
            try {
              setSecret(what === "phrase" ? await keystore.revealMnemonic(s, account.address) : await keystore.revealPrivateKey(s, account.address));
              return null;
            } catch (e) {
              return secretError(e, kind === "passphrase" ? "passphrase" : "PIN");
            }
          }}
        />
      ) : (
        <div className="flex flex-col gap-4">
          <p className="rounded border border-border-primary bg-layer-01 px-4 py-3 text-sm text-secondary">
            Anyone with {what === "phrase" ? "these words" : "this key"} can take everything in this account. Write {it} down and keep {it} offline. Never type {it} into a website or share {it} in chat.
          </p>
          {what === "phrase" ? (
            <ol className="grid grid-cols-3 gap-2">
              {words.map((w, i) => (
                <li key={i} className="flex items-baseline gap-3 rounded border border-border-primary bg-layer-01 px-4 py-3">
                  <span className="w-5 text-right text-xs tabular-nums text-secondary">{i + 1}</span>
                  <span className="font-mono text-base font-medium">{w}</span>
                </li>
              ))}
            </ol>
          ) : (
            <p className="break-all rounded border border-border-primary bg-layer-01 px-4 py-4 font-mono text-base">{secret}</p>
          )}
        </div>
      )}
    </Sheet>
  );
}

export function ChangePinSheet({ onClose }: { onClose: () => void }) {
  const kind = useWallet((s) => s.vault?.kind ?? "pin");
  const noun = kind === "passphrase" ? "passphrase" : "PIN";
  const [step, setStep] = useState<"current" | "new" | "confirm" | "done">("current");
  const [cur, setCur] = useState("");
  const [next, setNext] = useState("");
  const [error, setError] = useState<string | null>(null);

  const finish = async (confirm: string): Promise<string | null> => {
    if (confirm !== next) {
      setStep("new");
      return `Those did not match. Choose the new ${noun} again.`;
    }
    try {
      await keystore.changeSecret(cur, next, kind);
      setCur("");
      setNext("");
      setStep("done");
      return null;
    } catch (e) {
      setStep("current");
      return secretError(e, noun);
    }
  };
  const validNew = (v: string) => (kind === "passphrase" ? (v.length >= 8 ? null : "Use at least 8 characters") : /^\d{6,}$/.test(v) ? null : "Use at least 6 digits");

  return (
    <Sheet title={`Change ${noun}`} onClose={onClose} width="36rem" ready={step} footer={step === "done" ? <Btn variant="primary" def className="flex-1" onClick={onClose}>Done</Btn> : undefined}>
      {error && step !== "done" && <p className="mb-4 text-center text-sm text-negative">{error}</p>}
      {step === "current" && (
        <SecretEntry
          key="c"
          kind={kind}
          prompt={`Enter your current ${noun}`}
          okLabel="Next"
          onSubmit={async (s) => {
            setCur(s);
            setError(null);
            setStep("new");
            return null;
          }}
        />
      )}
      {step === "new" && (
        <SecretEntry
          key="n"
          kind={kind}
          prompt={kind === "passphrase" ? "Choose a new passphrase" : "Choose a new PIN (6 digits or more)"}
          okLabel="Next"
          onSubmit={async (s) => {
            const bad = validNew(s);
            if (bad) return bad;
            setNext(s);
            setError(null);
            setStep("confirm");
            return null;
          }}
        />
      )}
      {step === "confirm" && (
        <SecretEntry
          key="f"
          kind={kind}
          prompt={`Enter the new ${noun} again`}
          okLabel="Change"
          onSubmit={async (s) => {
            setError(await finish(s));
            return null;
          }}
        />
      )}
      {step === "done" && <p className="py-6 text-center text-base">Your {noun} is changed.</p>}
    </Sheet>
  );
}

/** Confirm sheet for removing one account (unlocks first when the account's key is in the vault). */
export function RemoveAccountSheet({ account, onClose, onDone }: { account: RareAccount; onClose: () => void; onDone?: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const watch = account.kind === "watch";
  const device = account.kind === "ledger" || account.kind === "keystone";
  const remove = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      if (!(await ensureUnlockedFor(account.address, "Enter your PIN to remove this account"))) {
        setBusy(false);
        return;
      }
      await keystore.remove(account.address);
      await useWallet.getState().refresh();
      useApp.getState().toast("Account removed");
      onClose();
      onDone?.();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Could not remove it");
      setBusy(false);
    }
  };
  return (
    <Sheet
      title="Remove this account?"
      subtitle={`${account.label}, ${kindLabel(account).toLowerCase()}`}
      onClose={onClose}
      width="38rem"
      footer={
        <>
          <Btn def className="flex-1" onClick={onClose}>
            Cancel
          </Btn>
          <Btn className="flex-1 !text-negative" onClick={() => void remove()}>
            {busy ? "Removing" : "Remove"}
          </Btn>
        </>
      }
    >
      <p className="text-base leading-relaxed text-secondary">
        {watch
          ? "Rare Wallet stops watching this address. Nothing moves and nothing is lost."
          : device
            ? "The account is removed from this device. Your hardware wallet still holds the key, so you can add it again later."
            : "The account and its key are removed from this device. Make sure you saved the recovery phrase or key first. Without it the funds in this account cannot be recovered."}
      </p>
      {error && <p className="mt-3 text-sm text-negative">{error}</p>}
    </Sheet>
  );
}

/** Wipes every account and the vault. Needs REMOVE typed in. */
export function RemoveAllSheet({ onClose }: { onClose: () => void }) {
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ok = typed.trim().toUpperCase() === "REMOVE";
  const wipe = async () => {
    setBusy(true);
    try {
      await keystore.wipe();
      await useWallet.getState().refresh();
      useWalletNav.getState().reset();
      useApp.getState().toast("All wallets removed from this device");
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Could not remove the wallets");
      setBusy(false);
    }
  };
  return (
    <Sheet
      title="Remove all wallets?"
      onClose={onClose}
      width="40rem"
      footer={
        <>
          <Btn def className="flex-1" onClick={onClose}>
            Cancel
          </Btn>
          <Btn className={ok ? "flex-1 !text-negative" : "flex-1 opacity-40"} onClick={() => ok && !busy && void wipe()}>
            {busy ? "Removing" : "Remove all"}
          </Btn>
        </>
      }
    >
      <p className="text-base leading-relaxed text-secondary">
        Every account, recovery phrase and key is erased from this device, and the PIN is cleared. Funds stay on the network, but without your recovery phrases or hardware wallets you cannot reach them again.
      </p>
      <Field className="mt-5" label="Type REMOVE to confirm" value={typed} onChange={setTyped} placeholder="REMOVE" error={error} />
    </Sheet>
  );
}

/** Ledger permission help: what to do when Linux blocks the device, with a live check. */
export function HardwareHelpSheet({ onClose }: { onClose: () => void }) {
  const [res, setRes] = useState<(LedgerCheck & { found?: string }) | "busy" | null>(null);
  const [done, copy] = useCopy();
  const run = async () => {
    setRes("busy");
    setRes(await checkLedger());
  };
  const fix = res && res !== "busy" ? res.fix : undefined;
  return (
    <Sheet
      title="Hardware wallets"
      onClose={onClose}
      width="46rem"
      footer={
        <>
          <Btn variant="primary" def className="flex-1" onClick={() => void run()}>
            {res === "busy" ? "Looking" : "Check my Ledger"}
          </Btn>
          <Btn onClick={onClose}>Close</Btn>
        </>
      }
    >
      <div className="flex flex-col gap-4 text-base leading-relaxed text-secondary">
        <p>Ledger and Keystone connect over USB. Plug the device in, unlock it, and for Ledger open the Sui app. Ledger also works over Bluetooth.</p>
        <p>If Rare OS says it needs permission to use the device, run one command in a terminal, then unplug and plug the device back in. The check below shows the exact command for this system.</p>
        {res && res !== "busy" && (
          <div className="rounded border border-border-primary bg-layer-01 p-4 text-sm">
            {res.state === "ok" ? <p className="text-primary">Found {res.found}. Nothing is in the way.</p> : <p className="text-primary">{res.message}</p>}
            {fix && (
              <>
                <p className="mt-3 break-all font-mono text-xs text-secondary">{fix}</p>
                <Btn size="sm" className="mt-3" onClick={() => copy(fix)}>
                  {done ? "Copied" : "Copy command"}
                </Btn>
              </>
            )}
          </div>
        )}
      </div>
    </Sheet>
  );
}
