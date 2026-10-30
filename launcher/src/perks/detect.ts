// Rareling ownership over Sui gRPC (zero GraphQL): held in the wallet, or inside a kiosk the owner
// controls (a KioskOwnerCap, or one wrapped in a PersonalKioskCap). Rarelings are kiosk-locked by
// their TransferPolicy, so the kiosk read is the usual case.

import { canonType, grpc, type Network } from "../sui/chain";

/** Mysten's kiosk extensions package that defines PersonalKioskCap (rare.market's lib/chain/ids.ts). */
export const PERSONAL_KIOSK_CAP = "0x0cb4bcc0560340eb1a1b929cabe56b33fc6449820ec8c1980d69bb98b649b802::personal_kiosk::PersonalKioskCap";
export const KIOSK_OWNER_CAP = "0x2::kiosk::KioskOwnerCap";

export type RarelingResult = { status: "unavailable" } | { status: "none" } | { status: "owned"; count: number; where: "wallet" | "kiosk"; ids: string[] };

/** The reads the check needs; fakes in tests. */
export interface RarelingReader {
  owned: (owner: string, type: string) => Promise<Array<{ objectId: string; json: Record<string, unknown> | null }>>;
  /** a kiosk's items: object id and its type */
  kioskItems: (kioskId: string) => Promise<Array<{ objectId: string; type: string }>>;
}

/** The kiosk a cap opens: KioskOwnerCap.for, or PersonalKioskCap.cap (an Option) .for. */
export function kioskOf(json: Record<string, unknown> | null): string | null {
  if (!json) return null;
  if (typeof json.for === "string") return json.for;
  const cap = json.cap as { for?: unknown; vec?: Array<{ for?: unknown }> } | null | undefined;
  const f = cap?.for ?? cap?.vec?.[0]?.for;
  return typeof f === "string" ? f : null;
}

export async function checkRareling(owner: string, type: string | null, read: RarelingReader): Promise<RarelingResult> {
  if (!type) return { status: "unavailable" };
  const want = canonType(type);
  // 1. In the wallet.
  const held = await read.owned(owner, want);
  if (held.length) return { status: "owned", count: held.length, where: "wallet", ids: held.map((o) => o.objectId) };
  // 2. In the owner's kiosks.
  const caps = (await Promise.all([read.owned(owner, KIOSK_OWNER_CAP), read.owned(owner, PERSONAL_KIOSK_CAP).catch(() => [])])).flat();
  const kiosks = [...new Set(caps.map((c) => kioskOf(c.json)).filter((k): k is string => !!k))];
  const ids: string[] = [];
  // Every kiosk (a few at most for a person), so the badge can say how many.
  const lists = await Promise.all(kiosks.slice(0, 20).map((k) => read.kioskItems(k).catch(() => [])));
  for (const items of lists) for (const it of items) if (canonType(it.type) === want) ids.push(it.objectId);
  return ids.length ? { status: "owned", count: ids.length, where: "kiosk", ids } : { status: "none" };
}

export function grpcRarelingReader(network: Network): RarelingReader {
  const c = grpc(network);
  return {
    owned: async (owner, type) => {
      const r = await c.core.listOwnedObjects({ owner, type, limit: 50, include: { json: true } });
      return r.objects.map((o) => ({ objectId: o.objectId, json: (o.json as Record<string, unknown> | null) ?? null }));
    },
    kioskItems: async (kioskId) => {
      const r = await c.core.listDynamicFields({ parentId: kioskId, limit: 200 });
      // Items are dynamic object fields keyed by kiosk::Item { id }; valueType is the item's type.
      return r.dynamicFields.filter((f) => f.type.includes("::kiosk::Item")).map((f) => ({ objectId: f.childId ?? f.fieldId, type: f.valueType }));
    },
  };
}
