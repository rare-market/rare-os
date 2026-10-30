// The username picker: the on-screen keyboard with a live line under the text (rules first, then
// the server's answer for that name), then one wallet signature or the linked device key.

import { askText, type OskStatus } from "../../components/Osk";
import { useApp } from "../../lib/store";
import { currentAddress, useProfile } from "../../sui/profile";
import { SignRejected } from "../../wallet/sign";
import { check, claim, type Mine } from "../username";
import { normalizeUsername, usernameProblem } from "../username-rules";

async function statusFor(raw: string, address: string | undefined, current: string | null): Promise<OskStatus | null> {
  const u = normalizeUsername(raw);
  if (!u) return null;
  const problem = usernameProblem(u);
  if (problem) return { text: problem, ok: false };
  if (u === current) return { text: "That is your username.", ok: false };
  try {
    const a = await check(u, address);
    if (a.available) return { text: `${u} is free`, ok: true };
    return { text: a.message ?? "That name is not available.", ok: false };
  } catch {
    return { text: "Could not check the name. Try again.", ok: false };
  }
}

/** Opens the picker; resolves with the saved name, or null when cancelled or it failed (a toast says why). */
export async function pickUsername(): Promise<Mine | null> {
  const cur = await currentAddress();
  if (!cur) {
    useApp.getState().toast("Add a Rare Wallet account first", "error");
    return null;
  }
  const current = useProfile.getState().username;
  const raw = await askText("Pick a username", current ?? "", {
    placeholder: "3 to 20 letters, numbers, _ or .",
    status: (v) => statusFor(v, cur.address.toLowerCase(), current),
  });
  if (!raw?.trim()) return null;
  try {
    const m = await claim(raw);
    useApp.getState().toast(`Your username is ${m.username}`);
    return m;
  } catch (e) {
    if (!(e instanceof SignRejected)) useApp.getState().toast(e instanceof Error ? e.message : "Could not save the username", "error");
    return null;
  }
}
