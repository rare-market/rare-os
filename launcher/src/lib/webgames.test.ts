import { describe, expect, it } from "vitest";

// webgames.ts pulls in the app store; the address check is pure, so stub what it imports.
import { vi } from "vitest";
vi.mock("./bridge", () => ({ native: {} }));
vi.mock("./store", () => ({ useApp: { getState: () => ({ scan: async () => {} }) } }));

import { normaliseUrl } from "./webgames";

describe("web game addresses", () => {
  it("accepts https anywhere and adds it when the scheme is missing", () => {
    expect(normaliseUrl("https://play.example.com/game")).toBe("https://play.example.com/game");
    expect(normaliseUrl("play.example.com")).toBe("https://play.example.com/");
  });

  it("accepts http on localhost, 127.0.0.1 and [::1] with any port", () => {
    expect(normaliseUrl("http://localhost")).toBe("http://localhost/");
    expect(normaliseUrl("http://localhost:3000/play")).toBe("http://localhost:3000/play");
    expect(normaliseUrl("http://127.0.0.1:8080")).toBe("http://127.0.0.1:8080/");
    expect(normaliseUrl("http://[::1]:5173/")).toBe("http://[::1]:5173/");
    expect(normaliseUrl("HTTP://LOCALHOST:9000")).toBe("http://localhost:9000/");
  });

  it("reads a bare local address as http", () => {
    expect(normaliseUrl("localhost:3000")).toBe("http://localhost:3000/");
    expect(normaliseUrl("127.0.0.1:4000/x")).toBe("http://127.0.0.1:4000/x");
    expect(normaliseUrl("[::1]:4000")).toBe("http://[::1]:4000/");
  });

  it("keeps everything else https only", () => {
    expect(() => normaliseUrl("http://example.com")).toThrow(/https/);
    expect(() => normaliseUrl("http://localhost.example.com")).toThrow(/https/);
    expect(() => normaliseUrl("http://localhost@example.com")).toThrow(/https/);
    expect(() => normaliseUrl("http://192.168.1.5:3000")).toThrow(/https/);
    expect(() => normaliseUrl("http://127.0.0.2")).toThrow(/https/);
    expect(() => normaliseUrl("http://0.0.0.0:3000")).toThrow(/https/);
    expect(() => normaliseUrl("ftp://localhost")).toThrow();
    expect(() => normaliseUrl("javascript:alert(1)")).toThrow();
  });

  it("still allows https on a local host", () => {
    expect(normaliseUrl("https://localhost:3000")).toBe("https://localhost:3000/");
  });
});
