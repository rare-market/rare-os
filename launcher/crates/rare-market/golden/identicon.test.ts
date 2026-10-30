// Golden identicons for the Rust port (rare-market src/identicon.rs): the web launcher's gem
// avatars and banners for a few addresses. `UPDATE_GOLDEN=1 npx vitest run crates/rare-market/golden`
// rewrites identicon.json.

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { avatarSvg, avatarUri, bannerSvg } from "../../../src/marketplace/identicon";

const FILE = fileURLToPath(new URL("./identicon.json", import.meta.url));
const ADDRS = ["0x7682c8538863a72d81f09c1673d50ffda1220d5ec680f7f7f0c527eed0324fac", "0x3C2F7DE4C6E2892A80D44B77FAE4E14DCD95CF3B8F016C6B1F1DA456DF28587D", "0x1", "", "ünïcode"];

describe("golden identicons for the Rust port", () => {
  it("match identicon.json", () => {
    const out = ADDRS.map((a) => ({ address: a, avatar: avatarSvg(a), banner: bannerSvg(a), uri: avatarUri(a) }));
    const text = JSON.stringify(out, null, 1) + "\n";
    if (process.env.UPDATE_GOLDEN) writeFileSync(FILE, text);
    expect(JSON.parse(readFileSync(FILE, "utf8"))).toEqual(JSON.parse(text));
  });
});
