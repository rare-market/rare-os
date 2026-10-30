// Ported from rare.market:
//   market/packages/venues/src/policy.ts  (RULES, Policy: what the builder proves)
//   market/packages/venues/src/json.ts    (normType)
//   market/web/lib/chain/policy.ts        (readPolicy, choosePolicy over /v1/policies)
//   market/web/lib/chain/rules.json       (the rules it can satisfy; notes dropped)

import type { Transaction, TransactionArgument, TransactionObjectArgument } from "@mysten/sui/transactions";
import { CLOCK, KIOSK_EXT, KIOSK_EXT_ALL } from "./ids";
import type { PolicyRule, TransferPolicy } from "./model";

/** Type strings with full-length addresses, as the node prints them. */
export function normType(t: string): string {
  return t.replace(/0x([0-9a-fA-F]{1,64})(?=::)/g, (_, h: string) => "0x" + h.toLowerCase().padStart(64, "0"));
}

// Rule types carry the id of the extension version that introduced them.
const rule = (origin: string, m: string) => normType(`${origin}::${m}::Rule`);
export const RULES = {
  royalty: rule(KIOSK_EXT.origin, "royalty_rule"),
  kioskLock: rule(KIOSK_EXT.origin, "kiosk_lock_rule"),
  personalKiosk: rule(KIOSK_EXT.personalOrigin, "personal_kiosk_rule"),
  floorPrice: rule(KIOSK_EXT.floorOrigin, "floor_price_rule"),
};

/** Rare Store: the `<origin>::resale::Rule` on TransferPolicy<ResaleLicence>. `origin` is the package the
 * rule type was defined in, `pkg` the current package that is called. Both empty until set. */
const RESALE = { origin: "", pkg: "" };
export function configureResale(c: { origin?: string | null; pkg?: string | null }) {
  if (c.origin) RESALE.origin = normPkg(c.origin);
  if (c.pkg) RESALE.pkg = normPkg(c.pkg);
}
/** The resale rule type, or null while the store is not configured. */
export const resaleRule = (): string | null => (RESALE.origin ? normType(`${RESALE.origin}::resale::Rule`) : null);
export const isKnownRule = (r: string): boolean => Object.values(RULES).includes(r) || r === resaleRule();
/** The ResaleLicence Move type, or null while the store is not configured. */
export const resaleLicenceType = (): string | null => (RESALE.origin ? normType(`${RESALE.origin}::licence::ResaleLicence`) : null);

/** rare_store::resale::prove(game, req, kiosk, cap, pay, clock): hold check and royalty. Before confirm_request. */
export function addResaleProve(
  tx: Transaction,
  o: { game: string | undefined; req: TransactionArgument; kiosk: TransactionObjectArgument; cap: TransactionArgument; payFrom: TransactionObjectArgument },
) {
  if (!o.game) throw new Error("resale licence: the game id is needed to prove the resale rule");
  if (!RESALE.pkg) throw new Error("resale licence: Rare Store package not configured");
  tx.moveCall({ target: `${RESALE.pkg}::resale::prove`, arguments: [tx.object(o.game), o.req, o.kiosk, o.cap, o.payFrom, tx.object(CLOCK)] });
}

/** The builder's view of a policy. */
export type Policy = {
  policyId: string;
  nftType: string;
  rules: string[];
  /** Rules no builder knows how to prove: such policies cannot be traded through. */
  unknownRules: string[];
  royalty: { bps: number; min: bigint } | null;
  floor: bigint | null;
  sharedVersion: bigint | null;
};

// ---------- web/lib/chain/policy.ts: picking the policy from /v1/policies ----------

interface RuleEntry {
  id: string;
  module: string;
  packages: string[];
}

const MYSTEN = KIOSK_EXT_ALL;
const ROYALTY_COPIES = [
  "0x1f015c1a000631f2c0afd0258b299c63ffb8e5452e0c0f9be8fc28d2e20ad9b4",
  "0x93195daadbc4f26c0c498f4ceac92593682d2325ce3a0f5ba9f2db3b6a9733dd",
  "0x95bde20afc7cc60458d8360c87d4ed1f136dd0fee64828bb7b613aea2d98e9ff",
  "0xd441d82fa791d7e7fc89eb2a40b0714bd9a6a1aaf0c897d702802d30109c1f7b",
  "0xf74aa26054e5db492f93d88bce836466a799b70eae200bc254b5e3c9ed0cea8b",
  "0x05cf6401224b471a2489e0b13a0a23363378e89a0f14b6c58ffe9f71d1e36bc8",
  "0x34f9e32641795c57d936b3e387b9c7c941fd99b5a8a90e86b61e966e9da1d1f0",
  "0xd71803e11220094f86f3df6cd0c13655bf46f6d617f1bda40180f0e5b557bca3",
];
const LOCK_COPIES = [
  "0x1f015c1a000631f2c0afd0258b299c63ffb8e5452e0c0f9be8fc28d2e20ad9b4",
  "0x93195daadbc4f26c0c498f4ceac92593682d2325ce3a0f5ba9f2db3b6a9733dd",
  "0x95bde20afc7cc60458d8360c87d4ed1f136dd0fee64828bb7b613aea2d98e9ff",
  "0xf74aa26054e5db492f93d88bce836466a799b70eae200bc254b5e3c9ed0cea8b",
];

/** rules.json `rules`: every rule type (by defining package) the app can satisfy. */
const REGISTRY: RuleEntry[] = [
  { id: "royalty_rule", module: "royalty_rule", packages: MYSTEN },
  { id: "royalty_rule_copy", module: "royalty_rule", packages: ROYALTY_COPIES },
  { id: "kiosk_lock_rule", module: "kiosk_lock_rule", packages: MYSTEN },
  { id: "kiosk_lock_rule_copy", module: "kiosk_lock_rule", packages: LOCK_COPIES },
  { id: "personal_kiosk_rule", module: "personal_kiosk_rule", packages: MYSTEN },
  { id: "floor_price_rule", module: "floor_price_rule", packages: MYSTEN },
  {
    id: "staking_lock_rule",
    module: "staking_lock_rule",
    packages: [
      "0x3ab9fef3dcf219a23355fa0e3d4f7d9cdd421ddba4e3b2cb566543d4a10b4c38",
      "0x2e4b6da7ccda3acd22a89d07005f34857e38776f22a64183e819b2650fa3cb71",
      "0x515a552652d3d7cb4e219772ba78e17997bf7e2ffd9502a0275750574bfacdc8",
    ],
  },
];

const normPkg = (x: string) => `0x${x.toLowerCase().replace(/^0x/, "").padStart(64, "0")}`;
const samePkg = (a: string, b: string) => normPkg(a) === normPkg(b);
const isMysten = (pkg: string) => MYSTEN.some((v) => samePkg(v, pkg));

function splitRuleType(ruleType: string): { pkg: string; module: string; struct: string } | null {
  const [pkg, module, rest] = (ruleType || "").split("::");
  if (!pkg || !module) return null;
  return { pkg: normPkg(pkg), module, struct: (rest || "").split("<")[0] };
}

function recognise(r: PolicyRule): { entry: RuleEntry; config: Record<string, unknown> } | null {
  const parsed = splitRuleType(r.ruleType || "");
  if (parsed && RESALE.origin && parsed.module === "resale" && parsed.struct === "Rule" && samePkg(parsed.pkg, RESALE.origin)) {
    return { entry: { id: "resale_rule", module: "resale", packages: [] }, config: r.config ?? {} };
  }
  if (parsed) {
    const entry = REGISTRY.find((e) => e.module === parsed.module && (!parsed.struct || parsed.struct === "Rule") && e.packages.some((p) => samePkg(p, parsed.pkg)));
    return entry ? { entry, config: r.config ?? {} } : null;
  }
  // Payloads without a rule type: trust only the Mysten rule names.
  const entry = REGISTRY.find((e) => e.id === r.type && e.packages.some(isMysten));
  return entry ? { entry, config: r.config ?? {} } : null;
}

export interface PolicyShape {
  lock: boolean;
  personal: boolean;
  royalty: { bps: bigint; min: bigint; mysten: boolean } | null;
  floor: bigint | null;
  unsupported: string[];
}

function big(v: unknown, fallback = 0n): bigint {
  if (v === null || v === undefined || v === "") return fallback;
  try {
    return BigInt(typeof v === "number" ? Math.floor(v) : String(v));
  } catch {
    return fallback;
  }
}

export function readPolicy(policy: TransferPolicy | null | undefined): PolicyShape & { known: number } {
  const shape: PolicyShape & { known: number } = { lock: false, personal: false, royalty: null, floor: null, unsupported: [], known: 0 };
  for (const r of policy?.rules ?? []) {
    const known = recognise(r);
    if (!known) {
      shape.unsupported.push(r.ruleType || r.type || "unknown");
      continue;
    }
    shape.known++;
    const cfg = known.config;
    switch (known.entry.id) {
      case "royalty_rule":
      case "royalty_rule_copy": {
        const roy = { bps: big(cfg.amount_bp ?? cfg.amountBp), min: big(cfg.min_amount ?? cfg.minAmount), mysten: known.entry.id === "royalty_rule" };
        if (!shape.royalty || roy.mysten) shape.royalty = roy;
        break;
      }
      case "kiosk_lock_rule":
      case "kiosk_lock_rule_copy":
        shape.lock = true;
        break;
      case "personal_kiosk_rule":
        shape.personal = true;
        break;
      case "floor_price_rule":
        shape.floor = big(cfg.floor_price ?? cfg.floorPrice ?? cfg.price);
        break;
    }
  }
  return shape;
}

const canSatisfy = (p: TransferPolicy | null | undefined) => !!p && p.isShared !== false && readPolicy(p).unsupported.length === 0;

function cost(p: TransferPolicy): number {
  const s = readPolicy(p);
  return s.known * 10_000 + (s.personal ? 5_000 : 0) + (s.lock ? 1_000 : 0) + (s.royalty ? Number(s.royalty.bps) : 0);
}

/** The cheapest shared policy whose rules we can all resolve. */
export function choosePolicy(policies: TransferPolicy[] | null | undefined): TransferPolicy | null {
  return (
    (policies ?? [])
      .filter((p) => p && p.id && canSatisfy(p))
      .map((p, i) => ({ p, i }))
      .sort((a, b) => cost(a.p) - cost(b.p) || a.i - b.i)[0]?.p ?? null
  );
}
