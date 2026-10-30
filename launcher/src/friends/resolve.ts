// The add friend box: a username (with or without @), a SuiNS name or a 0x address, to an address.

import { resolveRecipient } from "../wallet/tx";
import { shown } from "./display";
import { lookup } from "./username";
import { parseTarget, usernameProblem } from "./username-rules";

export type Resolved = { ok: true; address: string; label: string; secondary: string | null } | { ok: false; msg: string };

export async function resolveTarget(raw: string): Promise<Resolved | null> {
  const t = parseTarget(raw);
  if (!t) return null;
  if (t.kind === "username") {
    const problem = usernameProblem(t.username);
    if (problem) return { ok: false, msg: problem };
    const u = await lookup(t.username);
    if (!u) return { ok: false, msg: "No player has that username." };
    const s = shown(u);
    return { ok: true, address: u.address, label: s.primary, secondary: s.secondary };
  }
  const r = await resolveRecipient(t.kind === "address" ? t.address : t.name);
  if (r.s === "ready") return { ok: true, address: r.address, label: r.suins ?? `${r.address.slice(0, 6)}...${r.address.slice(-4)}`, secondary: null };
  if (r.s === "invalid" || r.s === "missing" || r.s === "error") return { ok: false, msg: r.msg };
  return { ok: false, msg: "Could not find that player." };
}
