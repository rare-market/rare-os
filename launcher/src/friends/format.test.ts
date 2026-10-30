import { describe, expect, it } from "vitest";
import { presenceText } from "./format";

const NOW = 1_800_000_000_000;
const act = { id: "web:act", title: "ACT", kind: "web" as const };

describe("presence in words", () => {
  it("says what a friend is doing", () => {
    expect(presenceText({ state: "online", game: act, since: NOW - 42 * 60_000, lastSeen: NOW }, false, NOW)).toBe("Playing ACT");
    expect(presenceText({ state: "online", game: act, since: NOW - 42 * 60_000, lastSeen: NOW }, true, NOW)).toBe("Playing ACT for 42 min");
    expect(presenceText({ state: "online", game: act, since: NOW - 75 * 60_000, lastSeen: NOW }, true, NOW)).toBe("Playing ACT for 1 h 15 min");
    expect(presenceText({ state: "online", game: null, since: NOW, lastSeen: NOW }, false, NOW)).toBe("Online");
    expect(presenceText({ state: "away", game: null, since: NOW, lastSeen: NOW }, false, NOW)).toBe("Away");
  });
  it("says when an offline friend was last seen, if they share it", () => {
    expect(presenceText({ state: "offline", game: null, since: null, lastSeen: NOW - 3 * 3_600_000 }, false, NOW)).toBe("Last seen 3 h ago");
    expect(presenceText({ state: "offline", game: null, since: null, lastSeen: NOW - 30 * 86_400_000 }, false, NOW)).toBe("Last seen 30 days ago");
    expect(presenceText({ state: "offline", game: null, since: null, lastSeen: null }, false, NOW)).toBe("Offline");
  });
});
