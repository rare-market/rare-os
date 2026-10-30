import { afterEach, describe, expect, it, vi } from "vitest";
import { asRarelingType, packageFromManifest, rarelingsOn, rarelingType } from "./config";
import { walletHoldsRareling } from "./store";
import { applyTheme, THEMES, themeById } from "./themes";
import { upcomingLaunches } from "./ui";

describe("rareling type", () => {
  it("takes a package id or a full type", () => {
    expect(asRarelingType("0xABC")).toBe("0xabc::rareling::Rareling");
    expect(asRarelingType("0xabc::rareling::Rareling")).toBe("0xabc::rareling::Rareling");
    expect(asRarelingType("")).toBe(null);
    expect(asRarelingType("rareling")).toBe(null);
  });
  it("reads the package id from likely manifest shapes, original id first", () => {
    expect(packageFromManifest({ packageId: "0x1" })).toBe("0x1");
    expect(packageFromManifest({ rarelings: { packageId: "0x2" } })).toBe("0x2");
    expect(packageFromManifest({ packages: { rarelings: { id: "0x3" }, market: { id: "0x9" } } })).toBe("0x3");
    expect(packageFromManifest({ packages: [{ name: "market", packageId: "0x9" }, { name: "rarelings", packageId: "0x4", originalId: "0x5" }] })).toBe("0x5");
    expect(packageFromManifest(null)).toBe(null);
  });
});

describe("themes", () => {
  it("only rare.market triplets, and a reset back to app.css", () => {
    const allowed = new Set(["61 90 254", "47 72 217", "92 118 255", "138 155 255", "122 53 255", "212 189 255"]);
    for (const t of THEMES) for (const v of Object.values(t.vars ?? {})) expect(allowed.has(v)).toBe(true);
    const props = new Map<string, string>();
    const el = { style: { setProperty: (k: string, v: string) => void props.set(k, v), removeProperty: (k: string) => void props.delete(k) }, dataset: {} as Record<string, string> } as unknown as HTMLElement;
    applyTheme("royal", el);
    expect(props.get("--brand")).toBe("122 53 255");
    applyTheme(null, el);
    expect(props.size).toBe(0);
    expect(themeById("nope").id).toBe("rare");
  });
});

describe("early access", () => {
  it("lists launches that have not opened, soonest first", () => {
    const now = Date.parse("2026-10-26T00:00:00Z");
    const list = [
      { slug: "past", name: "Past", startsAt: "2026-10-01T00:00:00Z" },
      { slug: "late", name: "Late", startsAt: now + 9e6 },
      { slug: "soon", name: "Soon", phases: [{ startsAt: now + 1e6 }] },
      { slug: "none", name: "None" },
    ];
    expect(upcomingLaunches(list, now).map((l) => l.slug)).toEqual(["soon", "late"]);
  });
});

describe("Rarelings are off for this release", () => {
  afterEach(() => vi.unstubAllEnvs());
  it("have no type, whatever the env or the device says, until VITE_RARELINGS=1", async () => {
    vi.stubEnv("VITE_RARELING_TYPE", "0xabc");
    vi.stubGlobal("localStorage", { getItem: () => "0xdef" });
    expect(rarelingsOn()).toBe(false);
    expect(rarelingType()).toBe(null);
    expect(await walletHoldsRareling("0x" + "1".repeat(64))).toBe(false);
    vi.stubEnv("VITE_RARELINGS", "1");
    expect(rarelingType()).toBe("0xdef::rareling::Rareling");
    vi.unstubAllGlobals();
  });
});
