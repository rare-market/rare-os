import { describe, expect, it } from "vitest";
import { liveLaunches } from "./data";
import { addSnapshot, changeSince } from "./live";

const H = 3600_000;

describe("portfolio snapshots", () => {
  it("keeps one snapshot per 10 minutes and two days at most", () => {
    let list = addSnapshot([], { t: 0, usd: 1 });
    list = addSnapshot(list, { t: 5 * 60_000, usd: 2 });
    expect(list).toHaveLength(1);
    list = addSnapshot(list, { t: 11 * 60_000, usd: 3 });
    expect(list).toHaveLength(2);
    list = addSnapshot(list, { t: 49 * H, usd: 4 });
    expect(list.map((s) => s.usd)).toEqual([4]);
  });
  it("compares against the snapshot nearest 24 h ago", () => {
    const list = [
      { t: 0, usd: 100 },
      { t: 23 * H, usd: 80 },
      { t: 40 * H, usd: 90 },
    ];
    const c = changeSince(list, { t: 47 * H, usd: 120 })!;
    expect(c.since).toBe(23 * H);
    expect(c.usd).toBe(40);
    expect(c.pct).toBe(50);
  });
  it("needs half an hour of history", () => {
    expect(changeSince([{ t: 0, usd: 1 }], { t: 10 * 60_000, usd: 2 })).toBeNull();
  });
});

describe("liveLaunches", () => {
  const now = 1_800_000_000_000;
  it("keeps launches inside a phase window and not sold out", () => {
    const list = [
      { slug: "a", name: "A", phases: [{ startsAt: now - 1000, endsAt: now + 1000 }] },
      { slug: "b", name: "B", phases: [{ startsAt: now + 1000, endsAt: now + 2000 }] },
      { slug: "c", name: "C", minted: 10, supply: 10, startsAt: now - 1000 },
      { slug: "d", name: "D", startsAt: new Date(now - 1000).toISOString() },
      { slug: "e", name: "E" },
    ];
    expect(liveLaunches(list, now).map((l) => l.slug)).toEqual(["a", "d"]);
  });
});
