// Rare Store licence check, launcher side: does an owner hold a licence for a game? The real
// contract (rare-store sources/licence.move, read the same way by src/store/chain.ts) has two
// types: `licence::Licence` (soulbound, key only, owned by the buyer) and `licence::ResaleLicence`
// (key + store, after `make_tradable`; usually in a kiosk, listed or not, and it still counts until
// sold). Both carry `game: ID` and `edition`. All reads are gRPC.
//
// `RARE_STORE.packageId` is the package the types were first published at (rare.market's
// /v1/store/config `origin`). Until it is set every check answers { status: "unavailable" }, so
// nothing in the launcher can mistake "not deployed" for "not owned".

import { grpc, canonAddr, type Network } from "./chain";

export const RARE_STORE: { packageId: string | null; network: Network } = { packageId: null, network: "mainnet" };

export interface LicenceFields {
  id: string;
  /** The store's `Game` object id. */
  game: string;
  slug: string;
  edition: number;
  purchasedAtMs: number;
  /** A tradable ResaleLicence (else the soulbound Licence). */
  resale: boolean;
}

export type LicenceResult = { status: "unavailable" } | { status: "none" } | { status: "owned"; licence: LicenceFields; where: "wallet" | "kiosk"; kioskId?: string };

/** Reads that the check needs; swapped for fakes in tests. */
export interface LicenceReader {
  owned: (owner: string, type: string) => Promise<Array<{ objectId: string; json: Record<string, unknown> | null }>>;
  kioskFields: (kioskId: string) => Promise<Array<{ objectId: string; type: string }>>;
  objects: (ids: string[]) => Promise<Array<{ objectId: string; type: string; json: Record<string, unknown> | null }>>;
}

const idText = (v: unknown): string | null => (typeof v === "string" ? v : v && typeof v === "object" && typeof (v as { id?: unknown }).id === "string" ? (v as { id: string }).id : null);

const toLicence = (id: string, j: Record<string, unknown> | null, resale: boolean): LicenceFields | null => {
  const game = idText(j?.game);
  return j && game ? { id, game: canonAddr(game), slug: typeof j.slug === "string" ? j.slug : "", edition: Number(j.edition ?? 0), purchasedAtMs: Number(j.purchased_at_ms ?? 0), resale } : null;
};

export const licenceTypes = (pkg: string) => {
  const p = canonAddr(pkg);
  return { licence: `${p}::licence::Licence`, resale: `${p}::licence::ResaleLicence` };
};

export async function checkLicence(owner: string, gameId: string, pkg = RARE_STORE.packageId, read: LicenceReader = grpcReader(RARE_STORE.network)): Promise<LicenceResult> {
  if (!pkg) return { status: "unavailable" };
  const want = canonAddr(gameId);
  const T = licenceTypes(pkg);
  // 1. Held in the wallet: the soulbound Licence, or a ResaleLicence taken out of a kiosk.
  for (const [type, resale] of [[T.licence, false], [T.resale, true]] as const) {
    for (const o of await read.owned(owner, type)) {
      const l = toLicence(o.objectId, o.json, resale);
      if (l?.game === want) return { status: "owned", licence: l, where: "wallet" };
    }
  }
  // 2. A ResaleLicence inside a kiosk the owner controls (KioskOwnerCap.for -> the kiosk's items).
  for (const cap of await read.owned(owner, "0x2::kiosk::KioskOwnerCap")) {
    const kioskId = idText(cap.json?.for);
    if (!kioskId) continue;
    const fields = (await read.kioskFields(kioskId)).filter((f) => f.type === T.resale);
    if (!fields.length) continue;
    for (const o of await read.objects(fields.map((f) => f.objectId))) {
      const l = o.type === T.resale ? toLicence(o.objectId, o.json, true) : null;
      if (l?.game === want) return { status: "owned", licence: l, where: "kiosk", kioskId };
    }
  }
  return { status: "none" };
}

/** `0xpkg::m::T` with the address in its long form, so types from the API and the chain compare. */
function canonType(t: string): string {
  const [a, ...rest] = t.split("::");
  return [canonAddr(a), ...rest].join("::");
}

export function grpcReader(network: Network): LicenceReader {
  const c = grpc(network);
  return {
    owned: async (owner, type) => {
      const r = await c.core.listOwnedObjects({ owner, type, limit: 50, include: { json: true } });
      return r.objects.map((o) => ({ objectId: o.objectId, json: (o.json as Record<string, unknown> | null) ?? null }));
    },
    kioskFields: async (kioskId) => {
      const r = await c.core.listDynamicFields({ parentId: kioskId, limit: 100 });
      // Kiosk items are dynamic object fields keyed by kiosk::Item { id }; the value is the object itself.
      return r.dynamicFields.filter((f) => f.type.includes("::kiosk::Item")).map((f) => ({ objectId: f.childId ?? f.fieldId, type: canonType(f.valueType) }));
    },
    objects: async (ids) => {
      if (!ids.length) return [];
      const r = await c.core.getObjects({ objectIds: ids, include: { json: true } });
      return r.objects.flatMap((o) => (o instanceof Error ? [] : [{ objectId: o.objectId, type: canonType(o.type), json: (o.json as Record<string, unknown> | null) ?? null }]));
    },
  };
}
