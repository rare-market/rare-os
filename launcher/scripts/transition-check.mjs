// Tab and route transition smoothness in mock mode: long tasks, layout shift, blank frames, slow frames.
// Run `npm run dev:ui`, then `npm run transcheck`. ENGINE=webkit (default) or chrome (CPU throttle via
// THROTTLE=4, a 7840U at 15 W is about 4x slower than this Mac). A blank frame is a frame where the tab
// content area holds no focusable element and no text (the Suspense fallback showing nothing).
import { webkit, chromium } from "playwright-core";

const base = process.env.URL ?? "http://localhost:5180";
const engine = process.env.ENGINE ?? "webkit";
const throttle = Number(process.env.THROTTLE ?? (engine === "chrome" ? 4 : 1));
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const browser = engine === "chrome"
  ? await chromium.launch({ executablePath: process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", args: ["--hide-scrollbars", "--mute-audio"] })
  : await webkit.launch();
const TABS = ["library", "games", "wallet", "trade", "market", "friends", "settings", "home"];
const cold = process.env.COLD !== "0"; // each tab visited first time from a fresh page (lazy chunk not loaded)
let bad = 0;

function probe() {
  // Installed before each transition: records per-frame gaps, blank frames, layout shifts and long tasks.
  const w = window;
  w.__m = { frames: [], blank: 0, blankMs: 0, shift: 0, long: [], t0: performance.now() };
  try {
    new PerformanceObserver((l) => l.getEntries().forEach((e) => { if (!e.hadRecentInput) w.__m.shift += e.value; })).observe({ type: "layout-shift", buffered: false });
  } catch {}
  try {
    new PerformanceObserver((l) => l.getEntries().forEach((e) => w.__m.long.push(Math.round(e.duration)))).observe({ type: "longtask", buffered: false });
  } catch {}
  let last = performance.now();
  const empty = () => {
    const m = document.querySelector("main");
    if (!m) return false;
    const c = m.firstElementChild;
    return !c || (c.childElementCount === 0 && !(c.textContent || "").trim());
  };
  const loop = () => {
    const now = performance.now();
    w.__m.frames.push(now - last);
    if (empty()) { w.__m.blank++; w.__m.blankMs += now - last; }
    last = now;
    if (now - w.__m.t0 < 1500) requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
}

async function run(label, prepare, act) {
  const ctx = await browser.newContext({ viewport: { width: 1920, height: 1200 } });
  await ctx.addInitScript(() => { const N = function () { return new Proxy({}, { get: () => () => N() }); }; window.AudioContext = window.webkitAudioContext = class { constructor() { return N(); } }; window.Audio = class { play() { return Promise.resolve(); } pause() {} addEventListener() {} }; });
  const p = await ctx.newPage();
  if (throttle > 1 && engine === "chrome") { const c = await ctx.newCDPSession(p); await c.send("Emulation.setCPUThrottlingRate", { rate: throttle }); }
  await p.goto(`${base}/?${prepare}`);
  await pause(3500);
  await p.evaluate(probe);
  await act(p);
  await pause(1700);
  const m = await p.evaluate(() => window.__m);
  const worst = Math.max(...m.frames.slice(1));
  const slow = m.frames.slice(1).filter((f) => f > 25).length;
  const row = `${label.padEnd(22)} blank ${String(m.blank).padStart(2)} fr (${Math.round(m.blankMs)} ms)  worst frame ${Math.round(worst)} ms  >25ms frames ${slow}  CLS ${m.shift.toFixed(3)}  long tasks ${m.long.length ? m.long.join(",") : "-"}`;
  console.log(row);
  if (m.blank > 0 || m.shift > 0.1 || m.long.some((l) => l > 200)) bad++;
  await ctx.close();
}

console.log(`engine ${engine}, cpu throttle ${throttle}x, cold = first visit of each tab`);
for (const t of TABS) await run(`first open ${t}`, "tab=home&nosignin", async (p) => { await p.evaluate((x) => { window.__pad; }, t); await p.evaluate(async (x) => { const s = await import("/src/lib/store.ts"); s.useApp.getState().setTab(x); }, t); });
// Warm: second visit (chunk loaded) via LB/RB.
await run("RB then LB (warm)", "tab=library&nosignin", async (p) => { await p.evaluate(() => window.__pad("rb")); await pause(400); await p.evaluate(() => window.__pad("lb")); });
await run("open game page + B", "tab=home&nosignin", async (p) => { await p.evaluate(() => window.__pad("x")); await pause(500); await p.evaluate(() => window.__pad("b")); });
await run("quick settings + B", "tab=home&nosignin", async (p) => { await p.evaluate(() => window.__pad("start")); await pause(500); await p.evaluate(() => window.__pad("b")); });

// Home stage while sweeping the shelf: the logo block and its buttons must always be on screen. Per frame, the best
// effective opacity of any Play button (outgoing layers included) must stay high; 0 means an empty stage.
{
  const ctx = await browser.newContext({ viewport: { width: 1920, height: 1200 } });
  await ctx.addInitScript(() => { window.AudioContext = window.webkitAudioContext = undefined; });
  const p = await ctx.newPage();
  if (throttle > 1 && engine === "chrome") { const c = await ctx.newCDPSession(p); await c.send("Emulation.setCPUThrottlingRate", { rate: throttle }); }
  await p.goto(`${base}/?tab=home&nosignin`);
  await pause(3500);
  await p.evaluate(() => {
    window.__s = { frames: 0, min: 1, empty: 0, crossfades: 0 };
    const eff = (el) => { let o = 1; for (let e = el; e; e = e.parentElement) o *= parseFloat(getComputedStyle(e).opacity); return o; };
    const loop = () => {
      const btns = [...document.querySelectorAll("main [data-nav-group=hero] button")].filter((b) => /Play|Resume|Open/.test(b.textContent));
      const best = btns.length ? Math.max(...btns.map(eff)) : 0;
      window.__s.frames++;
      if (document.querySelector("main [inert] [data-nav-group=hero]")) window.__s.crossfades++;
      window.__s.min = Math.min(window.__s.min, best);
      if (best < 0.4) window.__s.empty++;
      requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
  });
  for (let i = 0; i < 10; i++) { await p.evaluate(() => window.__pad("right")); await pause(i % 2 ? 380 : 120); }
  await pause(600);
  const s = await p.evaluate(() => window.__s);
  console.log(`${"home stage sweep".padEnd(22)} frames ${s.frames}  lowest Play opacity ${s.min.toFixed(2)}  frames with a crossfade ${s.crossfades}  empty frames ${s.empty}`);
  if (s.empty > 0) bad++;
  await ctx.close();
}
await browser.close();
console.log(bad ? `${bad} transitions need work` : "all transitions clean");
