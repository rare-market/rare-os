// What the review sheet's footer shows while a device or phone is doing the signing: the live
// state of the Ledger, the Keystone (with the hash to compare), or the passkey phone ceremony.

import { askText } from "../../../components/Osk";
import { passkeyNative } from "../../native";
import { Btn, Qr, cn } from "../kit";
import { useKeystonePrompt, useLedgerPrompt, usePasskeyState } from "./prompts";

/** 64 hex chars as eight groups of 8, the way the Keystone shows its hash. */
export function chunkHash(hex: string, size = 8): string[] {
  const out: string[] = [];
  for (let i = 0; i < hex.length; i += size) out.push(hex.slice(i, i + size));
  return out;
}

function Lead({ children, sub }: { children: string; sub?: string }) {
  return (
    <div className="min-w-0">
      <p className="flex items-center gap-2.5 text-base font-semibold">
        <span className="size-2 shrink-0 animate-pulse rounded-full bg-primary" aria-hidden />
        {children}
      </p>
      {sub && <p className="mt-1 text-sm text-secondary">{sub}</p>}
    </div>
  );
}

export function LedgerPanel({ onRetry, onCancel }: { onRetry: () => void; onCancel: () => void }) {
  const p = useLedgerPrompt();
  const err = p.error;
  return (
    <div className="flex w-full flex-col gap-4">
      {err ? (
        <div className="min-w-0">
          <p className="text-base font-semibold text-negative">{err.message}</p>
          {err.fix && (
            <>
              <p className="mt-2 text-sm text-secondary">Run this once, then plug the Ledger in again:</p>
              <pre className="mt-1.5 overflow-x-auto rounded border border-border-primary bg-layer-01 px-3 py-2 font-mono text-xs">{err.fix}</pre>
            </>
          )}
        </div>
      ) : (
        <Lead sub={p.device ? p.device : undefined}>{p.message || "Connecting to your Ledger"}</Lead>
      )}
      <div className="flex gap-2">
        {err && (
          <Btn variant="primary" def className="flex-1" onClick={onRetry}>
            Retry
          </Btn>
        )}
        <Btn def={!err} className={err ? undefined : "flex-1"} onClick={onCancel}>
          Cancel
        </Btn>
      </div>
    </div>
  );
}

export function KeystonePanel({ onRetry, onCancel }: { onRetry: () => void; onCancel: () => void }) {
  const k = useKeystonePrompt();
  const err = k.error;
  const groups = k.digestHex ? chunkHash(k.digestHex) : [];
  return (
    <div className="flex w-full flex-col gap-4">
      {err ? (
        <div className="min-w-0">
          <p className="text-base font-semibold text-negative">{err.message}</p>
          {err.fix && (
            <>
              <p className="mt-2 text-sm text-secondary">Run this once, then plug the Keystone in again:</p>
              <pre className="mt-1.5 overflow-x-auto rounded border border-border-primary bg-layer-01 px-3 py-2 font-mono text-xs">{err.fix}</pre>
            </>
          )}
        </div>
      ) : (
        <Lead>{k.message || "Connecting to your Keystone"}</Lead>
      )}
      {!err && groups.length > 0 && (
        <div>
          <div className="grid grid-cols-4 gap-x-4 gap-y-1.5 rounded border border-border-primary bg-layer-01 px-4 py-3 font-mono text-sm tabular-nums" aria-label="Transaction hash">
            {groups.map((g, i) => (
              <span key={i}>{g}</span>
            ))}
          </div>
          <p className="mt-2 text-sm text-secondary">Check it matches the hash on your Keystone.</p>
        </div>
      )}
      <div className="flex gap-2">
        {err && (
          <Btn variant="primary" def className="flex-1" onClick={onRetry}>
            Retry
          </Btn>
        )}
        <Btn def={!err} className={err ? undefined : "flex-1"} onClick={onCancel}>
          Cancel
        </Btn>
      </div>
    </div>
  );
}

export function PasskeyPanel({ onRetry, onCancel }: { onRetry: () => void; onCancel: () => void }) {
  const c = usePasskeyState();
  const askPin = async () => {
    const pin = await askText("Security key PIN", "", { secret: true });
    await passkeyNative.pin(pin);
  };
  return (
    <div className="flex w-full flex-col gap-4">
      {c.kind === "qr" ? (
        <div className="flex items-center gap-5">
          <Qr text={c.url} className="size-[9rem] shrink-0" />
          <div className="min-w-0">
            <p className="text-base font-semibold">Scan with your phone</p>
            <p className="mt-1 text-sm text-secondary">Then approve with your fingerprint or face. Keep Bluetooth on.</p>
          </div>
        </div>
      ) : c.kind === "status" ? (
        <Lead>{c.message}</Lead>
      ) : c.kind === "pin" ? (
        <Lead sub={c.attemptsLeft != null ? `${c.attemptsLeft} tries left` : undefined}>Your security key needs its PIN</Lead>
      ) : c.kind === "error" ? (
        <p className="text-base font-semibold text-negative">{c.message}</p>
      ) : (
        <Lead>Waiting for your phone</Lead>
      )}
      <div className="flex gap-2">
        {c.kind === "pin" && (
          <Btn variant="primary" def className="flex-1" onClick={() => void askPin()}>
            Enter PIN
          </Btn>
        )}
        {c.kind === "error" && (
          <Btn variant="primary" def className="flex-1" onClick={onRetry}>
            Retry
          </Btn>
        )}
        <Btn def={c.kind !== "pin" && c.kind !== "error"} className={cn(c.kind === "pin" || c.kind === "error" ? undefined : "flex-1")} onClick={onCancel}>
          Cancel
        </Btn>
      </div>
    </div>
  );
}
