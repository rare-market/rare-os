// Browser-only conveniences for UI development and screenshots (never active in Tauri):
//   ?watch=0x...   adds a watch account named "Demo wallet" on first load
//   ?devsign       lets that watch account go through send and review with a fake signature;
//                  everything stays a dry run (simulated, never submitted)
import { isTauri } from "../native";
import type { RareAccount } from "../index";

const q = typeof location !== "undefined" ? new URLSearchParams(location.search) : new URLSearchParams();

export const devWatch = !isTauri ? q.get("watch") : null;
export const devSign = !isTauri && q.has("devsign");

/** Whether the UI should offer signing actions (Send, Approve) for this account. */
export const canSignAccount = (a: RareAccount | null | undefined) => !!a && (a.kind !== "watch" || devSign);
