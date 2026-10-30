import { describe, expect, it } from "vitest";
import { cleanEndpoint } from "./DeveloperSection";

describe("developer endpoints", () => {
  it("clears on blank, trims the slash, refuses unsafe", () => {
    expect(cleanEndpoint("  ")).toBeNull();
    expect(cleanEndpoint("https://t.example.com/")).toBe("https://t.example.com");
    expect(cleanEndpoint("http://localhost:3001")).toBe("http://localhost:3001");
    expect(cleanEndpoint("http://example.com")).toBeUndefined();
  });
});
