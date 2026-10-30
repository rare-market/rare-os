// Refreshes src/lib/anticheat.json from AreWeAntiCheatYet (MIT, https://github.com/AreWeAntiCheatYet/AreWeAntiCheatYet).
// Keyed by Steam appid: [status, anti-cheats]. Status is Supported | Running | Planned | Broken | Denied.
import { writeFileSync } from "node:fs";
const r = await fetch("https://raw.githubusercontent.com/AreWeAntiCheatYet/AreWeAntiCheatYet/HEAD/games.json");
const games = await r.json();
const out = {};
for (const g of games) {
  const id = g.storeIds?.steam;
  if (id) out[id] = [g.status, g.anticheats ?? []];
}
writeFileSync(new URL("../src/lib/anticheat.json", import.meta.url), JSON.stringify(out));
console.log(Object.keys(out).length, "games");
