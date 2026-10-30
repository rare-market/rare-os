import { describe, expect, it } from "vitest";
import { shown, nameOf } from "./display";

const A = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

describe("display rule", () => {
  it("username leads, SuiNS is the quiet second line", () => {
    expect(shown({ address: A, name: "mira", username: "mira", suins: "mira.sui" })).toEqual({ primary: "mira", secondary: "mira.sui", kind: "username" });
  });
  it("a username alone has no second line", () => {
    expect(shown({ address: A, username: "mira", suins: null })).toEqual({ primary: "mira", secondary: null, kind: "username" });
  });
  it("SuiNS alone leads when there is no username", () => {
    expect(shown({ address: A, username: null, suins: "mira.sui", name: "mira.sui" })).toEqual({ primary: "mira.sui", secondary: null, kind: "suins" });
  });
  it("falls back to a short address", () => {
    expect(shown({ address: A })).toEqual({ primary: "0xa049...887e", secondary: null, kind: "address" });
    expect(nameOf(null)).toBe("Someone");
  });
  it("answers that only carry name still work", () => {
    expect(shown({ address: A, name: "mira" }).kind).toBe("username");
    expect(shown({ address: A, name: "mira.sui" }).kind).toBe("suins");
  });
});
