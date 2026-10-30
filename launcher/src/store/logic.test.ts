import { describe, expect, it } from "vitest";
import { onSale, pickBuild, priceNow, refundable, steamAppId, tradableNow, updateDecision, usdText, withinGrace, LAUNCH_GRACE_MS } from "./logic";
import { normalizeConfig, canSell, canResell } from "./api";
import type { ChainRelease } from "./types";

const release = (version: number, edition = 1): ChainRelease => ({
  version,
  label: `1.${version}`,
  edition,
  publishedMs: 0,
  builds: [
    { platform: "windows-x86_64", sha256: "aa".repeat(32), size: 10n, path: "w" },
    { platform: "linux-x86_64", sha256: "bb".repeat(32), size: 20n, path: "l" },
  ],
});

describe("update decision", () => {
  it("installs when nothing is installed", () => expect(updateDecision(null, release(2))).toBe("install"));
  it("updates only for a strictly higher version", () => {
    expect(updateDecision({ edition: 1, version: 2 }, release(3))).toBe("update");
    expect(updateDecision({ edition: 1, version: 3 }, release(3))).toBe("current");
    expect(updateDecision({ edition: 1, version: 4 }, release(3))).toBe("current");
  });
  it("has nothing to offer without a release", () => {
    expect(updateDecision(null, null)).toBe("unavailable");
    expect(updateDecision({ edition: 1, version: 1 }, null)).toBe("current");
  });
});

describe("build choice", () => {
  it("takes the device's first platform the release has", () => {
    expect(pickBuild(release(1), ["linux-x86_64", "windows-x86_64"])?.path).toBe("l");
    expect(pickBuild(release(1), ["windows-x86_64"])?.path).toBe("w");
    expect(pickBuild(release(1), ["macos-arm64"])).toBeNull();
    expect(pickBuild(release(1), [])).toBeNull();
  });
});

describe("licence windows", () => {
  const now = 30 * 86_400_000;
  it("refundable only before the window ends and only as a plain licence", () => {
    expect(refundable({ type: "licence", refundable_until_ms: now + 1 }, now)).toBe(true);
    expect(refundable({ type: "licence", refundable_until_ms: now }, now)).toBe(false);
    expect(refundable({ type: "licence", refundable_until_ms: null }, now)).toBe(false);
    expect(refundable({ type: "resale", refundable_until_ms: now + 1 }, now)).toBe(false);
  });
  it("tradable once the hold is over", () => {
    expect(tradableNow({ type: "licence", tradable_from_ms: now }, now)).toBe(true);
    expect(tradableNow({ type: "licence", tradable_from_ms: now + 1 }, now)).toBe(false);
    expect(tradableNow({ type: "licence", tradable_from_ms: null }, now)).toBe(false);
  });
  it("launch grace is 7 days from the last good check", () => {
    expect(withinGrace(now - LAUNCH_GRACE_MS, now)).toBe(true);
    expect(withinGrace(now - LAUNCH_GRACE_MS - 1, now)).toBe(false);
    expect(withinGrace(0, now)).toBe(false);
  });
});

describe("prices", () => {
  const g = (usd: string, sale: string | null, ends: number | null) => ({ price: { usd, sale_usd: sale, sale_ends_ms: ends, accept_sui: true } });
  it("uses the sale price while the sale runs", () => {
    expect(priceNow(g("9990000", "6990000", 2000), 1000)).toBe(6_990_000n);
    expect(priceNow(g("9990000", "6990000", 2000), 2000)).toBe(9_990_000n);
    expect(onSale(g("9990000", "6990000", 2000), 1000)).toBe(true);
    expect(priceNow({ price: null })).toBeNull();
  });
  it("formats micro-USD", () => {
    expect(usdText(14_990_000n)).toBe("$14.99");
    expect(usdText(0n)).toBe("Free");
    expect(usdText(1_234_500_000n)).toBe("$1,234.50");
  });
  it("finds Steam app ids", () => {
    expect(steamAppId("https://store.steampowered.com/app/2379780/Balatro/")).toBe(2379780);
    expect(steamAppId("https://example.com")).toBeNull();
  });
});

describe("config", () => {
  it("is closed until the package is deployed", () => {
    const c = normalizeConfig({ package: null, store: null, usdc_type: null, uploads_open: false });
    expect(canSell(c)).toBe(false);
  });
  it("reads snake_case, camelCase and env names", () => {
    const a = normalizeConfig({ package: "0x1", store: "0x2", store_version: "7", usdc_type: "0x3::usdc::USDC", policy: "0x4" });
    expect(canSell(a) && canResell(a)).toBe(true);
    expect(a.storeVersion).toBe("7");
    const b = normalizeConfig({ config: { RARE_STORE_PACKAGE: "0x1", RARE_STORE_STORE: "0x2", RARE_STORE_USDC_TYPE: "0x3::usdc::USDC" } });
    expect(canSell(b)).toBe(true);
    expect(canResell(b)).toBe(false);
  });
});
