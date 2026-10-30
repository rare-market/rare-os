// In-memory VaultPort for unit tests: same contract and error codes as the Rust vault, no crypto.
import type { SecretKind, VaultPort, VaultStatus } from "../native";
import { NativeError } from "../native";

export function memoryVault(): VaultPort {
  let st: { secret: string; kind: SecretKind; payload: string; failures: number } | null = null;
  let unlocked = false;
  const status = async (): Promise<VaultStatus> => ({
    exists: !!st,
    unlocked: !!st && unlocked,
    backend: st ? "file" : null,
    kind: st?.kind ?? null,
    failures: st?.failures ?? 0,
    retryAfterMs: 0,
  });
  const check = (secret: string) => {
    if (!st) throw new NativeError("missing", "No wallet");
    if (secret !== st.secret) {
      st.failures++;
      throw new NativeError("wrong", "Wrong PIN", { failures: st.failures, retryAfterMs: 0 });
    }
    st.failures = 0;
    return st;
  };
  return {
    status,
    async create(secret, kind, payload) {
      if (st) throw new NativeError("exists", "exists");
      st = { secret, kind, payload, failures: 0 };
      unlocked = true;
      return status();
    },
    async unlock(secret) {
      const s = check(secret);
      unlocked = true;
      return s.payload;
    },
    async read() {
      return st && unlocked ? st.payload : null;
    },
    async save(payload) {
      if (!st || !unlocked) throw new NativeError("locked", "locked");
      st.payload = payload;
    },
    async lock() {
      unlocked = false;
    },
    async verify(secret) {
      return check(secret).payload;
    },
    async changeSecret(current, next, kind) {
      const s = check(current);
      s.secret = next;
      s.kind = kind;
    },
    async reset() {
      st = null;
      unlocked = false;
    },
  };
}
