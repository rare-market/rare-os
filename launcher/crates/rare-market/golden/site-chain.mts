// Writes media.json, the golden answers for the Rust image resolver (rare-market src/media.rs), from
// rare.market's own sources, so the native launcher shows the same art as the site:
//   node site-chain.mts [path to the market repo, default $MARKET_REPO or ../market]
// parseRef, candidates, usableRef, inlineImage, wsrvBlocked: packages/venues/src/media.ts.
// chain400 / chainInside: web/lib/media.ts imageChain (own renditions, the wsrv front, wsrv, the
// originals, the passthrough), as the native launcher loads it: relative links on
// https://rare.market and no video: poster links (it has no video element).
import { writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";
import { INPUTS, JSONS } from "./media-inputs.ts";

const market = process.argv[2] ?? `${process.env.MARKET_REPO ?? "../market"}`;
const v = await import(`${market}/packages/venues/src/media.ts`);
const site = await import(`${market}/web/lib/media.ts`);
const abs = (u: string) => (u.startsWith("/") && !u.startsWith("//") ? `https://rare.market${u}` : u);
const native = (l: string[]) => l.filter((u) => !u.startsWith(site.VIDEO)).map(abs);
const out = {
  refs: INPUTS.map((s: string) => {
    const r = v.parseRef(s);
    return {
      input: s,
      ref: r,
      key: v.refKey(r),
      candidates: v.candidates(r),
      usable: v.usableRef(s),
      inline: v.inlineImage(s),
      chain400: native(site.imageChain(s, 400)),
      chainInside: native(site.imageChain(s, 1000.6, { fit: "inside" })),
      wsrvBlocked: v.wsrvBlocked(s),
    };
  }),
  json: JSONS.map((j: Record<string, unknown>) => ({ input: j, image: v.pickImageFromJson(j) })),
};
writeFileSync(fileURLToPath(new URL("./media.json", import.meta.url)), JSON.stringify(out, null, 1) + "\n");
console.log(`media.json: ${out.refs.length} refs, ${out.json.length} json`);
