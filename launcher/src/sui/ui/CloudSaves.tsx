// Cloud saves: emulator and web game saves backed up to Walrus, encrypted with a key from the
// player's wallet. Every upload and every restore starts from a button press on that save set;
// nothing runs in the background and nothing is paid without Rare Wallet's review.

import { useEffect, useRef, useState } from "react";
import { Cloud, Lock } from "lucide-react";
import { useApp } from "../../lib/store";
import { focusFirst } from "../../nav/focus";
import { useHints } from "../../nav/hints";
import { usePadHandler } from "../../nav/input";
import { Button, Dot, Empty, Glyph, HeadRow, LoadBar, Panel, SectionHead, Segmented } from "../../ui";
import type { SaveSet } from "../native";
import { useProfile } from "../profile";
import { daysLeft, KEEP_OPTIONS, latestBySet, useCloud, type Backup, type Busy } from "../saves/cloud";
import type { CostEstimate } from "../saves/walrus";

const KEEP_LABEL: Record<number, string> = { 30: "30 days", 90: "90 days", 180: "6 months", 365: "1 year" };

export function fmtWal(frost: bigint | null | undefined): string {
  if (frost == null) return "";
  const n = Number(frost) / 1e9;
  if (n === 0) return "0 WAL";
  if (n < 0.0001) return "<0.0001 WAL";
  return `${Number(n.toPrecision(3)).toLocaleString("en-US", { maximumFractionDigits: 4 })} WAL`;
}

export const fmtSuiMist = (mist: bigint) => `${Number((Number(mist) / 1e9).toPrecision(2)).toLocaleString("en-US", { maximumFractionDigits: 4 })} SUI`;

export function fmtBytes(b: number): string {
  if (b >= 1e9) return `${(b / 1e9).toFixed(1)} GB`;
  if (b >= 1e6) return `${(b / 1e6).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(b / 1e3))} KB`;
}

export function ago(t: number | null | undefined): string {
  if (!t) return "";
  const s = (Date.now() - t) / 1000;
  if (s < 3600) return "just now";
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  if (s < 86400 * 2) return "yesterday";
  if (s < 86400 * 30) return `${Math.round(s / 86400)} days ago`;
  return new Date(t).toLocaleDateString([], { month: "short", day: "numeric" });
}

const STEP: Record<NonNullable<Busy>["step"], string> = {
  pack: "Packing",
  encode: "Encrypting",
  register: "Approve in Rare Wallet",
  upload: "Uploading",
  certify: "Approve to confirm",
  done: "Done",
  download: "Downloading",
  unpack: "Restoring",
};

const COLS = "grid-cols-[minmax(0,1fr)_6rem_7.5rem_9rem_7.5rem_15rem]";

export function CloudSaves() {
  const s = useCloud();
  const address = useProfile((p) => p.address);
  const root = useRef<HTMLDivElement>(null);
  const [confirm, setConfirm] = useState<string | null>(null);

  useEffect(() => {
    void useProfile.getState().refresh();
    void useCloud.getState().detect();
  }, []);
  useEffect(() => useCloud.getState().setAddress(address), [address]);
  useEffect(() => {
    setTimeout(() => root.current && focusFirst(root.current), 30);
  }, [s.sets === null]); // eslint-disable-line react-hooks/exhaustive-deps

  useHints([{ b: "x", label: `Keep for ${KEEP_LABEL[s.days]}` }]);
  usePadHandler((b) => {
    if (b === "x") {
      const i = KEEP_OPTIONS.indexOf(s.days as (typeof KEEP_OPTIONS)[number]);
      useCloud.getState().setDays(KEEP_OPTIONS[(i + 1) % KEEP_OPTIONS.length]);
      return true;
    }
    return false;
  });

  const latest = latestBySet(s.backups ?? []);
  const here = new Set((s.sets ?? []).map((x) => x.id));
  const elsewhere = [...latest.values()].filter((b) => !here.has(b.meta.id));
  const totalWal = Object.values(s.estimates).reduce((a, e) => a + (e?.walFrost ?? 0n), 0n);
  const totalSui = Object.values(s.estimates).reduce((a, e) => a + (e ? e.tipMist + e.gasMist : 0n), 0n);
  const toast = useApp.getState().toast;

  const backUp = (set: SaveSet) =>
    void s.backUp(set).then(
      () => toast(`${set.emulator} ${set.label.toLowerCase()} backed up`),
      () => {},
    );
  const restore = (b: Backup) => {
    if (confirm !== b.blobObjectId) return setConfirm(b.blobObjectId);
    setConfirm(null);
    void s.restore(b).then(
      (moved) => toast(moved ? `Restored. Your previous saves are kept in ${moved.split("/").pop()}` : `${b.meta.emulator} saves restored`),
      () => {},
    );
  };

  return (
    <div ref={root} data-nav-scope className="rise fixed inset-0 z-30 bg-bg">
      <LoadBar on={!!s.busy || s.sets === null} />
      <div data-scroll className="h-full overflow-y-auto px-[2.5rem] pb-[3rem] pt-[2rem]">
        <div className="flex items-end gap-[1rem]">
          <div className="min-w-0 flex-1">
            <h1 className="text-title font-semibold tracking-[-0.02em]">Cloud saves</h1>
            <p className="mt-[0.25rem] max-w-[46rem] text-small text-muted">
              Emulator and web game saves, encrypted on this device and kept on Walrus. Your wallet pays for the storage and only your account can open them. Steam games use Steam Cloud.
            </p>
          </div>
          <Glyph b="x" size="sm" />
          <Segmented items={KEEP_OPTIONS.map((d) => ({ value: String(d), label: KEEP_LABEL[d] }))} value={String(s.days)} />
        </div>

        <Panel className="mt-[1.5rem] flex items-center gap-[1rem] px-[1.25rem] py-[1rem]">
          <span className="flex h-[2.5rem] w-[2.5rem] shrink-0 items-center justify-center rounded-full bg-l3 text-icon-2">{s.key ? <Cloud size="1.2rem" /> : <Lock size="1.15rem" />}</span>
          <div className="min-w-0 flex-1">
            <div className="text-body font-semibold">{!address ? "No account" : s.key ? "Unlocked on this device" : "Locked"}</div>
            <div className="text-small text-muted">
              {!address
                ? "Add a Rare Wallet account to back up saves."
                : s.key
                  ? `${(s.backups ?? []).length} ${(s.backups ?? []).length === 1 ? "backup" : "backups"} on Walrus${s.network === "testnet" ? " testnet" : ""}. Backing up everything here for ${KEEP_LABEL[s.days]} costs about ${fmtWal(totalWal)} plus ${fmtSuiMist(totalSui)} in fees.`
                  : "Approve one signature in Rare Wallet to make your save key. It costs nothing and the key never leaves this device."}
            </div>
          </div>
          {address && !s.key && (
            <Button variant="primary" def hint="Unlock" onClick={() => void s.unlock()}>
              Unlock
            </Button>
          )}
        </Panel>
        {s.error && (
          <div className="mt-[0.9rem] flex items-center gap-[0.5rem] text-small text-fg">
            <Dot tone="negative" />
            {s.error}
          </div>
        )}

        <SectionHead className="mb-[0.5rem] mt-[2rem]" title="On this device" note={s.sets ? `${s.sets.length} found` : undefined} />
        {s.sets && s.sets.length === 0 && <Empty title="No emulator saves found" body="Rare looks in RetroArch, RPCS3, PCSX2, Cemu, DuckStation, Dolphin, PPSSPP and other emulators, EmuDeck's Emulation folder and your web games." />}
        {s.sets && s.sets.length > 0 && (
          <>
            <HeadRow cols={COLS}>
              <span>Saves</span>
              <span className="text-right">Size</span>
              <span className="text-right">Changed</span>
              <span className="text-right">Last backup</span>
              <span className="text-right">{KEEP_LABEL[s.days]}</span>
              <span />
            </HeadRow>
            <div data-nav-group="saves-here">
              {s.sets.map((set) => (
                <SetRow key={set.id} set={set} last={latest.get(set.id) ?? null} est={s.estimates[set.id]} unlocked={!!s.key} busy={s.busy} confirm={confirm} onBackUp={() => backUp(set)} onRestore={restore} epoch={s.epoch} network={s.network} />
              ))}
            </div>
          </>
        )}

        {elsewhere.length > 0 && (
          <>
            <SectionHead className="mb-[0.5rem] mt-[2rem]" title="From your other devices" note="not on this device yet" />
            <div data-nav-group="saves-elsewhere">
              {elsewhere.map((b) => (
                <div key={b.blobObjectId} className={`grid h-[3.25rem] items-center gap-x-[0.75rem] px-[0.75rem] text-small ${COLS}`}>
                  <span className="min-w-0 truncate">
                    <span className="font-medium text-fg">{b.meta.emulator}</span> <span className="text-muted">{b.meta.label}</span>
                  </span>
                  <span className="text-right tabular-nums text-muted">{fmtBytes(b.meta.bytes)}</span>
                  <span />
                  <span className="text-right text-muted">{ago(b.meta.created)}</span>
                  <span />
                  <span className="flex justify-end">
                    <Button size="sm" hint={confirm === b.blobObjectId ? "Restore" : "Restore here"} disabled={!!s.busy} onClick={() => restore(b)}>
                      {s.busy?.setId === b.meta.id ? STEP[s.busy.step] : confirm === b.blobObjectId ? "Press again" : "Restore here"}
                    </Button>
                  </span>
                </div>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function SetRow({
  set,
  last,
  est,
  unlocked,
  busy,
  confirm,
  onBackUp,
  onRestore,
  epoch,
  network,
}: {
  set: SaveSet;
  last: Backup | null;
  est: CostEstimate | undefined;
  unlocked: boolean;
  busy: Busy;
  confirm: string | null;
  onBackUp: () => void;
  onRestore: (b: Backup) => void;
  epoch: number | null;
  network: "mainnet" | "testnet";
}) {
  const mine = busy?.setId === set.id;
  const left = last ? daysLeft(last, epoch, network) : null;
  return (
    <div data-nav-group={`save-${set.id}`} data-nav-remember className={`grid h-[3.25rem] items-center gap-x-[0.75rem] rounded-card px-[0.75rem] text-small ${COLS}`}>
      <span className="min-w-0 truncate">
        <span className="font-medium text-fg">{set.emulator}</span> <span className="text-muted">{set.label}</span>
        <span className="ml-[0.5rem] text-label text-muted">{set.files.toLocaleString("en-US")} files</span>
      </span>
      <span className="text-right tabular-nums text-fg">{fmtBytes(set.bytes)}</span>
      <span className="text-right text-muted">{ago(set.modified)}</span>
      <span className="text-right text-muted">
        {!unlocked ? "Unlock to see" : last ? `${ago(last.meta.created)}${left != null ? ` · ${left} d left` : ""}` : "Never"}
      </span>
      <span className="text-right tabular-nums text-fg">{fmtWal(est?.walFrost)}</span>
      <span className="flex justify-end gap-[0.5rem]">
        {mine ? (
          <span className="flex h-[2.25rem] items-center gap-[0.5rem] text-small text-fg">
            <Dot tone="brand" live />
            {STEP[busy!.step]}
          </span>
        ) : (
          <>
            {last && (
              <Button size="sm" hint={confirm === last.blobObjectId ? "Restore" : "Restore this backup"} disabled={!!busy} onClick={() => onRestore(last)}>
                {confirm === last.blobObjectId ? "Press again" : "Restore"}
              </Button>
            )}
            <Button size="sm" hint="Back up now" disabled={!unlocked || !!busy} onClick={onBackUp}>
              Back up
            </Button>
          </>
        )}
      </span>
    </div>
  );
}
