import { describe, expect, it } from "vitest";
import { cleanTyped, padPress } from "./padKeys";

const type = (keys: string, d = 9) => [...keys].reduce((v, k) => padPress(v, k === "<" ? "back" : k, d), "");

describe("amount pad", () => {
  it("types digits and one dot", () => {
    expect(type("12.5")).toBe("12.5");
    expect(type("1.2.3")).toBe("1.23");
  });
  it("starts a fraction with a zero", () => {
    expect(type(".5")).toBe("0.5");
  });
  it("drops leading zeros", () => {
    expect(type("007")).toBe("7");
    expect(type("0.05")).toBe("0.05");
  });
  it("stops at the coin's decimals", () => {
    expect(type("0.1234567", 4)).toBe("0.1234");
    expect(type("1.5", 0)).toBe("15");
  });
  it("deletes", () => {
    expect(type("1.5<<")).toBe("1");
    expect(type("<")).toBe("");
  });
  it("cleans typed text", () => {
    expect(cleanTyped("1,5abc", 9)).toBe("1.5");
  });
});
