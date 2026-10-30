// Ports the web launcher's TS mocks to the native launcher's fixtures, mechanically.
//
//   node launcher/native/tests/fixtures/export.mjs            (needs launcher/node_modules: npm ci)
//   LAUNCHER_NODE_MODULES=/other/checkout/launcher/node_modules node .../export.mjs
//
// The mocks (src/lib/mock.ts, stream, share, notify, friends, store, trophies, inventory,
// onboarding/move/demo) are bundled with rolldown and run in Node once per web screenshot
// query (scripts/shots.mjs and shots-wallet-*.mjs), with the clock fixed at FIXED_MS and no network. Every getter's
// answer is written as JSON in the shape the Tauri commands and rare.market return, which is
// the shape the Rust backend crates serialize, so a native screen deserializes a fixture into
// the same types it gets from the backend. Math.random is seeded, so a rerun is byte-identical.
//
// Output (committed; rerun after a mock changes and review the diff):
//   <module>/<variant>.json  one module's answers, e.g. lib/default.json { "library": ..., "device": ... }
//   index.json               { fixedMs, sets: { <set>: { query, modules: { lib: <variant>, ... } } },
//                              shots: { <web shot name>: <set> } }
// Identical answers are stored once: most sets share most modules' "default" variant.
// Art URLs stay as the mocks have them; tools/render/golden maps them to the placeholder art
// in art/ (no network in the golden runs).

import { createRequire } from "node:module";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, writeFileSync, rmSync, mkdtempSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const LAUNCHER = resolve(HERE, "../../..");
const SRC = join(LAUNCHER, "src");
// 2026-10-27 13:41:00 UTC (9:41 AM in New York): the clock every golden image shows.
export const FIXED_MS = Date.UTC(2026, 9, 27, 13, 41, 0);

const nm = process.env.LAUNCHER_NODE_MODULES ?? join(LAUNCHER, "node_modules");
if (!existsSync(join(nm, "rolldown"))) {
  console.error(`rolldown not found in ${nm}: run npm ci in launcher/ or set LAUNCHER_NODE_MODULES`);
  process.exit(2);
}
const { rolldown } = await import(createRequire(join(nm, "x.js")).resolve("rolldown"));

// ---- 1. the probe: imports every mock, calls each getter, prints one JSON object

const entry = `
import * as lib from ${JSON.stringify(join(SRC, "lib/mock.ts"))};
import * as stream from ${JSON.stringify(join(SRC, "stream/mock.ts"))};
import * as share from ${JSON.stringify(join(SRC, "share/mock.ts"))};
import * as notify from ${JSON.stringify(join(SRC, "notify/mock.ts"))};
import * as friends from ${JSON.stringify(join(SRC, "friends/mock.ts"))};
import * as store from ${JSON.stringify(join(SRC, "store/mock.ts"))};
import * as trophies from ${JSON.stringify(join(SRC, "trophies/mock.ts"))};
import * as inventory from ${JSON.stringify(join(SRC, "inventory/mock.ts"))};
import * as move from ${JSON.stringify(join(SRC, "onboarding/move/demo.ts"))};

const ME = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const calls = {
  lib: {
    library: () => lib.library(), device: () => lib.device(), perf: () => lib.perf(), gamePower: () => lib.gamePower(),
    hud: () => lib.hud(), batteryCare: () => lib.batteryCare(), levels: () => lib.levels(), config: () => lib.config(),
    netStatus: () => lib.netStatus(), wifiScan: () => lib.wifiScan(), radios: () => lib.radios(), bluetooth: () => lib.bluetooth(),
    steamAccount: () => lib.steamAccount(), upscale: () => lib.upscale(), brightnessInfo: () => lib.brightnessInfo(),
    displayState: () => lib.displayState(), osUpdate: () => lib.osUpdate(), osRestoreAvailable: () => lib.osRestoreAvailable(),
    running: () => lib.running(),
  },
  stream: { status: () => stream.status(), hosts: () => stream.refresh(), discover: () => stream.discover(), apps: () => stream.apps(stream.PC) },
  share: { status: () => share.status(), list: () => share.list() },
  notify: { recent: () => notify.recent(), alerts: () => notify.listAlerts() },
  friends: { overview: () => friends.overview(), code: () => friends.code() },
  store: {
    config: () => store.answer("/v1/store/config"), games: () => store.answer("/v1/store/games?limit=100"),
    licences: () => store.answer("/v1/store/licences"), price: () => store.answer("/v1/store/price/sui"),
  },
  trophies: { steam: () => trophies.mockSteam(), config: () => trophies.MOCK_CONFIG },
  inventory: { inventory: () => inventory.mock.get("/v1/players/" + ME + "/inventory") },
  move: { scan: () => move.demoScan(move.DEMO_PLAYTRON) },
};

const out = {};
const errors = {};
for (const [mod, fns] of Object.entries(calls)) {
  out[mod] = {};
  for (const [k, f] of Object.entries(fns)) {
    try {
      const v = await f();
      out[mod][k] = v === undefined ? null : JSON.parse(JSON.stringify(v, (_, x) => (typeof x === "bigint" ? x.toString() : x)));
    } catch (e) {
      errors[mod + "." + k] = String(e && e.message || e).slice(0, 200);
    }
  }
}
// A file, not stdout: process.exit cuts a large write to a pipe short.
(await import("node:fs")).writeFileSync(process.env.FX_OUT, JSON.stringify({ data: out, errors }));
process.exit(0);
`;

// Runs before any module body: a browser-ish global with the shot's query, an empty
// localStorage, and Date frozen at FIXED_MS (timers still run, so the mocks' delays resolve).
const banner = `
const __FIXED = ${FIXED_MS};
const __RealDate = Date;
class __FixedDate extends __RealDate {
  constructor(...a) { if (a.length === 0) super(__FIXED); else super(...a); }
  static now() { return __FIXED; }
}
globalThis.Date = __FixedDate;
let __seed = 0x5eed;
Math.random = () => { __seed |= 0; __seed = (__seed + 0x6d2b79f5) | 0; let t = Math.imul(__seed ^ (__seed >>> 15), 1 | __seed); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
const __q = process.env.FX_Q || "";
globalThis.location = { search: __q ? "?" + __q : "", href: "http://localhost/?" + __q, origin: "http://localhost", pathname: "/" };
const __ls = new Map();
globalThis.localStorage = { getItem: (k) => (__ls.has(k) ? __ls.get(k) : null), setItem: (k, v) => __ls.set(k, String(v)), removeItem: (k) => __ls.delete(k), clear: () => __ls.clear() };
globalThis.window = globalThis;
globalThis.document = globalThis.document || { addEventListener() {}, removeEventListener() {}, querySelector: () => null, visibilityState: "visible" };
`;

const work = mkdtempSync(join(tmpdir(), "rare-fixtures-"));
const entryPath = join(work, "entry.ts");
writeFileSync(entryPath, entry);
const bundle = await rolldown({
  input: entryPath,
  platform: "node",
  logLevel: "silent",
  // Everything is bundled (the probe runs from a temp dir), resolved from the launcher's node_modules.
  resolve: { modules: [nm, "node_modules"] },
  transform: { define: { __MOCK_OK__: "true", __RARE_NATIVE__: "false", "import.meta.env.DEV": "true", "import.meta.env.PROD": "false" } },
  moduleTypes: { ".svg": "text", ".png": "dataurl", ".jpg": "dataurl", ".webp": "dataurl" },
});
const bundlePath = join(work, "probe.mjs");
await bundle.write({ file: bundlePath, format: "esm", banner });

// ---- 2. one probe run per web screenshot query (8 at a time: each waits out the mocks' delays)

const shotsSrc = readFileSync(join(LAUNCHER, "scripts/shots.mjs"), "utf8");
const consts = {};
for (const m of shotsSrc.matchAll(/const (\w+) = (?:process\.env\.\w+ \?\? )?"([^"]*)";/g)) consts[m[1]] = m[2];
const shots = [];
for (const m of shotsSrc.matchAll(/\{ name: "([^"]+)", q: (["`])(.*?)\2/g)) {
  const q = m[3].replace(/\$\{(\w+)\}/g, (_, k) => consts[k] ?? "");
  shots.push({ name: m[1], q });
}
// The wallet shot lists are plain modules (no browser at import): take their entries as they are.
for (const f of readdirSync(join(LAUNCHER, "scripts")).filter((f) => /^shots-wallet-.*\.mjs$/.test(f)).sort()) {
  for (const s of (await import(pathToFileURL(join(LAUNCHER, "scripts", f)).href)).default) shots.push({ name: s.name, q: s.q });
}
if (shots.length < 50) {
  console.error(`only ${shots.length} shots parsed from scripts/shots.mjs; the parser needs updating`);
  process.exit(1);
}

const queries = [...new Set(shots.map((s) => s.q))];
const byQuery = new Map();
let next = 0;
async function worker() {
  while (next < queries.length) {
    const q = queries[next++];
    const outPath = join(work, `out-${createHash("sha1").update(q).digest("hex")}.json`);
    await new Promise((ok, fail) =>
      execFile(process.execPath, [bundlePath], { env: { ...process.env, FX_Q: q, FX_OUT: outPath, TZ: "UTC" }, timeout: 60000 }, (e, _o, err) =>
        e ? fail(new Error(`query ${q}: ${e.message} ${err}`)) : ok(),
      ),
    );
    byQuery.set(q, JSON.parse(readFileSync(outPath, "utf8")));
  }
}
await Promise.all(Array.from({ length: 8 }, worker));

// ---- 3. dedupe each module's answers by content and write

const stable = (v) => JSON.stringify(v, (_, x) => (x && typeof x === "object" && !Array.isArray(x) ? Object.fromEntries(Object.keys(x).sort().map((k) => [k, x[k]])) : x), 1);
const modules = Object.keys(byQuery.get(queries[0]).data);
for (const m of modules) rmSync(join(HERE, m), { recursive: true, force: true });
rmSync(join(HERE, "sets"), { recursive: true, force: true });
const variants = new Map(); // module -> hash -> variant name
const index = {
  _doc: "Generated by export.mjs from the web launcher's TS mocks (do not edit by hand). A set is one web screenshot query; its data is <module>/<variant>.json for each module. tools/render/golden merges a set into one file for RR_FIXTURE.",
  fixedMs: FIXED_MS, fixedIso: new Date(FIXED_MS).toISOString(), modules, sets: {}, shots: {}, errors: {},
};
const plain = shots.find((s) => s.q === "tab=home&nosignin") ?? shots[0];
const setName = new Map();
for (const s of [plain, ...shots.filter((x) => x !== plain)]) {
  if (!setName.has(s.q)) {
    const name = s === plain ? "default" : s.name;
    setName.set(s.q, name);
    const r = byQuery.get(s.q);
    const mods = {};
    for (const m of modules) {
      const body = stable(r.data[m]);
      const h = createHash("sha256").update(body).digest("hex").slice(0, 12);
      if (!variants.has(m)) variants.set(m, new Map());
      const vm = variants.get(m);
      if (!vm.has(h)) {
        vm.set(h, name);
        mkdirSync(join(HERE, m), { recursive: true });
        writeFileSync(join(HERE, m, `${name}.json`), body + "\n");
      }
      mods[m] = vm.get(h);
    }
    index.sets[name] = { query: s.q, modules: mods };
    if (Object.keys(r.errors).length) index.errors[name] = r.errors;
  }
  index.shots[s.name] = setName.get(s.q);
}
index.shots = Object.fromEntries(Object.entries(index.shots).sort());
writeFileSync(join(HERE, "index.json"), JSON.stringify(index, null, 1) + "\n");
rmSync(work, { recursive: true, force: true });
const nv = [...variants].map(([m, v]) => `${m} ${v.size}`).join(", ");
console.log(`${shots.length} web shots, ${queries.length} sets; variants per module: ${nv}`);
if (Object.keys(index.errors).length) console.log(`getters that threw (listed in index.json): ${[...new Set(Object.values(index.errors).flatMap(Object.keys))].join(", ")}`);
