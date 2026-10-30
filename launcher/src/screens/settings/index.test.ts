import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { SETTINGS_INDEX } from "./index";

const here = __dirname;
const sources = [
  ...readdirSync(here).filter((f) => f.endsWith(".tsx")).map((f) => join(here, f)),
  join(here, "../../notify/SettingsSection.tsx"),
]
  .map((f) => readFileSync(f, "utf8"))
  .join("\n");

describe("settings index", () => {
  it("has a row in a section for every entry", () => {
    const missing = SETTINGS_INDEX.filter((x) => x.id !== x.section) // section-level entries are whole panels
      .filter((x) => !(sources.includes(`id="${x.id}"`) || (x.id.startsWith("notif-") && sources.includes("id={`notif-${"))))
      .map((x) => x.id);
    expect(missing).toEqual([]);
  });

  it("every row id in the sections is in the index", () => {
    const ids = [...sources.matchAll(/\bid="([a-z-]+)"/g)].map((m) => m[1]);
    const known = new Set(SETTINGS_INDEX.map((x) => x.id));
    expect(ids.filter((i) => !known.has(i))).toEqual([]);
  });
});
