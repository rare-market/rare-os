// Rare cloud saves: the Steam Cloud of everything that isn't on Steam. Emulator and web game saves
// are packed on the device, encrypted with a key only the player's wallet can produce, and kept on
// Walrus, paid from the player's own wallet. Restore works on any device signed in to the same
// account.
//
// Rules: nothing uploads or restores without the player pressing the button for that save set, and
// every payment goes through Rare Wallet's review sheet.

import { create } from "zustand";
import { toBase64, fromBase64 } from "@mysten/sui/utils";
import type { Network } from "../chain";
import { savesNative, type SaveSet } from "../native";
import { walletSigner, type CloudSigner } from "../signer";
import { deriveSaveKey, open, SAVES_CONTEXT, seal, SEAL_OVERHEAD, type SaveKey } from "./crypto";
import { deriveSecret } from "../../wallet/secret";
import { APP_TAG, currentEpoch, DEFAULT_NETWORK, epochsFor, estimate, EPOCH_DAYS, listTagged, readBlob, storeBlob, type CostEstimate, type UploadProgress } from "./walrus";

export interface BackupMeta {
  v: 1;
  id: string;
  rel: string;
  emulator: string;
  label: string;
  kind: SaveSet["kind"];
  files: number;
  bytes: number;
  /** unix ms */
  created: number;
}

export interface Backup {
  blobObjectId: string;
  blobId: string;
  size: number;
  endEpoch: number | null;
  meta: BackupMeta;
}

export const KEEP_OPTIONS = [30, 90, 180, 365] as const;

const enc = new TextEncoder();
const dec = new TextDecoder();

/** Encrypts the description stored next to a backup (public attributes never show what it is). */
export async function sealMeta(key: SaveKey, meta: BackupMeta): Promise<string> {
  return toBase64(await seal(key, enc.encode(JSON.stringify(meta))));
}

export async function openMeta(key: SaveKey, b64: string): Promise<BackupMeta | null> {
  try {
    const m = JSON.parse(dec.decode(await open(key, fromBase64(b64)))) as BackupMeta;
    return m && m.v === 1 && typeof m.id === "string" && typeof m.rel === "string" ? m : null;
  } catch {
    return null;
  }
}

/** The newest backup of each save set. */
export function latestBySet(list: Backup[]): Map<string, Backup> {
  const out = new Map<string, Backup>();
  for (const b of list) {
    const cur = out.get(b.meta.id);
    if (!cur || b.meta.created > cur.meta.created) out.set(b.meta.id, b);
  }
  return out;
}

/** Packs, encrypts and stores one save set. Called only from the player's Back up button. */
export async function backUpSet(o: { network: Network; signer: CloudSigner; key: SaveKey; set: SaveSet; days: number; onStep?: (s: UploadProgress) => void; pack?: (id: string) => Promise<Uint8Array> }): Promise<Backup> {
  const archive = await (o.pack ?? savesNative.pack)(o.set.id);
  const meta: BackupMeta = { v: 1, id: o.set.id, rel: o.set.rel, emulator: o.set.emulator, label: o.set.label, kind: o.set.kind, files: o.set.files, bytes: archive.length, created: Date.now() };
  const blob = await seal(o.key, archive);
  const epochs = epochsFor(o.days, o.network);
  const r = await storeBlob(o.network, o.signer, blob, {
    epochs,
    title: `${o.set.emulator} ${o.set.label.toLowerCase()}`,
    attributes: { app: APP_TAG, v: "1", key: o.key.id, meta: await sealMeta(o.key, meta) },
    onStep: o.onStep,
  });
  return { blobObjectId: r.blobObjectId, blobId: r.blobId, size: blob.length, endEpoch: null, meta };
}

/** The player's backups that this key opens. Read only. */
export async function listBackups(network: Network, owner: string, key: SaveKey): Promise<Backup[]> {
  const tagged = await listTagged(network, owner);
  const out: Backup[] = [];
  for (const t of tagged) {
    if (!t.certified || t.attributes.key !== key.id || !t.attributes.meta) continue;
    const meta = await openMeta(key, t.attributes.meta);
    if (meta) out.push({ blobObjectId: t.blobObjectId, blobId: t.blobId, size: t.size, endEpoch: t.endEpoch, meta });
  }
  return out.sort((a, b) => b.meta.created - a.meta.created);
}

/** Downloads and decrypts a backup. */
export async function fetchBackup(network: Network, key: SaveKey, b: Backup): Promise<Uint8Array> {
  return open(key, await readBlob(network, b.blobId));
}

// ---------- store for the Cloud saves screen ----------

export type Busy = { setId: string; what: "backup" | "restore"; step: UploadProgress["step"] | "pack" | "download" | "unpack" } | null;

interface CloudState {
  network: Network;
  address: string | null;
  sets: SaveSet[] | null;
  key: SaveKey | null;
  backups: Backup[] | null;
  epoch: number | null;
  days: number;
  estimates: Record<string, CostEstimate | undefined>;
  busy: Busy;
  error: string | null;
  setAddress: (a: string | null) => void;
  setDays: (d: number) => void;
  detect: () => Promise<void>;
  estimateAll: () => Promise<void>;
  unlock: () => Promise<void>;
  refresh: () => Promise<void>;
  backUp: (set: SaveSet) => Promise<void>;
  restore: (b: Backup) => Promise<string>;
}

const startNetwork: Network = DEFAULT_NETWORK;

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export const useCloud = create<CloudState>((set, get) => ({
  network: startNetwork,
  address: null,
  sets: null,
  key: null,
  backups: null,
  epoch: null,
  days: 90,
  estimates: {},
  busy: null,
  error: null,
  setAddress: (address) => {
    if (address === get().address) return;
    // A different account has a different key and different backups.
    set({ address, key: null, backups: null, error: null });
  },
  setDays: (days) => {
    set({ days, estimates: {} });
    void get().estimateAll();
  },
  detect: async () => {
    try {
      set({ sets: await savesNative.detect() });
      void get().estimateAll();
    } catch (e) {
      set({ sets: [], error: message(e) });
    }
  },
  estimateAll: async () => {
    const { sets, days, network } = get();
    if (!sets?.length) return;
    // gzip shrinks saves; estimate on the raw size so the number is never low.
    await Promise.all(
      sets.map((s) =>
        estimate(s.bytes + SEAL_OVERHEAD, days, network)
          .then((e) => set((st) => ({ estimates: { ...st.estimates, [s.id]: e } })))
          .catch(() => {}),
      ),
    );
  },
  unlock: async () => {
    const a = get().address;
    if (!a) return set({ error: "Add a Rare Wallet account first" });
    set({ error: null });
    try {
      const secret = await deriveSecret(SAVES_CONTEXT, { account: a, title: "Unlock cloud saves" });
      set({ key: await deriveSaveKey(secret, a) });
      secret.fill(0);
      await get().refresh();
    } catch (e) {
      set({ error: message(e) });
    }
  },
  refresh: async () => {
    const { address, key, network } = get();
    if (!address || !key) return;
    try {
      const [backups, epoch] = await Promise.all([listBackups(network, address, key), currentEpoch(network).catch(() => null)]);
      set({ backups, epoch });
    } catch (e) {
      set({ error: message(e) });
    }
  },
  backUp: async (s) => {
    const { address, key, network, days, busy } = get();
    if (busy || !address || !key) return;
    set({ busy: { setId: s.id, what: "backup", step: "pack" }, error: null });
    try {
      const signer = await walletSigner(address);
      const b = await backUpSet({ network, signer, key, set: s, days, onStep: (p) => set({ busy: { setId: s.id, what: "backup", step: p.step } }) });
      set((st) => ({ backups: [b, ...(st.backups ?? [])] }));
      void get().refresh();
    } catch (e) {
      set({ error: message(e) });
      throw e;
    } finally {
      set({ busy: null });
    }
  },
  restore: async (b) => {
    const { key, network, busy } = get();
    if (busy || !key) return "";
    set({ busy: { setId: b.meta.id, what: "restore", step: "download" }, error: null });
    try {
      const archive = await fetchBackup(network, key, b);
      set({ busy: { setId: b.meta.id, what: "restore", step: "unpack" } });
      const moved = await savesNative.restore(b.meta.id, b.meta.rel, archive);
      void get().detect();
      return moved;
    } catch (e) {
      set({ error: message(e) });
      throw e;
    } finally {
      set({ busy: null });
    }
  },
}));

/** Days of storage left on a backup, from its end epoch. */
export function daysLeft(b: Backup, epoch: number | null, network: Network): number | null {
  if (b.endEpoch === null || epoch === null) return null;
  return Math.max(0, (b.endEpoch - epoch) * EPOCH_DAYS[network]);
}
