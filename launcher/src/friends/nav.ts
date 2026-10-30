// Opening Friends at a person or a party (notices, Home's "Friends playing" row, the game page).

import { create } from "zustand";
import { useApp } from "../lib/store";

export type FriendsTarget = { kind: "friend"; address: string } | { kind: "party"; id: string } | { kind: "add" } | null;

/** What the Friends screen should show next; the screen clears it once shown. */
export const useFriendsNav = create<{ target: FriendsTarget; seq: number }>(() => ({ target: null, seq: 0 }));

export function openFriends(target: FriendsTarget = null) {
  const app = useApp.getState();
  app.close();
  useFriendsNav.setState((s) => ({ target, seq: s.seq + 1 }));
  app.setTab("friends");
}
