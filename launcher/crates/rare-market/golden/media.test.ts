// The web launcher's copy of the image resolver against media.json (written by site-chain.mts from
// rare.market's own sources): parseRef, candidates, usableRef and inlineImage. The chains in
// media.json are the site's (own renditions first), which the native launcher follows; the web
// launcher's older chain is not compared.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { candidates, inlineImage, parseRef, pickImageFromJson, refKey, usableRef, wsrvBlocked } from "../../../src/marketplace/venues-media";

const FILE = fileURLToPath(new URL("./media.json", import.meta.url));
import { INPUTS, JSONS } from "./media-inputs";

describe("golden media answers for the Rust port", () => {
  it("match media.json", () => {
    const want = JSON.parse(readFileSync(FILE, "utf8"));
    const refs = INPUTS.map((s) => {
      const r = parseRef(s);
      return { input: s, ref: r, key: refKey(r), candidates: candidates(r), usable: usableRef(s), inline: inlineImage(s), wsrvBlocked: wsrvBlocked(s) };
    });
    const strip = (c: Record<string, unknown>) => {
      const { chain400: _a, chainInside: _b, ...rest } = c;
      return rest;
    };
    expect(want.refs.map(strip)).toEqual(JSON.parse(JSON.stringify(refs)));
    expect(want.json).toEqual(JSON.parse(JSON.stringify(JSONS.map((j) => ({ input: j, image: pickImageFromJson(j) })))));
  });
});
