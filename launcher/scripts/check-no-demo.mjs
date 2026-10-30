// Fails when a release bundle (dist/, from `vite build`) still contains mock, demo or sample code.
// Release builds fix isTauri to true and replace every mock module with an empty stub (vite.config.ts), so none
// of these strings may survive. Run by `npm run build:ui`; `node scripts/check-no-demo.mjs [dir]`.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

export const DEMO_STRINGS = [
  "Demo wallet", // the ?watch= stand-in account
  "mock archive", // browser cloud-save stand-in
  "storedemo", // Rare Store sample listings (?storedemo)
  "Hollow Keep", // Rare Store sample game
  "devsign", // fake-signature wallet mode
  "devphrase", // test recovery phrase account
  "mockhw", // simulated Ledger / Keystone
  "mockudev",
  "demoscan", // made-up Playtron wallet
  "trophychain", // mocked trophies chain
  "squad up in the hub", // mock friends chat
  "mira.sui", // mock friends
  "Test of Resolve", // mock trophy
];

const root = process.argv[2] ?? new URL("../dist", import.meta.url).pathname;
const hits = [];
(function walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(js|mjs|html|css|json)$/.test(name)) {
      const text = readFileSync(p, "utf8");
      for (const s of DEMO_STRINGS) if (text.includes(s)) hits.push(`${p}: ${s}`);
    }
  }
})(root);
if (hits.length) {
  console.error(`Demo or mock content found in the release bundle:\n${hits.join("\n")}`);
  process.exit(1);
}
console.log("check-no-demo: release bundle is free of mock and demo content.");
