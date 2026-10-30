// Rare Wallet service for the launcher: accounts from the forked keystore, balances from
// Sui gRPC (gRPC-web against the public fullnode), and
// collectibles from Rare Marketplace.

import { isValidSuiAddress, normalizeSuiAddress } from "@mysten/sui/utils";
import { grpcClient } from "./grpc";
import { Keystore } from "./core/keys/keystore";
import { LocalKV } from "./core/store/kv";
import { call, isTauri, vault } from "./native";
import { registerHardwareSigners } from "./hw";
import type { RareAccount } from "./core/types";

export type { RareAccount };

const grpc = grpcClient;

/** The keystore's account list key (core/keys/keystore.ts K_META). */
const ACCOUNTS = "rare.meta";

/**
 * localStorage, with the account list mirrored to Rust (`<data dir>/rare/wallet/accounts.json`)
 * so the native launcher's keystore sees the same accounts. Public data only; secrets stay in the
 * vault. Mirror failures never block the wallet.
 */
class MirroredKV extends LocalKV {
  async get<T>(key: string): Promise<T | undefined> {
    const v = await super.get<T>(key);
    if (v !== undefined || key !== ACCOUNTS || !isTauri) return v;
    // a fresh webview profile (or one the native launcher wrote first): take Rust's copy
    const raw = await call<string | null>("wallet_accounts_read").catch(() => null);
    if (!raw) return undefined;
    try {
      const m = JSON.parse(raw) as T;
      await super.set(key, m);
      return m;
    } catch {
      return undefined;
    }
  }
  async set<T>(key: string, value: T): Promise<void> {
    await super.set(key, value);
    if (key === ACCOUNTS && isTauri) await call("wallet_accounts_write", { json: JSON.stringify(value) }).catch(() => {});
  }
  async remove(key: string): Promise<void> {
    await super.remove(key);
    if (key === ACCOUNTS && isTauri) await call("wallet_accounts_clear").catch(() => {});
  }
}

const disk = new MirroredKV("rare.wallet.");
export const keystore = new Keystore({ disk, vault });

// wallets made before the mirror existed: copy the list over once per start
if (isTauri)
  void disk.get(ACCOUNTS).then((m) => (m ? call("wallet_accounts_write", { json: JSON.stringify(m) }).catch(() => {}) : undefined));
registerHardwareSigners(keystore);

export { grpcClient };
// Balances moved to data/balances.ts; re-exported so existing imports keep working.
export { balances, portfolio, type Balance, type Portfolio, type PortfolioToken } from "./data/balances";

/** Accepts 0x addresses and SuiNS names (name.sui or @name). */
export async function resolveAddress(input: string): Promise<string> {
  const v = input.trim();
  if (isValidSuiAddress(normalizeSuiAddress(v)) && /^0x[0-9a-fA-F]{1,64}$/.test(v)) return normalizeSuiAddress(v);
  if (/^@?[a-z0-9-]+(\.[a-z0-9-]+)*(\.sui)?$/i.test(v)) {
    const name = v.startsWith("@") || v.endsWith(".sui") ? v : `${v}.sui`;
    const r = await grpc.core.resolveNameServiceAddress({ name });
    if (r.address) return r.address;
    throw new Error(`No address for ${name}`);
  }
  throw new Error("Enter a Sui address (0x...) or a SuiNS name");
}

export async function watch(input: string, label?: string): Promise<RareAccount> {
  const address = await resolveAddress(input);
  const acc = await keystore.addWatch(address, label ?? (input.startsWith("0x") ? undefined : input));
  await keystore.select(acc.address);
  return acc;
}

export const shortAddr = (a: string) => `${a.slice(0, 6)}...${a.slice(-4)}`;
