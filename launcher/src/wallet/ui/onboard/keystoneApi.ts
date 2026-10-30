// The Keystone page's door to the keystone module (loaded on first use; it pulls in the UR libraries).

import type { RareAccount } from "../../index";
import type { KeystoneDevice } from "../../native";

export interface KeystoneAccountInfo {
  index: number;
  path: string;
  publicKey: string;
  address: string;
  mfp: string;
}

export interface KeystoneApi {
  list(): Promise<KeystoneDevice[]>;
  accounts(from: number, count: number): Promise<KeystoneAccountInfo[]>;
  add(index: number, label: string): Promise<RareAccount>;
  toError(e: unknown): { message: string; fix?: string };
}

export async function keystoneApi(): Promise<KeystoneApi> {
  const m = await import("../../keystone");
  return {
    list: m.listKeystones,
    accounts: (from, count) => m.keystoneAccounts(from, count),
    add: (index, label) => m.addKeystoneAccount(index, label),
    toError: (e) => {
      const k = m.toKeystoneError(e);
      return { message: k.message, fix: k.fix };
    },
  };
}
