import { beforeEach, describe, expect, it } from "vitest";
import { activeParty, apply, partyInvites, useFriends } from "./store";
import type { Friend, Party } from "./types";

const ME = "0x" + "a".repeat(64);
const MIRA = "0x" + "b".repeat(64);
const KENJI = "0x" + "c".repeat(64);
const off = { state: "offline" as const, game: null, since: null, lastSeen: 1 };
const friend = (address: string, name: string): Friend => ({ address, name, avatarUrl: null, since: 1, presence: off });
const party = (members: Party["members"]): Party => ({ id: "party0001", host: MIRA, game: null, createdAt: 1, members });

describe("friend events from the stream", () => {
  beforeEach(() => {
    useFriends.setState({ status: "ready", address: ME, friends: [friend(MIRA, "mira.sui"), friend(KENJI, "kenji.sui")], parties: [], messages: {}, unread: {}, openParty: null });
  });

  it("a friend coming online is news once; a heartbeat while online is not", async () => {
    const on = { state: "online" as const, game: { id: "web:act", title: "ACT", kind: "web" as const }, since: 2, lastSeen: 2 };
    const r = await apply({ event: "presence", data: { address: MIRA, presence: on } });
    expect(r).toMatchObject({ kind: "online", game: { id: "web:act" } });
    expect(useFriends.getState().friends[0].presence.state).toBe("online");
    expect(await apply({ event: "presence", data: { address: MIRA, presence: { ...on, game: null } } })).toBeNull();
  });

  it("party invites land in the list and are told once", async () => {
    const p = party([
      { address: MIRA, name: "mira.sui", avatarUrl: null, state: "joined", presence: off },
      { address: ME, name: null, avatarUrl: null, state: "invited", presence: off },
    ]);
    const r = await apply({ event: "party", data: { change: "invite", party: p, from: MIRA } });
    expect(r).toMatchObject({ kind: "invite", from: MIRA });
    expect(partyInvites(useFriends.getState())).toHaveLength(1);
    expect(activeParty(useFriends.getState())).toBeNull();
    await apply({ event: "party", data: { change: "closed", id: p.id } });
    expect(useFriends.getState().parties).toHaveLength(0);
  });

  it("chat counts unread unless that chat is open, and ignores repeats and my own lines", async () => {
    const m = { id: "1", partyId: "party0001", from: MIRA, text: "ready?", at: 3 };
    expect(await apply({ event: "chat", data: m })).toMatchObject({ kind: "message" });
    expect(await apply({ event: "chat", data: m })).toBeNull();
    expect(useFriends.getState().unread.party0001).toBe(1);
    useFriends.setState({ openParty: "party0001" });
    expect(await apply({ event: "chat", data: { ...m, id: "2" } })).toBeNull();
    expect(await apply({ event: "chat", data: { ...m, id: "3", from: ME } })).toBeNull();
    expect(useFriends.getState().messages.party0001).toHaveLength(3);
  });

  it("nothing applies before friends are ready", async () => {
    useFriends.setState({ status: "unlinked" });
    expect(await apply({ event: "chat", data: { id: "9", partyId: "x", from: MIRA, text: "hi", at: 1 } })).toBeNull();
  });
});
