import { describe, expect, it } from "vitest";
import { MAX_FOLLOWS, followKey, isFollowing, readFollows, toggleFollow, writeFollows, type KV } from "./follow";

const A = `0x${"a".repeat(64)}`;
const B = `0x${"B".repeat(64)}`;
const mem = (): KV & { m: Map<string, string> } => {
  const m = new Map<string, string>();
  return { m, getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
};

describe("follows", () => {
  it("toggles, newest first, addresses lower case", () => {
    let l = toggleFollow([], { address: A, name: "a" }, 1);
    l = toggleFollow(l, { address: B, name: "b" }, 2);
    expect(l.map((f) => f.address)).toEqual([B.toLowerCase(), A]);
    expect(isFollowing(l, B)).toBe(true);
    l = toggleFollow(l, { address: B.toLowerCase(), name: "b" });
    expect(l.map((f) => f.address)).toEqual([A]);
  });

  it("caps the list", () => {
    let l = [] as ReturnType<typeof toggleFollow>;
    for (let i = 0; i < MAX_FOLLOWS + 5; i++) l = toggleFollow(l, { address: `0x${i.toString(16).padStart(64, "0")}`, name: String(i) });
    expect(l).toHaveLength(MAX_FOLLOWS);
    expect(l[0].name).toBe(String(MAX_FOLLOWS + 4));
  });

  it("keeps one list per account on the device", () => {
    const kv = mem();
    writeFollows(A, [{ address: B.toLowerCase(), name: "b", since: 1 }], kv);
    writeFollows(null, [{ address: A, name: "a", since: 1 }], kv);
    expect(readFollows(A.toUpperCase().replace("0X", "0x"), kv).map((f) => f.name)).toEqual(["b"]);
    expect(readFollows(null, kv).map((f) => f.name)).toEqual(["a"]);
    expect(readFollows(B, kv)).toEqual([]);
    expect(followKey(null)).toBe("rare.traders.follow.device");
  });

  it("ignores junk in storage", () => {
    const kv = mem();
    kv.setItem(followKey(A), JSON.stringify([{ address: "nope" }, null, { address: A, name: "ok", since: 0 }]));
    expect(readFollows(A, kv).map((f) => f.name)).toEqual(["ok"]);
    kv.setItem(followKey(A), "{broken");
    expect(readFollows(A, kv)).toEqual([]);
  });
});
