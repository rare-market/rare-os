// Before/after shots for the UX 21, 22 and 27 work. Headless Chromium from Playwright's cache,
// muted, with AudioContext stubbed. Usage: node scripts/ux-shots.mjs <port> <label> <outDir>
import puppeteer from "puppeteer-core";
import { mkdirSync } from "node:fs";
const [port, label, out] = process.argv.slice(2);
mkdirSync(out, { recursive: true });
const chrome = process.env.CHROME;
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const demo = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const holder = "0x7aa0c2206ef649591f9b12db3baa4572e01a30727b949f7749fc50e08f5ad784";
const shots = [
  { name: "home-stage", q: "tab=home&nosignin", wait: 1500 },
  { name: "home-menu", q: "tab=home&nosignin", after: async (p) => { await p.keyboard.press("Tab"); await pause(500); } },
  { name: "library-view", q: "tab=library&nosignin", after: async (p) => { await p.keyboard.press("s"); await pause(500); } },
  { name: "library-steam-empty", q: "tab=library&nosignin&filter=steam", after: async (p) => { await p.evaluate(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent.startsWith("Steam")); b && b.click(); }); await pause(400); } },
  { name: "topbar-offline", q: "tab=home&nosignin&offline", wait: 1500 },
  { name: "market-collection-list", q: "tab=market&nosignin&mk=collection/popkins", wait: 6000, after: async (p) => { await p.keyboard.press("s"); await pause(1000); } },
  { name: "overlay-watch-only", q: `tab=home&nosignin&watch=${holder}&running=web:pawtato-land&overlay=pawtato-land`, wait: 9000, after: async (p) => { for (let i = 0; i < 3; i++) { await p.keyboard.press("ArrowDown"); await pause(200); } await p.keyboard.press("Enter"); await pause(3500); } },
  { name: "overlay-add-passkey", q: `tab=home&nosignin&watch=${holder}&running=web:pawtato-land&overlay=pawtato-land`, wait: 9000, after: async (p) => { for (let i = 0; i < 3; i++) { await p.keyboard.press("ArrowDown"); await pause(200); } await p.keyboard.press("Enter"); await pause(3500); await p.evaluate(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent === "Add a passkey"); b && b.click(); }); await pause(1200); } },
  { name: "overlay-no-wallet", q: "tab=home&nosignin&running=web:pawtato-land&overlay=pawtato-land", wait: 4000 },
];
const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ["--hide-scrollbars", "--force-device-scale-factor=1", "--mute-audio"] });
for (const s of shots) {
  const ctx = await browser.createBrowserContext();
  const p = await ctx.newPage();
  await p.evaluateOnNewDocument(() => { const stub = function () { return { createGain: () => ({ gain: {}, connect() {} }), createOscillator: () => ({ frequency: {}, connect() {}, start() {}, stop() {} }), destination: {}, resume: async () => {}, close: async () => {} }; }; window.AudioContext = stub; window.webkitAudioContext = stub; });
  p.on("pageerror", (e) => console.error(`${s.name}: ${e.message}`));
  await p.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
  await p.goto(`http://localhost:${port}/?${s.q}`, { waitUntil: "load" });
  await pause(s.wait ?? 2500);
  try { await s.after?.(p); } catch (e) { console.error(s.name, e.message); }
  await pause(400);
  await p.screenshot({ path: `${out}/${s.name}-${label}.png` });
  await ctx.close();
}
await browser.close();
