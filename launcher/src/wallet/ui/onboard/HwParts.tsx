// Pieces the Ledger and Keystone pages share: an error box (with the one-line USB permission fix),
// and the list of device accounts to pick from.

import { Check } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { balances } from "../../data/balances";
import { Addr, Btn, Field, Panel, cn, useCopy } from "../kit";
import { Sui } from "../../../marketplace/parts";

export function HwError({ message, fix, onRetry }: { message: string; fix?: string | null; onRetry?: () => void }) {
  const [copied, copy] = useCopy();
  return (
    <Panel className="flex flex-col gap-3 px-5 py-4">
      <p className="text-sm text-negative">{fix ? "Rare OS needs permission to use this device." : message}</p>
      {fix && (
        <>
          <p className="text-sm text-secondary">Run this once in Konsole, then plug your device in again.</p>
          <code className="select-text break-all rounded border border-border-primary bg-layer-02 px-3 py-2 font-mono text-xs leading-relaxed">{fix}</code>
          <div>
            <Btn size="md" onClick={() => copy(fix)}>
              {copied ? "Copied" : "Copy command"}
            </Btn>
          </div>
        </>
      )}
      {onRetry && (
        <div>
          <Btn size="md" def onClick={onRetry}>
            Try again
          </Btn>
        </div>
      )}
    </Panel>
  );
}

export interface HwRow {
  index: number;
  address: string;
}

type Verify = "busy" | "ok" | "bad";

/** SUI balance for an address, undefined while loading or when it cannot be read. */
function useSuiBalance(address: string) {
  const [raw, setRaw] = useState<bigint | undefined>();
  useEffect(() => {
    let live = true;
    void balances(address)
      .then((l) => live && setRaw(l.find((b) => b.coinType.endsWith("::sui::SUI"))?.raw ?? 0n))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [address]);
  return raw;
}

function Row({
  r,
  added,
  on,
  name,
  placeholder,
  onToggle,
  onName,
  verify,
  onVerify,
  first,
}: {
  r: HwRow;
  added: boolean;
  on: boolean;
  name: string;
  placeholder: string;
  onToggle: () => void;
  onName: (v: string) => void;
  verify?: Verify;
  onVerify?: () => void;
  first: boolean;
}) {
  const raw = useSuiBalance(r.address);
  return (
    <div className="px-1 py-1">
      <div className="flex items-center gap-2">
        <button
          data-nav
          data-nav-default={first ? "" : undefined}
          role="checkbox"
          aria-checked={on}
          disabled={added}
          onClick={onToggle}
          className={cn("flex min-w-0 flex-1 items-center gap-4 rounded px-4 py-3 text-left", added && "opacity-60")}
        >
          <span className={cn("flex size-6 shrink-0 items-center justify-center rounded border", on ? "border-transparent bg-primary text-background" : "border-border-light")}>
            {on && <Check size="1rem" strokeWidth={3} />}
          </span>
          <span className="w-24 shrink-0 whitespace-nowrap text-sm text-secondary">Account {r.index + 1}</span>
          <Addr address={r.address} className="min-w-0 flex-1 truncate text-base" />
          <span className="shrink-0 text-sm tabular-nums text-secondary">{added ? "Added" : raw === undefined ? "" : <Sui mist={raw} dp={3} />}</span>
        </button>
        {onVerify && !added && (
          <Btn size="md" onClick={onVerify} className="w-40">
            {verify === "busy" ? "Check device" : verify === "ok" ? "Verified" : verify === "bad" ? "Does not match" : "Verify on device"}
          </Btn>
        )}
      </div>
      {on && (
        <div className="px-4 pb-2 pt-1 pl-[3.5rem]">
          <Field value={name} onChange={onName} placeholder={placeholder} prompt={`Name for account ${r.index + 1}`} />
        </div>
      )}
    </div>
  );
}

/** Pick one or several device accounts, name them, add. */
export function HwAccounts({
  rows,
  existing,
  defaultName,
  onVerify,
  onMore,
  more,
  onAdd,
  adding,
  error,
  verified,
}: {
  rows: HwRow[];
  existing: string[];
  /** "Ledger 3": name for the nth (0-based) newly picked account */
  defaultName: (n: number) => string;
  onVerify?: (r: HwRow) => void;
  onMore: () => void;
  more: boolean;
  onAdd: (picked: Array<{ row: HwRow; label: string }>) => void;
  adding: boolean;
  error?: string;
  verified: Record<string, Verify>;
}) {
  const owned = new Set(existing.map((a) => a.toLowerCase()));
  const [picked, setPicked] = useState<string[]>([]);
  const [names, setNames] = useState<Record<string, string>>({});
  const firstFree = useRef(rows.find((r) => !owned.has(r.address.toLowerCase()))?.address);
  const nth = (a: string) => picked.indexOf(a);
  const fresh = picked.filter((a) => !owned.has(a.toLowerCase()));
  return (
    <div className="flex flex-col gap-4">
      <Panel className="divide-y divide-border-primary p-1">
        {rows.map((r) => {
          const added = owned.has(r.address.toLowerCase());
          return (
            <Row
              key={r.address}
              r={r}
              added={added}
              on={!added && picked.includes(r.address)}
              name={names[r.address] ?? ""}
              placeholder={defaultName(Math.max(0, nth(r.address)))}
              onToggle={() => setPicked((p) => (p.includes(r.address) ? p.filter((x) => x !== r.address) : [...p, r.address]))}
              onName={(v) => setNames((n) => ({ ...n, [r.address]: v }))}
              verify={verified[r.address]}
              onVerify={onVerify ? () => onVerify(r) : undefined}
              first={r.address === firstFree.current}
            />
          );
        })}
      </Panel>
      <Btn variant="ghost" onClick={onMore} className="w-full">
        {more ? "Reading" : "Show more"}
      </Btn>
      {error && <p className="text-sm text-negative">{error}</p>}
      <Btn
        variant="primary"
        className={cn("w-full", (fresh.length === 0 || adding) && "pointer-events-none opacity-40")}
        onClick={() =>
          onAdd(
            fresh.map((a, n) => ({
              row: rows.find((r) => r.address === a)!,
              label: names[a]?.trim() || defaultName(n),
            })),
          )
        }
      >
        {adding ? "Adding" : fresh.length <= 1 ? "Add account" : `Add ${fresh.length} accounts`}
      </Btn>
    </div>
  );
}
