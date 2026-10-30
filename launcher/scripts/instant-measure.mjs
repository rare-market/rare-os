// Per-screen timings in mock mode, for before/after comparisons of the instant layer (lib/state.ts,
// src-tauri/src/instant.rs). Headless WebKit, muted (no AudioContext, no Audio), never a real browser.
//
//   VITE_MOCK=1 npx vite build --outDir dist-mock && npx vite preview --outDir dist-mock --port 5181
//   node scripts/instant-measure.mjs            # URL=http://localhost:5181
//
// Network is recorded once into $FIXTURES (default: the OS temp dir) and replayed with a fixed
// latency (API_MS, IMG_MS), so two builds see the same data at the same speed. Per screen:
//   cold   a fresh profile (nothing cached) opening the launcher on that tab
//   warm   the same profile opened again (a restart with the caches of the first run)
//   switch the tab reached with RB from the one before, first visit in a session that has run
// and for each: content (ms until the screen's content stops changing), images (ms until every
// image on screen has loaded), long tasks (main-thread blocks over 50 ms: count / longest) and
// dropped frames. WebKit has no Long Tasks API: a MessageChannel ping measures the blocks.
import { webkit } from "playwright-core";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const base = process.env.URL ?? "http://localhost:5181";
const FIX = process.env.FIXTURES ?? join(tmpdir(), "rare-instant-fixtures");
const API_MS = Number(process.env.API_MS ?? 250);
const IMG_MS = Number(process.env.IMG_MS ?? 150);
const WINDOW = Number(process.env.WINDOW_MS ?? 6000);
const TABS = (process.env.TABS ?? "home,library,games,market,wallet").split(",");
const RUNS = Number(process.env.RUNS ?? 3);
const label = process.env.LABEL ?? base;
mkdirSync(FIX, { recursive: true });
const pause = (ms) => new Promise((r) => setTimeout(r, ms));

// ---------- record / replay ----------
async function routeAll(ctx) {
  await ctx.route(/^https?:\/\//, async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const api = url.pathname.startsWith("/rare/") || url.pathname.startsWith("/trade-api/") || /trade\.casa|rare\.market/.test(url.host);
    const local = url.host === new URL(base).host && !api;
    if (local || req.method() !== "GET") return route.continue();
    const key = createHash("sha1").update(url.pathname.startsWith("/rare/") || url.pathname.startsWith("/trade-api/") ? url.pathname + url.search : req.url()).digest("hex");
    const meta = join(FIX, `${key}.json`), body = join(FIX, `${key}.bin`);
    if (!existsSync(meta)) {
      try {
        const res = await route.fetch({ timeout: 15000 });
        writeFileSync(body, await res.body());
        writeFileSync(meta, JSON.stringify({ status: res.status(), type: res.headers()["content-type"] ?? "application/octet-stream", url: req.url() }));
      } catch {
        writeFileSync(body, "");
        writeFileSync(meta, JSON.stringify({ status: 504, type: "text/plain", url: req.url() }));
      }
    }
    const m = JSON.parse(readFileSync(meta, "utf8"));
    await pause(req.resourceType() === "image" ? IMG_MS : API_MS);
    await route.fulfill({ status: m.status, headers: { "content-type": m.type, "cache-control": req.resourceType() === "image" ? "max-age=3600" : "no-store", "access-control-allow-origin": "*" }, body: readFileSync(body) }).catch(() => {});
  });
}

// ---------- in-page sampler ----------
function sampler() {
  const w = window;
  // Silent: no audio device is ever opened.
  const N = function () { return new Proxy({}, { get: () => () => N() }); };
  w.AudioContext = w.webkitAudioContext = class { constructor() { return N(); } };
  w.Audio = class { play() { return Promise.resolve(); } pause() {} addEventListener() {} };
  const m = (w.__im = { t0: 0, samples: [], frames: [], blocks: [] });
  const sig = () => {
    const main = document.querySelector("main");
    if (!main) return { s: "none", pending: 0 };
    const view = [...main.querySelectorAll(":scope > div:not([hidden]) img, :scope img")].filter((i) => {
      if (i.closest("[inert]")) return false;
      const r = i.getBoundingClientRect();
      return r.width > 0 && r.bottom > 0 && r.top < innerHeight && r.right > 0 && r.left < innerWidth;
    });
    const nav = main.querySelectorAll("[data-nav]").length;
    const text = (main.innerText || "").length;
    const pending = view.filter((i) => !i.complete || (i.naturalWidth === 0 && i.getAttribute("src"))).length;
    return { s: `${nav}|${view.length}|${Math.round(text / 40)}`, pending };
  };
  let last = performance.now();
  const frame = (now) => {
    m.frames.push([now, now - last]);
    last = now;
    const { s, pending } = sig();
    m.samples.push([now, s, pending]);
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
  // Main-thread blocks: a ping that should come back within a few ms.
  const ch = new MessageChannel();
  let sent = performance.now();
  ch.port1.onmessage = () => {
    const now = performance.now();
    if (now - sent > 50) m.blocks.push([sent, now - sent]);
    sent = now;
    setTimeout(() => { sent = performance.now(); ch.port2.postMessage(0); }, 4);
  };
  ch.port2.postMessage(0);
}

function analyse(m, t0, t1) {
  const s = m.samples.filter(([t]) => t >= t0 && t <= t1);
  if (!s.length) return null;
  const final = s[s.length - 1][1];
  let content = 0;
  for (let i = 1; i < s.length; i++) if (s[i][1] !== s[i - 1][1]) content = s[i][0] - t0;
  let images = content;
  for (const [t, sig, pending] of s) if (pending > 0) images = Math.max(images, t - t0 + 16);
  const blocks = m.blocks.filter(([t]) => t >= t0 && t <= t1).map(([, d]) => d);
  const dropped = m.frames.filter(([t, d]) => t >= t0 && t <= t1 && d > 25).reduce((n, [, d]) => n + Math.round(d / 16.7) - 1, 0);
  return { content: Math.round(content), images: Math.round(images), long: blocks.length, longest: Math.round(Math.max(0, ...blocks)), dropped, final };
}

const browser = await webkit.launch();
const rows = [];
const add = (screen, kind, r) => r && rows.push({ screen, kind, ...r });

for (let run = 0; run < RUNS; run++) {
  for (const tab of TABS) {
    const ctx = await browser.newContext({ viewport: { width: 1920, height: 1200 } });
    await ctx.addInitScript(sampler);
    await routeAll(ctx);
    const p = await ctx.newPage();
    // cold: nothing cached
    await p.goto(`${base}/?tab=${tab}&nosignin`);
    await pause(WINDOW);
    add(tab, "cold", analyse(await p.evaluate(() => window.__im), 0, WINDOW));
    // warm: the same profile again
    await p.goto(`${base}/?tab=${tab}&nosignin`);
    await pause(WINDOW);
    add(tab, "warm", analyse(await p.evaluate(() => window.__im), 0, WINDOW));
    await ctx.close();
  }
  // Tab switches with RB, in a session whose profile has run once (the device's normal case).
  const ctx = await browser.newContext({ viewport: { width: 1920, height: 1200 } });
  await ctx.addInitScript(sampler);
  await routeAll(ctx);
  const p = await ctx.newPage();
  await p.goto(`${base}/?tab=home&nosignin`);
  for (let i = 1; i < TABS.length; i++) {
    await pause(1500);
    await p.keyboard.press("e");
  }
  await pause(3000);
  await p.goto(`${base}/?tab=home&nosignin`);
  await pause(4000);
  for (const t of TABS.slice(1)) {
    const t0 = await p.evaluate(() => performance.now());
    await p.keyboard.press("e");
    await pause(3000);
    add(t, "switch", analyse(await p.evaluate(() => window.__im), t0, t0 + 3000));
  }
  await ctx.close();
}
await browser.close();

// Median per screen and kind.
const med = (a) => { const s = [...a].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]; };
const out = [];
for (const kind of ["cold", "warm", "switch"]) {
  for (const tab of TABS) {
    const r = rows.filter((x) => x.screen === tab && x.kind === kind);
    if (!r.length) continue;
    out.push({ screen: tab, kind, content: med(r.map((x) => x.content)), images: med(r.map((x) => x.images)), long: med(r.map((x) => x.long)), longest: med(r.map((x) => x.longest)), dropped: med(r.map((x) => x.dropped)) });
  }
}
console.log(`${label}: medians of ${RUNS} runs, API ${API_MS} ms, images ${IMG_MS} ms`);
console.log("screen    kind    content  images  long  longest  dropped");
for (const o of out) console.log(`${o.screen.padEnd(9)} ${o.kind.padEnd(7)} ${String(o.content).padStart(7)} ${String(o.images).padStart(7)} ${String(o.long).padStart(5)} ${String(o.longest).padStart(8)} ${String(o.dropped).padStart(8)}`);
if (process.env.JSON) writeFileSync(process.env.JSON, JSON.stringify(out, null, 2));
