import { describe, expect, it } from "vitest";
import { normalizeUsername, parseTarget, usernameProblem } from "./username-rules";

describe("username rules", () => {
  it("normalizes", () => {
    expect(normalizeUsername("  @Mira_K ")).toBe("mira_k");
  });
  it("accepts good names", () => {
    for (const s of ["mira", "a.b", "dave_99", "x".repeat(20)]) expect(usernameProblem(s)).toBeNull();
  });
  it("explains broken ones", () => {
    expect(usernameProblem("ab")).toMatch(/3 to 20/);
    expect(usernameProblem("x".repeat(21))).toMatch(/3 to 20/);
    expect(usernameProblem("da ve")).toMatch(/letters, numbers/);
    expect(usernameProblem(".dave")).toMatch(/Start and end/);
    expect(usernameProblem("dave_")).toMatch(/Start and end/);
    expect(usernameProblem("da..ve")).toMatch(/two/);
    expect(usernameProblem("")).toBeNull();
  });
  it("tells what the add box holds", () => {
    expect(parseTarget("@Mira")).toEqual({ kind: "username", username: "mira" });
    expect(parseTarget("mira")).toEqual({ kind: "username", username: "mira" });
    expect(parseTarget("Mira.sui")).toEqual({ kind: "suins", name: "mira.sui" });
    expect(parseTarget("pay@alice")).toEqual({ kind: "suins", name: "pay@alice" });
    expect(parseTarget("0xAB12")).toEqual({ kind: "address", address: "0xab12" });
    expect(parseTarget("  ")).toBeNull();
  });
});
