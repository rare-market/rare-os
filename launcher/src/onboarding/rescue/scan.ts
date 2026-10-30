// Read-only scan of a Playtron (GameOS) wallet over Sui gRPC.
//
// Lists everything the address owns and sorts it into coins, collectibles, kiosks (with
// listed and locked items), staked SUI, SuiNS names and soulbound objects (`key` without
// `store`, which only their own package can move). Nothing here signs or sends anything.

import { SuiGrpcClient } from "@mysten/sui/grpc";
import { normalizeStructTag, normalizeSuiAddress, parseStructTag } from "@mysten/sui/utils";
import type { RescueCoin, RescueItem, RescueKiosk, RescueKioskItem, RescueName, RescueScan, RescueStake } from "./types";

export const SUI_TYPE = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
const TWO = normalizeSuiAddress("0x2");
const THREE = normalizeSuiAddress("0x3");
/** owned objects read per scan; a wallet past this is flagged as truncated */
const MAX_OBJECTS = 5000;
const PAGE = 500;
/** Mysten's kiosk extensions package (personal_kiosk); upgradeable, so calls go to its latest version. */
export const KIOSK_EXT_LATEST = "0xdfb4f1d4e43e0c3ad834dcd369f0d39005c872e118c9dc1c5da9765bb93ee5f3";

let shared: SuiGrpcClient | null = null;
export function rescueClient(): SuiGrpcClient {
  shared ??= new SuiGrpcClient({ network: "mainnet", baseUrl: "https://fullnode.mainnet.sui.io:443" });
  return shared;
}

export interface ScanOptions {
  client?: SuiGrpcClient;
  onProgress?: (stage: string) => void;
  signal?: AbortSignal;
}

/** One owned object as the scan sees it. */
interface Raw {
  objectId: string;
  type: string;
  movable: boolean;
  json: Record<string, unknown> | null;
  display: Record<string, unknown> | null;
  balance: bigint | null;
}

const READ = ["object_id", "object_type", "has_public_transfer", "json", "display", "balance"];

function toJson(v: unknown): unknown {
  // gRPC returns google.protobuf.Value; the SDK's Value.toJson isn't exported from a public
  // path, so walk the oneof shape here.
  if (v == null || typeof v !== "object") return v ?? null;
  const k = (v as { kind?: { oneofKind?: string } & Record<string, unknown> }).kind;
  if (!k || !k.oneofKind) return null;
  switch (k.oneofKind) {
    case "nullValue":
      return null;
    case "numberValue":
    case "stringValue":
    case "boolValue":
      return k[k.oneofKind];
    case "structValue": {
      const out: Record<string, unknown> = {};
      const fields = (k.structValue as { fields?: Record<string, unknown> }).fields ?? {};
      for (const [key, val] of Object.entries(fields)) out[key] = toJson(val);
      return out;
    }
    case "listValue":
      return ((k.listValue as { values?: unknown[] }).values ?? []).map(toJson);
    default:
      return null;
  }
}

function asObj(v: unknown): Record<string, unknown> | null {
  return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
}

function str(v: unknown): string | null {
  if (typeof v === "string") return v;
  if (typeof v === "number") return String(v);
  return null;
}

function big(v: unknown): bigint {
  const o = asObj(v);
  if (o && "value" in o) return big(o.value);
  const s = str(v);
  try {
    return s ? BigInt(s) : 0n;
  } catch {
    return 0n;
  }
}

function toRaw(o: {
  objectId?: string;
  objectType?: string;
  hasPublicTransfer?: boolean;
  json?: unknown;
  display?: { output?: unknown };
  balance?: bigint;
}): Raw | null {
  if (!o.objectId || !o.objectType) return null;
  return {
    objectId: normalizeSuiAddress(o.objectId),
    type: normalizeStructTag(o.objectType),
    movable: !!o.hasPublicTransfer,
    json: asObj(toJson(o.json)),
    display: asObj(toJson(o.display?.output)),
    balance: o.balance ?? null,
  };
}

async function listOwned(client: SuiGrpcClient, owner: string, signal?: AbortSignal): Promise<{ objects: Raw[]; truncated: boolean }> {
  const objects: Raw[] = [];
  let pageToken: Uint8Array | undefined;
  for (;;) {
    const { response } = await client.stateService.listOwnedObjects(
      { owner, pageSize: PAGE, pageToken, readMask: { paths: READ } },
      { abort: signal },
    );
    for (const o of response.objects) {
      const r = toRaw(o);
      if (r) objects.push(r);
    }
    pageToken = response.nextPageToken;
    if (!pageToken) return { objects, truncated: false };
    if (objects.length >= MAX_OBJECTS) return { objects, truncated: true };
  }
}

async function getObjects(client: SuiGrpcClient, ids: string[], signal?: AbortSignal): Promise<Raw[]> {
  const out: Raw[] = [];
  for (let i = 0; i < ids.length; i += 50) {
    const { response } = await client.ledgerService.batchGetObjects(
      { requests: ids.slice(i, i + 50).map((objectId) => ({ objectId })), readMask: { paths: READ } },
      { abort: signal },
    );
    for (const r of response.objects) {
      const o = r.result.oneofKind === "object" ? r.result.object : null;
      const raw = o && toRaw(o);
      if (raw) out.push(raw);
    }
  }
  return out;
}

// ---------------------------------------------------------------- classification

function isType(type: string, address: string, module: string, name: string): boolean {
  try {
    const t = parseStructTag(type);
    return normalizeSuiAddress(t.address) === address && t.module === module && t.name === name;
  } catch {
    return false;
  }
}

function hasName(type: string, module: string, name: string): boolean {
  try {
    const t = parseStructTag(type);
    return t.module === module && t.name === name;
  } catch {
    return false;
  }
}

function coinTypeOf(type: string): string | null {
  try {
    const t = parseStructTag(type);
    if (normalizeSuiAddress(t.address) !== TWO || t.module !== "coin" || t.name !== "Coin") return null;
    const p = t.typeParams[0];
    return typeof p === "string" ? normalizeStructTag(p) : normalizeStructTag(p);
  } catch {
    return null;
  }
}

function pretty(name: string): string {
  return name.replace(/_/g, " ").replace(/([a-z])([A-Z])/g, "$1 $2");
}

function item(o: Raw): RescueItem {
  const d = o.display ?? {};
  const j = o.json ?? {};
  let mod = "";
  let typeName = "";
  try {
    const t = parseStructTag(o.type);
    mod = t.module;
    typeName = t.name;
  } catch {
    /* odd type string */
  }
  const name = str(d.name) ?? str(j.name) ?? pretty(typeName || "Object");
  const image = str(d.image_url) ?? str(d.image) ?? str(j.image_url) ?? str(j.url) ?? null;
  const collection = str(d.collection_name) ?? str(d.collection) ?? pretty(mod || typeName);
  return { objectId: o.objectId, type: o.type, name, image: image && /^(https?|ipfs|data):/.test(image) ? image : null, collection };
}

/** The KioskOwnerCap inside a PersonalKioskCap's `cap` (an Option, so JSON may be a value or a 1-item list). */
function personalInner(j: Record<string, unknown> | null): string | null {
  const cap = j?.cap;
  const one = Array.isArray(cap) ? cap[0] : asObj(cap)?.vec && Array.isArray(asObj(cap)!.vec) ? (asObj(cap)!.vec as unknown[])[0] : cap;
  return str(asObj(one)?.for) ?? null;
}

function idFromBcs(b: Uint8Array | undefined): string | null {
  if (!b || b.length < 32) return null;
  let hex = "0x";
  for (let i = 0; i < 32; i++) hex += b[i].toString(16).padStart(2, "0");
  return hex;
}

function u64(b: Uint8Array | undefined): bigint {
  if (!b || b.length < 8) return 0n;
  let v = 0n;
  for (let i = 7; i >= 0; i--) v = (v << 8n) | BigInt(b[i]);
  return v;
}

async function readKiosk(
  client: SuiGrpcClient,
  kioskId: string,
  capId: string,
  personalPackage: string | null,
  signal?: AbortSignal,
): Promise<RescueKiosk> {
  const ids: string[] = [];
  const listed = new Map<string, bigint>();
  const locked = new Set<string>();
  const exclusive = new Set<string>();
  let cursor: string | null = null;
  for (let page = 0; page < 40; page++) {
    const r: Awaited<ReturnType<SuiGrpcClient["listDynamicFields"]>> = await client.listDynamicFields({ parentId: kioskId, cursor, limit: 200, include: { value: true }, signal });
    for (const f of r.dynamicFields) {
      const t = f.name.type ?? "";
      const id = idFromBcs(f.name.bcs);
      if (!id) continue;
      if (isType(t, TWO, "kiosk", "Item")) ids.push(id);
      else if (isType(t, TWO, "kiosk", "Listing")) {
        listed.set(id, u64((f as { value?: { bcs: Uint8Array } }).value?.bcs));
        if (f.name.bcs[32] === 1) exclusive.add(id);
      }
      else if (isType(t, TWO, "kiosk", "Lock")) locked.add(id);
    }
    if (!r.hasNextPage) break;
    cursor = r.cursor;
  }
  const [kiosk] = await getObjects(client, [kioskId], signal);
  const objs = await getObjects(client, ids, signal);
  const items: RescueKioskItem[] = objs.map((o) => ({
    ...item(o),
    listed: listed.get(o.objectId) ?? null,
    locked: locked.has(o.objectId),
    transferable: o.movable,
    exclusive: exclusive.has(o.objectId),
  }));
  return { kioskId, capId, personal: !!personalPackage, personalPackage, items, profits: big(kiosk?.json?.profits) };
}

// ---------------------------------------------------------------- scan

export async function scanPlaytron(address: string, opts: ScanOptions = {}): Promise<RescueScan> {
  const client = opts.client ?? rescueClient();
  const owner = normalizeSuiAddress(address);
  const say = opts.onProgress ?? (() => {});

  say("Reading what this wallet holds");
  const [{ objects, truncated }, balances, defaultName] = await Promise.all([
    listOwned(client, owner, opts.signal),
    listAllBalances(client, owner, opts.signal),
    client.core
      .defaultNameServiceName({ address: owner, signal: opts.signal })
      .then((r) => r.data.name)
      .catch(() => null),
  ]);

  const coinObjects = new Map<string, Array<{ id: string; raw: bigint }>>();
  const nfts: RescueItem[] = [];
  const soulbound: RescueItem[] = [];
  const staked: RescueStake[] = [];
  const names: RescueName[] = [];
  const caps: Array<{ kioskId: string; capId: string; personalPackage: string | null }> = [];

  for (const o of objects) {
    const ct = coinTypeOf(o.type);
    if (ct) {
      const list = coinObjects.get(ct) ?? [];
      list.push({ id: o.objectId, raw: o.balance ?? 0n });
      coinObjects.set(ct, list);
      continue;
    }
    if (isType(o.type, THREE, "staking_pool", "StakedSui")) {
      const j = o.json ?? {};
      staked.push({
        objectId: o.objectId,
        principal: big(j.principal),
        poolId: str(j.pool_id) ?? "",
        activationEpoch: Number(str(j.stake_activation_epoch) ?? 0),
      });
      continue;
    }
    if (isType(o.type, TWO, "kiosk", "KioskOwnerCap")) {
      const k = str(o.json?.for);
      if (k) caps.push({ kioskId: normalizeSuiAddress(k), capId: o.objectId, personalPackage: null });
      continue;
    }
    if (hasName(o.type, "personal_kiosk", "PersonalKioskCap")) {
      const k = personalInner(o.json);
      if (k) caps.push({ kioskId: normalizeSuiAddress(k), capId: o.objectId, personalPackage: KIOSK_EXT_LATEST });
      else soulbound.push(item(o));
      continue;
    }
    if (hasName(o.type, "suins_registration", "SuinsRegistration") || hasName(o.type, "subdomain_registration", "SubDomainRegistration")) {
      const sub = hasName(o.type, "subdomain_registration", "SubDomainRegistration");
      const j = sub ? asObj(o.json?.nft) ?? {} : o.json ?? {};
      const labels = asObj(j.domain)?.labels;
      const domain = str(j.domain_name) ?? (Array.isArray(labels) ? [...labels].reverse().join(".") : null) ?? "SuiNS name";
      names.push({ objectId: o.objectId, domain, expiresMs: Number(str(j.expiration_timestamp_ms) ?? 0), subname: sub });
      continue;
    }
    if (o.movable) nfts.push(item(o));
    else soulbound.push(item(o));
  }

  say("Reading coins");
  const coins = await coinRows(client, balances, coinObjects, opts.signal);

  if (caps.length) say("Reading kiosks");
  const kiosks: RescueKiosk[] = [];
  for (const c of caps) kiosks.push(await readKiosk(client, c.kioskId, c.capId, c.personalPackage, opts.signal));

  return {
    address: owner,
    scannedAt: Date.now(),
    defaultName,
    coins,
    nfts,
    kiosks,
    staked: staked.sort((a, b) => (b.principal > a.principal ? 1 : -1)),
    names,
    soulbound,
    truncated,
  };
}

interface BalanceRow {
  coinType: string;
  coinBalance: bigint;
  addressBalance: bigint;
}

async function listAllBalances(client: SuiGrpcClient, owner: string, signal?: AbortSignal): Promise<BalanceRow[]> {
  const out: BalanceRow[] = [];
  let cursor: string | null = null;
  for (let page = 0; page < 20; page++) {
    const r: Awaited<ReturnType<SuiGrpcClient["core"]["listBalances"]>> = await client.core.listBalances({ owner, cursor, limit: 100, signal });
    for (const b of r.balances) {
      out.push({ coinType: normalizeStructTag(b.coinType), coinBalance: BigInt(b.coinBalance || "0"), addressBalance: BigInt(b.addressBalance || "0") });
    }
    if (!r.hasNextPage) break;
    cursor = r.cursor;
  }
  return out;
}

async function coinRows(
  client: SuiGrpcClient,
  balances: BalanceRow[],
  objects: Map<string, Array<{ id: string; raw: bigint }>>,
  signal?: AbortSignal,
): Promise<RescueCoin[]> {
  const rows = balances.filter((b) => b.coinBalance + b.addressBalance > 0n);
  const metas = await Promise.all(
    rows.map((b) =>
      client.core
        .getCoinMetadata({ coinType: b.coinType, signal })
        .then((r) => r.coinMetadata)
        .catch(() => null),
    ),
  );
  const out = rows.map((b, i): RescueCoin => {
    const m = metas[i];
    const decimals = m?.decimals ?? 9;
    const raw = b.coinBalance + b.addressBalance;
    const own = [...(objects.get(b.coinType) ?? [])].sort((x, y) => (y.raw > x.raw ? 1 : y.raw < x.raw ? -1 : 0));
    let symbol = m?.symbol ?? "";
    if (!symbol) {
      try {
        symbol = parseStructTag(b.coinType).name;
      } catch {
        symbol = "?";
      }
    }
    return {
      coinType: b.coinType,
      symbol,
      name: m?.name ?? "",
      decimals,
      iconUrl: m?.iconUrl ?? null,
      raw,
      coinRaw: b.coinBalance,
      addressRaw: b.addressBalance,
      amount: Number(raw) / 10 ** decimals,
      objectIds: own.map((c) => c.id),
      objectBalances: own.map((c) => c.raw),
    };
  });
  return out.sort((a, b) => (a.coinType === SUI_TYPE ? -1 : b.coinType === SUI_TYPE ? 1 : b.amount - a.amount));
}

/** Plain-words reason an owned object can't be moved by its owner. */
export function stuckReason(type: string): string {
  if (hasName(type, "suiplay_nft", "SuiPlay0x1PreorderNFT")) return "Made to stay with the wallet it was sent to. Claim anything it unlocks from this wallet first.";
  if (hasName(type, "personal_kiosk", "PersonalKioskCap")) return "A personal kiosk stays with its owner. Items inside are moved one by one.";
  if (isType(type, TWO, "token", "Token")) return "A closed-loop token: only its app can send it.";
  return "Its app doesn't allow transfers. If it holds something of value, withdraw it in that app.";
}
