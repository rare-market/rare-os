import { describe, expect, it } from "vitest";
import { CLASS_RULES, DEFAULT_PREFS, PlayQueue, WakeSummary, classOf, deliveryFor, cmpId, isFresh, maxId, mergeNotices, newIds, percentile, pushSample, readPrefs, route, streamQuery, unreadCount, visible, type Prefs } from "./logic";
import type { Notice, NoticeKind } from "./types";

const now = 1_800_000_000_000;
const n = (id: string, kind: NoticeKind = "sold", age = 0): Notice => ({
  id,
  kind,
  addr: "0x1",
  title: `Notice ${id}`,
  body: null,
  imageUrl: null,
  amount: null,
  item: null,
  action: { kind: "tx", digest: "abc" },
  digest: "abc",
  cpTs: now - age - 300,
  createdAt: now - age,
  sentAt: now - age,
});

describe("ids", () => {
  it("compares decimal strings past 2^53", () => {
    expect(cmpId("9", "10")).toBe(-1);
    expect(cmpId("9007199254740993", "9007199254740992")).toBe(1);
    expect(cmpId("0012", "12")).toBe(0);
  });
  it("keeps the larger id for resume and ignores junk", () => {
    expect(maxId(null, "5")).toBe("5");
    expect(maxId("12", "9")).toBe("12");
    expect(maxId("12", "abc")).toBe("12");
    expect(maxId(undefined, undefined)).toBeNull();
  });
});

describe("dedupe", () => {
  it("merges newest first with one copy per id", () => {
    const a = mergeNotices([n("3"), n("1")], [n("2"), n("3"), n("4")]);
    expect(a.map((x) => x.id)).toEqual(["4", "3", "2", "1"]);
  });
  it("keeps the first copy of an id", () => {
    const first = { ...n("7"), title: "first" };
    expect(mergeNotices([first], [{ ...n("7"), title: "second" }])[0].title).toBe("first");
  });
  it("caps the list", () => {
    const many = Array.from({ length: 30 }, (_, i) => n(String(i + 1)));
    expect(mergeNotices([], many, 10).map((x) => x.id)[0]).toBe("30");
    expect(mergeNotices([], many, 10)).toHaveLength(10);
  });
  it("orders server and local notices by time, not id", () => {
    // A local id is a long decimal; it must not jump ahead of a newer server notice.
    const local = { ...n("1793000000000001", "followed_trade", 5000) };
    expect(mergeNotices([local], [n("42", "sold", 0), n("41", "sold", 9000)]).map((x) => x.id)).toEqual(["42", "1793000000000001", "41"]);
  });
  it("reports only ids it has not seen", () => {
    expect([...newIds([n("1"), n("2")], [n("2"), n("3")])]).toEqual(["3"]);
  });
});

describe("stream query", () => {
  it("dedupes and lowercases addresses, adds device and after", () => {
    const q = new URLSearchParams(streamQuery(["0xAB", "0xab", "nope", "0x01"], "dev_1234567890abcdef", "42"));
    expect(q.get("addr")).toBe("0x01,0xab");
    expect(q.get("device")).toBe("dev_1234567890abcdef");
    expect(q.get("after")).toBe("42");
  });
  it("leaves out after when there is no id yet", () => {
    expect(new URLSearchParams(streamQuery(["0x1"], "d", null)).has("after")).toBe(false);
  });
});

describe("kind toggles", () => {
  const prefs: Prefs = { ...DEFAULT_PREFS, kinds: { ...DEFAULT_PREFS.kinds, offer_received: false } };
  it("drops a switched-off kind", () => {
    expect(route(n("1", "offer_received"), { prefs, live: true, now, playing: false, focused: true }).route).toBe("drop");
    expect(route(n("1", "sold"), { prefs, live: true, now, playing: false, focused: true }).route).toBe("toast");
  });
  it("hides switched-off kinds and cleared notices from the centre", () => {
    const list = [n("5", "sold", 0), n("4", "offer_received", 1000), n("3", "bought", 2000), n("2", "sold", 3000)];
    expect(visible(list, prefs, now - 3000).map((x) => x.id)).toEqual(["5", "3"]);
  });
  it("counts unread from the read set and the read mark", () => {
    const list = [n("5", "sold", 0), n("4", "sold", 1000), n("3", "sold", 2000), n("2", "sold", 3000)];
    expect(unreadCount(list, DEFAULT_PREFS, null, new Set(["5"]), now - 2000)).toBe(1);
  });
  it("reads stored prefs with defaults for missing or bad values", () => {
    const p = readPrefs({ kinds: { sold: false, bought: "yes" }, desktop: false });
    expect(p.kinds.sold).toBe(false);
    expect(p.kinds.bought).toBe(true);
    expect(p.desktop).toBe(false);
    expect(p.gameToasts).toBe(true);
    expect(readPrefs(null)).toEqual(DEFAULT_PREFS);
  });
});

describe("routing and the play queue", () => {
  const ctx = { prefs: DEFAULT_PREFS, live: true, now, playing: false, focused: true };
  const out = (r: ReturnType<typeof route>) => ({ route: r.route, desktop: r.desktop, hud: r.hud, small: r.small });
  it("toasts a fresh live notice, desktop only when unfocused", () => {
    expect(out(route(n("1"), ctx))).toEqual({ route: "toast", desktop: false, hud: false, small: false });
    expect(route(n("1"), { ...ctx, focused: false }).desktop).toBe(true);
    expect(route(n("1"), { ...ctx, focused: false, prefs: { ...DEFAULT_PREFS, desktop: false } }).desktop).toBe(false);
  });
  it("sends old and replayed notices to the centre only", () => {
    expect(isFresh(n("1", "sold", 10 * 60_000), now)).toBe(false);
    expect(route(n("1", "sold", 10 * 60_000), ctx).route).toBe("centre");
    expect(route(n("1"), { ...ctx, live: false }).route).toBe("centre");
  });
  it("drops a switched-off kind", () => {
    expect(route(n("1"), { ...ctx, prefs: { ...DEFAULT_PREFS, kinds: { ...DEFAULT_PREFS.kinds, sold: false } } }).route).toBe("drop");
  });
  it("puts every kind in a class", () => {
    expect(classOf("sold")).toBe("money");
    expect(classOf("offer_received")).toBe("money");
    expect(classOf("party_invite")).toBe("invites");
    expect(classOf("friend_online")).toBe("social");
    expect(classOf("os_update")).toBe("system");
    expect(classOf("download_done")).toBe("system");
    expect(classOf("emu_update")).toBe("system");
    expect(classOf("trophy")).toBe("trophies");
    expect(classOf("battery")).toBe("battery");
    expect(classOf("something_new")).toBe("social");
  });
  it("follows the model's table over a game", () => {
    const g = { ...ctx, playing: true, focused: false };
    expect(out(route(n("1", "sold"), g))).toEqual({ route: "toast", desktop: false, hud: true, small: false });
    expect(out(route(n("1", "party_invite"), g)).route).toBe("toast");
    expect(out(route(n("1", "trophy"), g))).toEqual({ route: "toast", desktop: false, hud: true, small: true });
    expect(route(n("1", "friend_online"), g).route).toBe("queue");
    expect(route(n("1", "download_done"), g).route).toBe("queue");
    expect(route(n("1", "battery"), g).route).toBe("toast");
  });
  it("queues over a game when toasts over a game are off, except Battery", () => {
    const g = { ...ctx, playing: true, prefs: { ...DEFAULT_PREFS, gameToasts: false } };
    expect(route(n("1", "sold"), g).route).toBe("queue");
    expect(route(n("1", "battery"), g).route).toBe("toast");
  });
  it("Do not disturb keeps everything in the list but Battery", () => {
    const d = { ...ctx, prefs: { ...DEFAULT_PREFS, dnd: true } };
    for (const k of ["sold", "party_invite", "trophy", "friend_request", "os_update", "download_done"] as const) expect(route(n("1", k), d).route).toBe("centre");
    expect(route(n("1", "battery"), d).route).toBe("toast");
    expect(route(n("1", "battery"), { ...d, playing: true }).route).toBe("toast");
    expect(deliveryFor("system", { dnd: true, playing: false, gameToasts: true })).toBe(CLASS_RULES.system.dnd);
  });
  it("summarises what arrived while asleep", () => {
    const w = new WakeSummary();
    expect(w.flush()).toBeNull();
    w.add("1");
    w.add("1");
    w.add("2");
    expect(w.flush()).toBe("2 new while you were away");
    expect(w.count).toBe(0);
  });
  it("counts queued notices once each and summarises on exit", () => {
    const q = new PlayQueue();
    expect(q.flush()).toBeNull();
    q.add("1");
    q.add("1");
    expect(q.count).toBe(1);
    expect(q.flush()).toBe("1 new notification while you played");
    q.add("2");
    q.add("3");
    expect(q.flush()).toBe("2 new notifications while you played");
    expect(q.count).toBe(0);
  });
});

describe("latency samples", () => {
  it("keeps the last 50", () => {
    let l: number[] = [];
    for (let i = 0; i < 60; i++) l = pushSample(l, i);
    expect(l).toHaveLength(50);
    expect(l[0]).toBe(10);
  });
  it("finds percentiles", () => {
    expect(percentile([300, 100, 200, 400], 50)).toBe(200);
    expect(percentile([300, 100, 200, 400], 95)).toBe(400);
    expect(percentile([], 50)).toBeNull();
  });
});
