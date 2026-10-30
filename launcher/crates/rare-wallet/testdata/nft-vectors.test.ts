// Cross-check vectors between the TypeScript NFT send flow (launcher/src/wallet/tx/nft, nftSend.ts,
// recipient.ts) and the Rust port (rare-wallet src/nft, src/recipient.rs). This file runs the TS
// builders over fixed inputs and fake chain states and compares the result with nft-vectors.json;
// the Rust tests (src/nft/tests.rs, src/recipient.rs) feed the same inputs and chain states to the
// Rust code and must print the same `tx.getData()` JSON, sizes, batches, policy decisions,
// sendability and recipient states.
//
//   npx vitest run crates/rare-wallet/testdata/nft-vectors.test.ts                       check
//   RARE_WRITE_VECTORS=1 npx vitest run crates/rare-wallet/testdata/nft-vectors.test.ts  rewrite

import { describe, expect, it } from "vitest";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { bcs } from "@mysten/sui/bcs";
import { fromBase64, normalizeStructTag, normalizeSuiAddress, toBase64, toHex } from "@mysten/sui/utils";
import { fits, measure, planBatches, SUI_LIMITS, typeTagBytes, type Limits } from "../../../src/wallet/tx/nft/limits";
import { analyzeNfts, policyIdsFromTrade, policyLoader, readPolicies, type NftChainReader } from "../../../src/wallet/tx/nft/context";
import { KIOSK_EXT_PACKAGE, KIOSK_OWNER_CAP_TYPE, OB_OWNER_TOKEN_TYPE, PERSONAL_KIOSK_CAP_TYPE, TRADEPORT_ORDERBOOK_STORE, TRADEPORT_STORE, DRIP_LISTINGS_BAG } from "../../../src/wallet/tx/nft/ids";
import { matchRule, pickPolicy, policyNeeds, policyProblem, unsupportedReason } from "../../../src/wallet/tx/nft/policy";
import { buildSendNfts, planNftSends } from "../../../src/wallet/tx/nft/transfer";
import type { KioskRef, PolicyRule, SendItem, TransferPolicyInfo } from "../../../src/wallet/tx/nft/types";
import { buildNftSend } from "../../../src/wallet/tx/nftSend";
import { isAddress, parseRecipient, recipientState, shortAddress, toDotSui, type NameLookup } from "../../../src/wallet/tx/recipient";

const FILE = fileURLToPath(new URL("./nft-vectors.json", import.meta.url));
const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v));

// ---------- fixtures (send.test.ts) ----------

const SENDER = "0x" + "a1".repeat(32);
const TO = "0x" + "b2".repeat(32);
const id = (n: number) => "0x" + n.toString(16).padStart(64, "0");
const NFT_T = "0x" + "c3".repeat(32) + "::punks::Punk";
const BLOCKED_T = "0x" + "d4".repeat(32) + "::sbt::Badge";
const SBT_T = "0x" + "e5".repeat(32) + "::badge::Badge";

const PLAIN_KIOSK: KioskRef = { kioskId: id(0xa000), ownerCapId: id(0xa001), isPersonal: false };
const PERSONAL_KIOSK: KioskRef = { kioskId: id(0xb000), ownerCapId: id(0xb001), personalCapId: id(0xb002), isPersonal: true };
const OTHER_KIOSK: KioskRef = { kioskId: id(0x9000), ownerCapId: id(0x9001), isPersonal: false };

const rule = (module: string, pkg = KIOSK_EXT_PACKAGE): PolicyRule => ({ type: module, ruleType: `${pkg}::${module}::Rule` });
const STAKING_PKG = "0x3ab9fef3dcf219a23355fa0e3d4f7d9cdd421ddba4e3b2cb566543d4a10b4c38";
const COPY_PKG = "0x1f015c1a000631f2c0afd0258b299c63ffb8e5452e0c0f9be8fc28d2e20ad9b4";
const DISABLE = { type: "disable_transfer_rule", ruleType: "0xc88b1c7df0336eb4310b7d420ab93ce63a4190b1c9d5024a8df8f4ff4b8f1760::disable_transfer_rule::Rule" };

const P: Record<string, TransferPolicyInfo> = {
  royaltyLock: { id: id(0xf001), isShared: true, rules: [rule("royalty_rule"), rule("kiosk_lock_rule")] },
  personal: { id: id(0xf002), isShared: true, rules: [rule("kiosk_lock_rule"), rule("personal_kiosk_rule")] },
  floor: { id: id(0xf003), isShared: true, rules: [{ ...rule("floor_price_rule"), config: { floor_price: "1000" } }] },
  staking: { id: id(0xf004), isShared: true, rules: [{ ...rule("staking_lock_rule", STAKING_PKG), config: { registry_id: id(0xee01) } }] },
  stakingNoRegistry: { id: id(0xf005), isShared: true, rules: [rule("staking_lock_rule", STAKING_PKG)] },
  blocked: { id: id(0xf006), isShared: true, rules: [DISABLE] },
  copyRoyalty: { id: id(0xf007), isShared: true, rules: [rule("royalty_rule", COPY_PKG), rule("kiosk_lock_rule", COPY_PKG)] },
  royaltyOnly: { id: id(0xf008), rules: [rule("royalty_rule")] },
  owned: { id: id(0xf009), isShared: false, rules: [] },
  byId: { id: id(0xf00a), isShared: true, rules: [{ type: "royalty_rule" }, { type: "kiosk_lock_rule", ruleType: "" }] },
  floorNumber: { id: id(0xf00b), isShared: true, rules: [{ ...rule("floor_price_rule"), config: { floorPrice: 2500.7 } }, rule("royalty_rule")] },
  floorBad: { id: id(0xf00c), isShared: true, rules: [{ ...rule("floor_price_rule"), config: { floor: "1.5" } }] },
  custom: { id: id(0xf00d), isShared: true, rules: [{ type: "x", ruleType: "0x77::my_rule::Rule<0x2::sui::SUI>" }, { type: "bare" }] },
  personalOrigin: { id: id(0xf00e), isShared: true, rules: [rule("personal_kiosk_rule", "0x0cb4bcc0560340eb1a1b929cabe56b33fc6449820ec8c1980d69bb98b649b802"), rule("kiosk_lock_rule", "0x434b5bd8f6a7b05fede0ff46c6e511d71ea326ed38056e3bcd681d2d7c2a7879")] },
};

const wallet = (n: number, t = NFT_T): SendItem => ({ from: null, item: { nftId: id(n), nftType: t, isLocked: false } });
const unlocked = (n: number, from = PLAIN_KIOSK, extra = {}): SendItem => ({ from, item: { nftId: id(n), nftType: NFT_T, isLocked: false, ...extra } });
const locked = (n: number, policy = P.royaltyLock, from = PLAIN_KIOSK): SendItem => ({ from, item: { nftId: id(n), nftType: NFT_T, isLocked: true }, policies: [policy] });
const lockedWith = (n: number, policies: TransferPolicyInfo[], from = PLAIN_KIOSK): SendItem => ({ from, item: { nftId: id(n), nftType: NFT_T, isLocked: true }, policies });

const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e));

// ---------- builders ----------

// big selections are generated on both sides from { kind, count }: ids 1..=count, the fixtures above
type Gen = { kind: "wallet" | "locked" | "unlocked"; count: number };
const gen = (g: Gen): SendItem[] => Array.from({ length: g.count }, (_, i) => (g.kind === "wallet" ? wallet(i + 1) : g.kind === "locked" ? locked(i + 1) : unlocked(i + 1)));

const BUILD: Array<[string, SendItem[], string?]> = [
  ["wallet three", [1, 2, 3].map((n) => wallet(n))],
  ["wallet 520 splits transfers", gen({ kind: "wallet", count: 520 })],
  ["wallet duplicate id is one input", [wallet(1), wallet(1), wallet(2)]],
  ["unlocked kiosk items with a wallet one", [unlocked(1), unlocked(2), wallet(3)]],
  [
    "listings cancelled first",
    [
      unlocked(1, PLAIN_KIOSK, { isListed: true }),
      unlocked(2, PLAIN_KIOSK, { isListed: true, market: { kind: "drip" } }),
      unlocked(3, PLAIN_KIOSK, { isListed: true, isExclusiveListing: true, market: { kind: "tradeport", orderbook: false } }),
      unlocked(4, PLAIN_KIOSK, { isListed: true, isExclusiveListing: true, market: { kind: "tradeport", orderbook: true } }),
    ],
  ],
  ["listed elsewhere with a purchase cap", [unlocked(1, PLAIN_KIOSK, { isListed: true, isExclusiveListing: true })]],
  ["locked royalty into a new kiosk", [locked(1)]],
  ["two locked from one personal kiosk", [locked(1, P.royaltyLock, PERSONAL_KIOSK), locked(2, P.royaltyLock, PERSONAL_KIOSK)]],
  ["personal policy uses create_for", [locked(1, P.personal)]],
  ["floor price bought and withdrawn", [locked(1, P.floor)]],
  ["mixed set", [wallet(1), unlocked(2), locked(3), locked(4, P.personal, PERSONAL_KIOSK), wallet(5)]],
  ["same account", [wallet(1)], SENDER],
  ["staking lock rule", [locked(1, P.staking)]],
  ["staking rule without registry", [locked(1, P.stakingNoRegistry)]],
  ["blocked policy only", [locked(1, P.blocked)]],
  ["no policies", [lockedWith(1, [])]],
  ["copy package royalty", [locked(1, P.copyRoyalty)]],
  ["cheapest of several policies", [lockedWith(1, [P.royaltyLock, P.owned, P.blocked, P.royaltyOnly])]],
  ["rules named by id only", [locked(1, P.byId)]],
  ["floor from a number config", [locked(1, P.floorNumber)]],
  ["floor that does not parse", [locked(1, P.floorBad)]],
  ["personal rule at its origin package", [locked(1, P.personalOrigin, OTHER_KIOSK)]],
  ["personal kiosk without its cap id", [unlocked(1, { ...PERSONAL_KIOSK, personalCapId: null })]],
  ["kiosk without its cap id", [unlocked(1, { ...PLAIN_KIOSK, ownerCapId: "" })]],
  [
    "both recipient kiosks across three source kiosks",
    [locked(1, P.personal, OTHER_KIOSK), locked(2, P.royaltyLock, PERSONAL_KIOSK), unlocked(3, OTHER_KIOSK, { isListed: true }), locked(4, P.floorNumber), locked(5, P.personal), wallet(6)],
  ],
  ["empty selection", []],
];

function buildCase([name, items, to]: [string, SendItem[], string?]) {
  // long generated lists are stored as their recipe
  const stored = items.length > 100 ? { gen: { kind: "wallet", count: items.length } } : { items };
  try {
    const tx = buildSendNfts(SENDER, to ?? TO, items);
    const size = measure(tx);
    return { name, to: to ?? TO, ...stored, tx: tx.getData(), size, fits: fits(size) };
  } catch (e) {
    return { name, to: to ?? TO, ...stored, error: errMsg(e) };
  }
}

const WRAP: Array<[string, { sender: string; to: string; items: SendItem[] }]> = [
  ["one wallet item", { sender: SENDER, to: TO, items: [{ from: null, item: { nftId: normalizeSuiAddress("0x5"), nftType: "0x9::a::B", isLocked: false } }] }],
  ["short addresses normalise", { sender: " 0xA1 ", to: "0xb2", items: [unlocked(7)] }],
  ["no items", { sender: SENDER, to: TO, items: [] }],
  ["bad recipient", { sender: SENDER, to: "nope", items: [] }],
  ["bad sender", { sender: "0x12zz", to: TO, items: [wallet(1)] }],
  ["same account", { sender: SENDER, to: SENDER.toUpperCase().replace("0X", "0x"), items: [wallet(1)] }],
  ["builder error passes through", { sender: SENDER, to: TO, items: [locked(1, P.blocked)] }],
];

function wrapCase([name, p]: [string, { sender: string; to: string; items: SendItem[] }]) {
  try {
    return { name, ...p, tx: buildNftSend(p).getData() };
  } catch (e) {
    return { name, ...p, error: errMsg(e), code: (e as { code?: string }).code ?? null };
  }
}

const SMALL: Limits = { commands: 40, args: 512, objects: 2048, bytes: 4096 };
const TINY: Limits = { commands: 5, args: 512, objects: 2048, bytes: 100 * 1024 };

const PLAN: Array<{ name: string; items?: SendItem[]; gen?: Gen; limits?: Limits }> = [
  { name: "normal selection is one transaction", items: [wallet(1), unlocked(2), locked(3)] },
  { name: "thousands of wallet NFTs", gen: { kind: "wallet", count: 3000 } },
  { name: "many locked items", gen: { kind: "locked", count: 150 } },
  { name: "many unlocked kiosk items", gen: { kind: "unlocked", count: 700 } },
  {
    name: "grouped by kiosk, wallet last, small limits",
    items: [wallet(1), locked(2, P.royaltyLock, PERSONAL_KIOSK), unlocked(3, OTHER_KIOSK), locked(4), wallet(5), unlocked(6, PERSONAL_KIOSK), locked(7, P.personal, OTHER_KIOSK), unlocked(8), locked(9, P.floor), wallet(10)],
    limits: SMALL,
  },
  { name: "one item too big", items: [wallet(1), locked(2)], limits: TINY },
  { name: "builder error surfaces", items: [wallet(1), unlocked(2, PLAIN_KIOSK, { isListed: true, isExclusiveListing: true })] },
  { name: "nothing to plan", items: [] },
];

function planCase(c: (typeof PLAN)[number]) {
  const items = c.items ?? gen(c.gen!);
  const limits = c.limits ?? SUI_LIMITS;
  const base = { name: c.name, ...(c.items ? { items } : { gen: c.gen }), ...(c.limits ? { limits } : {}) };
  try {
    const batches = planNftSends(SENDER, TO, items, limits);
    return {
      ...base,
      batches: batches.map((b) => ({ ids: b.length > 20 ? [b[0].item.nftId, b[b.length - 1].item.nftId] : b.map((s) => s.item.nftId), count: b.length, size: measure(buildSendNfts(SENDER, TO, b)) })),
    };
  } catch (e) {
    return { ...base, error: errMsg(e) };
  }
}

// planBatches on its own: numbers whose "transaction" is n wallet transfers
function planBatchesCase() {
  const items = Array.from({ length: 50 }, (_, i) => i + 1);
  const limits: Limits = { commands: 1024, args: 512, objects: 7, bytes: 100 * 1024 };
  return planBatches(items, (slice) => buildSendNfts(SENDER, TO, slice.map((n) => wallet(n))), limits).map((b) => b.length);
}

// ---------- policies ----------

const POLICY_SETS: Array<[string, TransferPolicyInfo[] | null]> = [
  ["null", null],
  ["empty", []],
  ["royalty lock", [P.royaltyLock]],
  ["owned only", [P.owned]],
  ["blocked only", [P.blocked]],
  ["blocked and custom", [P.blocked, P.custom]],
  ["cheapest wins", [P.personal, P.royaltyLock, P.royaltyOnly, P.floor]],
  ["tie keeps order", [{ ...P.royaltyOnly, id: id(0xf101) }, { ...P.royaltyOnly, id: id(0xf102) }]],
  ["by id", [P.byId]],
  ["copy package", [P.copyRoyalty]],
  ["staking", [P.staking, P.stakingNoRegistry]],
  ["floor shapes", [P.floorNumber, P.floorBad, P.floor]],
  ["missing id is skipped", [{ ...P.royaltyLock, id: "" }]],
];

const needsOf = (p: TransferPolicyInfo) => {
  const n = policyNeeds(p);
  return { lock: n.lock, personal: n.personal, royalty: n.royalty, floor: n.floor === null ? null : n.floor.toString(), rules: n.rules.map((r) => ({ id: r.spec.id, pkg: r.pkg, callPkg: r.callPkg })), unsupported: n.unsupported };
};

const RULES: PolicyRule[] = [
  rule("royalty_rule"),
  rule("royalty_rule", "0x434b5bd8f6a7b05fede0ff46c6e511d71ea326ed38056e3bcd681d2d7c2a7879"),
  rule("royalty_rule", COPY_PKG),
  { type: "royalty_rule", ruleType: `${KIOSK_EXT_PACKAGE.toUpperCase().replace("0X", "0x")}::royalty_rule::Rule<0x2::sui::SUI>` },
  { type: "x", ruleType: `${KIOSK_EXT_PACKAGE}::royalty_rule::Other` },
  { type: "x", ruleType: `${KIOSK_EXT_PACKAGE}::royalty_rule` },
  { type: "x", ruleType: "dfb4f1d4e43e0c3ad834dcd369f0d39005c872e118c9dc1c5da9765bb93ee5f3::kiosk_lock_rule::Rule" },
  { type: "floor_price_rule" },
  { type: "staking_lock_rule" },
  { type: "nope" },
  DISABLE,
  { type: "x", ruleType: "0xbc3df36be17f27ac98e3c839b2589db8475fa07b20657b08e8891e3aaf5ee5f9::p2p_list::Rule" },
  { type: "x", ruleType: "0x0bc0c3c2d5df99933570781c2e84f0ef1c91e3d14d054e65b7bb386ff5d21951::nft::Other" },
  { type: "x", ruleType: "0x77::my_rule::Rule<0x2::sui::SUI>" },
  { type: "just_a_name" },
  { type: "x", ruleType: "0x77" },
];

const ruleCase = (r: PolicyRule) => {
  const m = matchRule(r);
  return { rule: r, match: m ? { id: m.spec.id, pkg: m.pkg, callPkg: m.callPkg } : null, reason: unsupportedReason(r) };
};

const TYPES = [
  "0x2::sui::SUI",
  NFT_T,
  "u64",
  "vector<u8>",
  "vector<0x2::sui::SUI>",
  "0x2::coin::Coin<0x2::sui::SUI>",
  "0x1::m::T<u8, vector<u64>, 0x2::a::B<0x3::c::D>>",
  "0xabc::pair::Pair<0x2::sui::SUI,0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC>",
  "0x1::m::T<",
  "0x1::m",
  "0x1::m::",
  "0x1::a::b::c",
  "  0x2::kiosk::Kiosk  ",
  "vector<",
  "0x1::m::T<vector<0x2::a::B<u8>>>",
  "0x1::m::T<vector<>>",
];

// ---------- chain fakes (shared JSON shape, read by both sides) ----------

interface ChainState {
  /** listOwnedObjects by type */
  owned: Record<string, Array<{ objectId: string; content: string }>>;
  /** listDynamicFields by parent; "error" makes the listing fail */
  fields: Record<string, Array<{ nameType: string; nameBcs: string; valueType: string }> | "error">;
  /** getDynamicObjectField(parent, ID key) -> object id */
  dof: Array<{ parent: string; key: string; objectId: string }>;
  /** getDynamicField(parent, name) -> value bcs */
  df: Array<{ parent: string; nameType: string; nameBcs: string; value: string }>;
  /** getObjects / getObject */
  objects: Record<string, { content?: string; owner?: string; previousTransaction?: string | null }>;
  /** getTransaction(digest).objectTypes */
  txTypes: Record<string, Array<[string, string]>>;
  /** whether a struct type (no type args) has store; missing = unknown */
  store: Record<string, boolean>;
}

const n = (a: string) => normalizeSuiAddress(a);
const sameType = (a: string, b: string) => {
  try {
    return normalizeStructTag(a) === normalizeStructTag(b);
  } catch {
    return a === b;
  }
};

function reader(s: ChainState): NftChainReader {
  return {
    listOwnedObjects: async ({ type }) => ({
      objects: Object.entries(s.owned)
        .filter(([t]) => sameType(t, type!))
        .flatMap(([t, v]) => v.map((o) => ({ objectId: o.objectId, type: t, content: fromBase64(o.content) }))),
      hasNextPage: false,
      cursor: null,
    }),
    listDynamicFields: async ({ parentId }) => {
      const f = s.fields[n(parentId)];
      if (f === "error") throw new Error("unavailable");
      return { dynamicFields: (f ?? []).map((x) => ({ name: { type: x.nameType, bcs: fromBase64(x.nameBcs) }, valueType: x.valueType })), hasNextPage: false, cursor: null };
    },
    getDynamicObjectField: async ({ parentId, name }) => {
      const hit = s.dof.find((d) => n(d.parent) === n(parentId) && n(d.key) === n(toHex(name.bcs)));
      if (!hit) throw new Error("not found");
      return { object: { objectId: hit.objectId } };
    },
    getDynamicField: async ({ parentId, name }) => {
      const hit = s.df.find((d) => n(d.parent) === n(parentId) && sameType(d.nameType, name.type) && d.nameBcs === toBase64(name.bcs));
      if (!hit) throw new Error("not found");
      return { dynamicField: { value: { bcs: fromBase64(hit.value) } } };
    },
    getObjects: async ({ objectIds }) => ({
      objects: objectIds.map((o) => {
        const x = s.objects[n(o)];
        if (!x) return new Error("not found");
        return { objectId: n(o), owner: x.owner ? { $kind: x.owner } : undefined, content: x.content === undefined ? undefined : fromBase64(x.content) };
      }),
    }),
    getObject: async ({ objectId }) => {
      const x = s.objects[n(objectId)];
      if (!x) throw new Error("not found");
      return { object: { previousTransaction: x.previousTransaction ?? null } };
    },
    getTransaction: async ({ digest }) => {
      const t = s.txTypes[digest];
      if (!t) throw new Error("pruned");
      return { Transaction: { objectTypes: Object.fromEntries(t) } };
    },
  };
}

const b64 = (b: Uint8Array) => toBase64(b);
const addr = (a: string) => bcs.Address.serialize(a).toBytes();
const cat = (...p: Array<Uint8Array | number[]>) => new Uint8Array(p.flatMap((x) => [...x]));
const kioskField = (kind: string, nft: string, extra: number[] = []) => ({ nameType: `0x2::kiosk::${kind}`, nameBcs: b64(cat(addr(nft), extra)), valueType: NFT_T });

const OB_KIOSK = id(0xe1);

/** send.test.ts's "analyzeNfts over gRPC" fake, as data, plus a kiosk whose listing fails */
const ANALYZE_CHAIN: ChainState = {
  owned: {
    [KIOSK_OWNER_CAP_TYPE]: [
      { objectId: PLAIN_KIOSK.ownerCapId, content: b64(cat(addr(PLAIN_KIOSK.ownerCapId), addr(PLAIN_KIOSK.kioskId))) },
      { objectId: OTHER_KIOSK.ownerCapId, content: b64(cat(addr(OTHER_KIOSK.ownerCapId), addr(OTHER_KIOSK.kioskId))) },
    ],
    [PERSONAL_KIOSK_CAP_TYPE]: [{ objectId: PERSONAL_KIOSK.personalCapId!, content: b64(cat(addr(PERSONAL_KIOSK.personalCapId!), [1], addr(PERSONAL_KIOSK.ownerCapId), addr(PERSONAL_KIOSK.kioskId))) }],
    [OB_OWNER_TOKEN_TYPE]: [{ objectId: id(0xe0), content: b64(cat(addr(id(0xe0)), addr(OB_KIOSK), addr(SENDER))) }],
  },
  fields: {
    [PLAIN_KIOSK.kioskId]: [
      kioskField("Item", id(11)), kioskField("Item", id(12)), kioskField("Lock", id(12)), kioskField("Item", id(13)), kioskField("Listing", id(13), [1]), kioskField("Item", id(14)), kioskField("Lock", id(14)),
      kioskField("Item", id(19)), kioskField("Listing", id(19), [1]), kioskField("Item", id(21)), kioskField("Listing", id(21), [0]), kioskField("Item", id(22)), kioskField("Listing", id(22), [0]),
      kioskField("Item", id(23)), kioskField("Lock", id(23)), kioskField("Item", id(24)), kioskField("Lock", id(24)), { nameType: "0x9::kiosk::Item", nameBcs: b64(addr(id(25))), valueType: NFT_T },
    ],
    [PERSONAL_KIOSK.kioskId]: [kioskField("Item", id(15)), kioskField("Listing", id(15), [1])],
    [OB_KIOSK]: [kioskField("Item", id(16))],
  },
  // Tradeport holds 15 (orderbook listing) and 19 (plain listing); Drip's registry has 21
  dof: [
    { parent: TRADEPORT_STORE, key: id(15), objectId: id(0x5000 + 15) },
    { parent: TRADEPORT_STORE, key: id(19), objectId: id(0x5000 + 19) },
  ],
  df: [
    { parent: TRADEPORT_ORDERBOOK_STORE, nameType: "0x2::object::ID", nameBcs: b64(addr(id(0x5000 + 15))), value: b64(new Uint8Array([0, 0, 0, 0, 0, 0, 0, 0])) },
    { parent: DRIP_LISTINGS_BAG, nameType: "0x2::object::ID", nameBcs: b64(addr(id(21))), value: b64(new Uint8Array([7])) },
  ],
  objects: {},
  txTypes: {},
  store: { [NFT_T]: true, [SBT_T]: false, [BLOCKED_T]: true },
};

const ANALYZE_NFTS = [
  { objectId: id(10), type: NFT_T },
  { objectId: id(11), type: NFT_T, kiosk: true },
  { objectId: id(12), type: NFT_T, kiosk: true },
  { objectId: id(13), type: NFT_T, kiosk: true },
  { objectId: id(14), type: BLOCKED_T, kiosk: true },
  { objectId: id(15), type: NFT_T, kiosk: true },
  { objectId: id(16), type: NFT_T, kiosk: true },
  { objectId: id(17), type: NFT_T, kiosk: true },
  { objectId: id(19), type: NFT_T, kiosk: true },
  { objectId: id(21), type: NFT_T, kiosk: true },
  { objectId: id(22), type: NFT_T, kiosk: true },
  { objectId: id(18), type: SBT_T },
  { objectId: "0x1A", type: "0x" + "f6".repeat(32) + "::unknown::Thing<u8>" },
  { objectId: id(23), type: "0x" + "f7".repeat(32) + "::locked::Err", kiosk: true },
  { objectId: id(24), type: "0x" + "f8".repeat(32) + "::locked::Empty", kiosk: true },
  { objectId: id(25), type: NFT_T, kiosk: true },
];

/** policies by type for the analysis; "error" makes the lookup fail */
const ANALYZE_POLICIES: Record<string, TransferPolicyInfo[] | "error"> = {
  [NFT_T]: [P.royaltyLock],
  [BLOCKED_T]: [P.blocked],
  ["0x" + "f7".repeat(32) + "::locked::Err"]: "error",
  ["0x" + "f8".repeat(32) + "::locked::Empty"]: [],
};

async function analyzeCase(name: string, chain: ChainState, nfts: typeof ANALYZE_NFTS, policies: typeof ANALYZE_POLICIES, hasStore: boolean) {
  const r = await analyzeNfts(
    {
      client: reader(chain),
      policies: async (t) => {
        const p = policies[t];
        if (p === "error" || p === undefined) throw new Error("lookup failed");
        return p;
      },
      hasStore: hasStore ? async (t) => chain.store[t] : undefined,
    },
    SENDER,
    nfts,
  );
  return { name, chain, nfts, policies, hasStore, result: clone(Object.fromEntries(r)) };
}

// readPolicies and the Trade API / history loader

const policyBytes = (pid: string, rules: string[]) => bcs.struct("P", { id: bcs.Address, balance: bcs.u64(), rules: bcs.vector(bcs.string()) }).serialize({ id: pid, balance: 5n, rules }).toBytes();
const strip = (t: string) => t.slice(2);
const ROYALTY_T = `${strip(KIOSK_EXT_PACKAGE)}::royalty_rule::Rule`;
const FLOOR_T = `${strip(KIOSK_EXT_PACKAGE)}::floor_price_rule::Rule`;
const STAKE_T = `${strip(STAKING_PKG)}::staking_lock_rule::Rule`;
const LOCK_T = `0x${strip(KIOSK_EXT_PACKAGE)}::kiosk_lock_rule::Rule`;
const ruleKey = (t: string) => `0x2::transfer_policy::RuleKey<0x${t}>`;

const POLICY_CHAIN: ChainState = {
  owned: {},
  fields: {
    [id(0xf003)]: [{ nameType: ruleKey(FLOOR_T), nameBcs: b64(new Uint8Array([0])), valueType: "x" }, { nameType: "0x2::balance::Balance<0x2::sui::SUI>", nameBcs: b64(new Uint8Array([0])), valueType: "x" }],
    [id(0xf004)]: [{ nameType: ruleKey(STAKE_T), nameBcs: b64(new Uint8Array([0])), valueType: "x" }],
    [id(0xf00f)]: "error",
  },
  dof: [],
  df: [
    { parent: id(0xf003), nameType: ruleKey(FLOOR_T), nameBcs: b64(new Uint8Array([0])), value: b64(bcs.u64().serialize(123456789n).toBytes()) },
    { parent: id(0xf004), nameType: ruleKey(STAKE_T), nameBcs: b64(new Uint8Array([0])), value: b64(cat(addr(id(0xee01)), [9, 9])) },
  ],
  objects: {
    [id(0xf001)]: { owner: "Shared", content: b64(policyBytes(id(0xf001), [ROYALTY_T, LOCK_T])) },
    [id(0xf003)]: { owner: "Shared", content: b64(policyBytes(id(0xf003), [FLOOR_T, ROYALTY_T])) },
    [id(0xf004)]: { owner: "ConsensusAddressOwner", content: b64(policyBytes(id(0xf004), [STAKE_T])) },
    [id(0xf00f)]: { owner: "Shared", content: b64(policyBytes(id(0xf00f), [FLOOR_T])) },
    [id(0xf009)]: { owner: "AddressOwner", content: b64(policyBytes(id(0xf009), [])) },
    [id(0xf010)]: { content: b64(policyBytes(id(0xf010), ["0x77::my_rule::Rule"])) },
    [id(0xf011)]: { owner: "Shared", content: b64(new Uint8Array([1, 2, 3])) },
    [id(0xf012)]: { owner: "Shared" },
    // the NFT whose last transaction locked it under 0xf001
    [id(12)]: { previousTransaction: "DIGEST" },
    [id(13)]: { previousTransaction: null },
    [id(14)]: { previousTransaction: "PRUNED" },
  },
  txTypes: {
    DIGEST: [
      [id(0xf001), `0x2::transfer_policy::TransferPolicy<${NFT_T}>`],
      [id(0xa000), "0x2::kiosk::Kiosk"],
      [id(0xf003), `0x0000000000000000000000000000000000000000000000000000000000000002::transfer_policy::TransferPolicy<${NFT_T.toUpperCase().replace("0X", "0x").replace("::PUNKS::PUNK", "::punks::Punk")}>`],
      [id(0xf00a), "not a type"],
    ],
  },
  store: {},
};

const READ_IDS = [id(0xf001), id(0xf003), id(0xf004), id(0xf00f), id(0xf009), id(0xf010), id(0xf011), id(0xf012), id(0xf0ff), id(0xf001)];

type TradeReply = { status: number; body: unknown } | "offline";
const LOADER: Array<{ name: string; trade: TradeReply; nftId: string; nftType?: string }> = [
  { name: "the Trade API names the policies", trade: { status: 200, body: { policies: [{ id: "0xf004" }, { id: 7 }, { id: id(0xf001), rules: [] }] } }, nftId: id(12) },
  { name: "the Trade API down falls back to history", trade: { status: 503, body: { error: "x" } }, nftId: id(12) },
  { name: "the Trade API knows none", trade: { status: 200, body: { policies: [] } }, nftId: id(12) },
  { name: "the Trade API answers junk", trade: { status: 200, body: { nope: 1 } }, nftId: id(12) },
  { name: "offline", trade: "offline", nftId: id(12) },
  { name: "the Trade API ids that do not exist fall back", trade: { status: 200, body: { policies: [{ id: id(0xf0ff) }] } }, nftId: id(12) },
  { name: "no previous transaction", trade: "offline", nftId: id(13) },
  { name: "pruned history", trade: "offline", nftId: id(14) },
  { name: "unknown nft", trade: "offline", nftId: id(99) },
  { name: "unparsable type", trade: "offline", nftId: id(12), nftType: "not a type" },
];

function fakeFetch(y: TradeReply, seen: string[]): typeof fetch {
  return (async (url: string) => {
    seen.push(url);
    if (y === "offline") throw new Error("offline");
    return new Response(JSON.stringify(y.body), { status: y.status, headers: { "content-type": "application/json" } });
  }) as unknown as typeof fetch;
}

async function loaderCase(c: (typeof LOADER)[number]) {
  const seen: string[] = [];
  const nftType = c.nftType ?? NFT_T;
  const base = "https://rare.market/v1/trade//";
  let tradeIds: string[] | { error: string };
  try {
    tradeIds = await policyIdsFromTrade(base, nftType, fakeFetch(c.trade, []));
  } catch (e) {
    tradeIds = { error: errMsg(e) };
  }
  let result: TransferPolicyInfo[] | { error: string };
  try {
    result = clone(await policyLoader(reader(POLICY_CHAIN), base, fakeFetch(c.trade, seen))(nftType, c.nftId));
  } catch (e) {
    result = { error: errMsg(e) };
  }
  return { ...c, nftType, base, urls: seen, tradeIds, result };
}

// ---------- recipient ----------

const A = "0x1eb7c57e3f2bd0fc6cb9dcffd143ea957e4d98f805c358733f76dee0667fe0b1";
const INPUTS = [
  A,
  ` ${A.toUpperCase().replace("0X", "0x")} `,
  A.toUpperCase(),
  "Adeniyi.SUI",
  "@adeniyi",
  "pay@adeniyi",
  "a.b@adeniyi",
  "",
  "   ",
  "0x12",
  "0x12zz",
  "0x",
  "bad_name.sui",
  "hello",
  "@",
  "a@b@c",
  "alice@",
  "@alice.sui",
  ".sui",
  "-bad.sui",
  "bad-.sui",
  "ok-1.sui",
  "x".repeat(63) + ".sui",
  "x".repeat(64) + ".sui",
  "a..b.sui",
  "Sub.Name.Sui",
  "﻿bob.sui",
  "bob.sui\n",
  "0x" + "1".repeat(65),
];
const SHORT = [A, "0x1234", "0x123456789012", "0x1234567890123", ""];
const LOOKUPS: NameLookup[] = [{ status: "idle" }, { status: "loading" }, { status: "found", address: A }, { status: "missing" }, { status: "error" }];

function recipientVectors() {
  const parse = INPUTS.map((input) => ({ input, parsed: parseRecipient(input), dotSui: toDotSui(input), isAddress: isAddress(input) }));
  const states: unknown[] = [];
  for (const input of [A, "@alice", "nobody.sui", "0x12", "", "hello"])
    for (const lookup of LOOKUPS)
      for (const reverse of [null, "alice.sui"])
        for (const paused of [false, true]) states.push({ input, lookup, reverse, paused, state: recipientState(parseRecipient(input), lookup, reverse, paused) });
  return { parse, short: SHORT.map((a) => [a, shortAddress(a)]), states };
}

// ---------- all of it ----------

async function generate() {
  const policySets = POLICY_SETS.map(([name, set]) => ({
    name,
    policies: set,
    pick: pickPolicy(set)?.id ?? null,
    problem: policyProblem(set),
    needs: (set ?? []).map(needsOf),
  }));
  const partialChain: ChainState = { ...ANALYZE_CHAIN, fields: { ...ANALYZE_CHAIN.fields, [OTHER_KIOSK.kioskId]: "error" } };
  const walletOnly = ANALYZE_NFTS.filter((x) => !x.kiosk);
  return clone({
    fixtures: { sender: SENDER, to: TO },
    build: BUILD.map(buildCase),
    wrap: WRAP.map(wrapCase),
    plan: PLAN.map(planCase),
    planBatches: planBatchesCase(),
    policySets,
    rules: RULES.map(ruleCase),
    typeTagBytes: TYPES.map((t) => [t, typeTagBytes(t)]),
    analyze: [
      await analyzeCase("send.test.ts chain", ANALYZE_CHAIN, ANALYZE_NFTS, ANALYZE_POLICIES, true),
      await analyzeCase("a kiosk that can not be listed", partialChain, ANALYZE_NFTS, ANALYZE_POLICIES, false),
      await analyzeCase("wallet only never reads kiosks", { ...ANALYZE_CHAIN, owned: {} }, walletOnly, {}, true),
    ],
    readPolicies: { chain: POLICY_CHAIN, ids: READ_IDS, result: await readPolicies(reader(POLICY_CHAIN), READ_IDS) },
    loader: await Promise.all(LOADER.map(loaderCase)),
    recipient: recipientVectors(),
  });
}

describe("TS/Rust NFT send vectors", () => {
  it("the TS NFT builders still produce nft-vectors.json", async () => {
    const got = await generate();
    if (process.env.RARE_WRITE_VECTORS) writeFileSync(FILE, JSON.stringify(got) + "\n");
    expect(got).toEqual(JSON.parse(readFileSync(FILE, "utf8")));
  });
});
