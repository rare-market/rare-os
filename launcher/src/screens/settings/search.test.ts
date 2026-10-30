import { describe, expect, it } from "vitest";
import { SETTINGS_INDEX } from "./index";
import { scoreEntry, searchSettings, type SettingEntry } from "./search";
import { SECTIONS } from "./sections";

const e = (id: string, section: SettingEntry["section"], title: string, keywords: string[] = []): SettingEntry => ({ id, section, title, keywords });

const sample = [
  e("brightness", "display", "Brightness", ["screen", "dim", "backlight"]),
  e("volume", "sound", "Volume", ["audio", "loud", "speaker"]),
  e("wifi", "network", "Wi-Fi", ["wireless", "internet", "connect"]),
  e("sleep", "power", "Sleep when idle", ["suspend", "timeout"]),
  e("dim", "display", "Dim after idle", ["screen"]),
];

describe("searchSettings", () => {
  it("returns nothing for an empty query", () => {
    expect(searchSettings("", sample)).toEqual([]);
    expect(searchSettings("   ", sample)).toEqual([]);
  });

  it("matches titles by word prefix, ignoring case", () => {
    const r = searchSettings("BRIGHT", sample);
    expect(r).toHaveLength(1);
    expect(r[0].entries[0].id).toBe("brightness");
  });

  it("matches keywords", () => {
    expect(searchSettings("internet", sample)[0].entries[0].id).toBe("wifi");
    expect(searchSettings("suspend", sample)[0].entries[0].id).toBe("sleep");
  });

  it("needs every word to match", () => {
    expect(searchSettings("screen idle", sample)[0].entries.map((x) => x.id)).toEqual(["dim"]);
    expect(searchSettings("screen volume", sample)).toEqual([]);
  });

  it("groups by section in list order and ranks the title above a keyword", () => {
    const r = searchSettings("screen", sample);
    expect(r.map((g) => g.section)).toEqual(["display"]);
    const r2 = searchSettings("idle", sample);
    expect(r2.map((g) => g.section)).toEqual(["display", "power"]);
    const r3 = searchSettings("dim", sample);
    expect(r3[0].entries.map((x) => x.id)).toEqual(["dim", "brightness"]);
  });

  it("matches inside a word only from three letters", () => {
    expect(scoreEntry("ri", e("a", "system", "Wi-Fi"))).toBe(0);
    expect(scoreEntry("ight", e("a", "display", "Brightness"))).toBeGreaterThan(0);
  });

  it("finds a section by its name", () => {
    expect(searchSettings("sound", sample).some((g) => g.section === "sound")).toBe(true);
  });
});

describe("the real index", () => {
  it("has unique ids and known sections", () => {
    const ids = SETTINGS_INDEX.map((x) => x.id);
    expect(new Set(ids).size).toBe(ids.length);
    const known = new Set(SECTIONS.map((s) => s.id));
    for (const x of SETTINGS_INDEX) expect(known.has(x.section)).toBe(true);
  });

  it("finds the rows people look for", () => {
    const first = (q: string) => searchSettings(q, SETTINGS_INDEX)[0]?.entries[0]?.id;
    expect(first("brightness")).toBe("brightness");
    expect(first("wifi")).toBe("wifi");
    expect(first("volume")).toBe("volume");
    expect(first("battery")).toBeTruthy();
    expect(first("language")).toBe("language");
  });
});
