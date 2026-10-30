// A release bundle must hold no mock, demo or sample code (the real app never shows them; see lib/mockflag.ts).
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

describe("release bundle", () => {
  it("builds without mock or demo content", () => {
    const out = mkdtempSync(join(tmpdir(), "rare-bundle-"));
    try {
      const env = { ...process.env, VITE_MOCK: "" };
      execFileSync("npx", ["vite", "build", "--outDir", out, "--emptyOutDir"], { env, stdio: "pipe" });
      const r = execFileSync("node", ["scripts/check-no-demo.mjs", out], { encoding: "utf8" });
      expect(r).toContain("free of mock and demo content");
    } finally {
      rmSync(out, { recursive: true, force: true });
    }
  }, 120_000);
});
