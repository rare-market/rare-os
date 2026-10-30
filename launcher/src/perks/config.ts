// Rarelings are OFF for this release and come at a separate launch: nothing here runs, no ownership
// check is made, no badge, label, theme or panel shows, and no fee discount applies, until the build
// sets VITE_RARELINGS=1. With it unset rarelingType() is null whatever localStorage or the env say.
//
// Which Move type is a Rareling. The rarelings package is not published yet: its id will come from
// the market repo's deploy manifests (market/deploy/contracts). Until a type is set every check
// answers "unavailable", so nothing mistakes "not deployed" for "not held".
//
// Set it, first match wins:
//   1. localStorage "rare.perks.rarelingType" (a full type, or a package id)
//   2. VITE_RARELING_TYPE at build time (same)
//   3. RARELING_PACKAGE below, once the manifest has it
// A Move type is always named by the package's ORIGINAL id, never an upgrade's.

import type { Network } from "../sui/chain";

/** The original package id of `rarelings`, from market/deploy/contracts once it is published. */
export const RARELING_PACKAGE: string | null = null;
export const RARELING_NETWORK: Network = "mainnet";
/** Where "Hold a Rareling to unlock" sends people. */
export const RARELINGS_URL = "https://rare.market/launchpad/rarelings";

const LS_KEY = "rare.perks.rarelingType";

/** "0xabc" -> "0x..::rareling::Rareling"; a full type stays as it is; anything else is null. */
export function asRarelingType(v: string | null | undefined): string | null {
  const s = v?.trim();
  if (!s) return null;
  if (/^0x[0-9a-fA-F]{1,64}$/.test(s)) return `${s.toLowerCase()}::rareling::Rareling`;
  if (/^0x[0-9a-fA-F]{1,64}::\w+::\w+$/.test(s)) return s;
  return null;
}

/**
 * The package id in a deploy manifest. Accepts the shapes a manifest is likely to take:
 * { packageId }, { rarelings: { packageId } }, { packages: { rarelings: { packageId | id } } },
 * or a list of { name: "rarelings", packageId }. `originalId` wins over `packageId` when present.
 */
export function packageFromManifest(m: unknown): string | null {
  const pick = (o: unknown): string | null => {
    if (!o || typeof o !== "object") return null;
    const r = o as Record<string, unknown>;
    for (const k of ["originalId", "originalPackageId", "packageId", "package", "id"]) if (typeof r[k] === "string" && /^0x[0-9a-fA-F]+$/.test(r[k] as string)) return r[k] as string;
    return null;
  };
  if (Array.isArray(m)) return pick(m.find((x) => x && typeof x === "object" && /rareling/i.test(String((x as { name?: unknown }).name ?? ""))));
  if (!m || typeof m !== "object") return null;
  const r = m as Record<string, unknown>;
  const pkgs = r.packages && typeof r.packages === "object" ? (r.packages as Record<string, unknown>) : null;
  return pick(r.rarelings) ?? pick(pkgs?.rarelings) ?? (Array.isArray(r.packages) ? packageFromManifest(r.packages) : null) ?? pick(r);
}

/** The one Rarelings switch for the launcher (build flag VITE_RARELINGS=1). Read at call time so tests can stub it. */
export const rarelingsOn = (): boolean => import.meta.env.VITE_RARELINGS === "1";

export function rarelingType(): string | null {
  if (!rarelingsOn()) return null;
  let local: string | null = null;
  try {
    local = localStorage.getItem(LS_KEY);
  } catch {
    local = null;
  }
  const env = (import.meta as { env?: Record<string, string | undefined> }).env?.VITE_RARELING_TYPE;
  return asRarelingType(local) ?? asRarelingType(env) ?? asRarelingType(RARELING_PACKAGE);
}

/** Sets the type on this device (dev, or before a build carries it). null clears it. */
export function setRarelingType(v: string | null) {
  try {
    if (v) localStorage.setItem(LS_KEY, v);
    else localStorage.removeItem(LS_KEY);
  } catch {
    /* nothing to keep it in */
  }
}
