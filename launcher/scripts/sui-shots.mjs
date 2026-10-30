// Screenshots of the Sui features (profile, cloud saves, Sui shelf on Home, owned game page) in
// browser mode with the mock native side and live rare.market and Sui data.
// Usage: npm run dev:ui (another shell), then: URL=http://localhost:5180 node scripts/sui-shots.mjs [outDir]
import puppeteer from "puppeteer-core";
import { mkdirSync } from "node:fs";

const out = process.argv[2] ?? "../docs/screenshots";
mkdirSync(out, { recursive: true });
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const base = process.env.URL ?? "http://localhost:5180";
// The same public demo account the other shots use (holds Popkins, a Sui game's collection).
const demo = process.env.ADDR ?? "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const click = (text) => (p) => p.evaluate((t) => [...document.querySelectorAll("button")].find((b) => b.textContent.includes(t))?.click(), text);

const shots = [
  { name: "sui-home", q: `tab=home&nosignin&as=${demo}`, wait: 7000, after: async (p) => {
      for (let i = 0; i < 12; i++) {
        if (await p.evaluate(() => !!document.activeElement?.closest('[data-nav-group="sui-shelf"]'))) break;
        await p.keyboard.press("ArrowDown");
        await pause(350);
      }
      await pause(1500);
    } },
  { name: "sui-profile", q: `tab=home&nosignin&as=${demo}&sui=profile`, wait: 9000 },
  { name: "sui-avatar-picker", q: `tab=home&nosignin&as=${demo}&sui=profile`, wait: 9000, after: async (p) => { await click("Change picture")(p); await pause(2500); } },
  { name: "sui-cloud-saves", q: `tab=home&nosignin&as=${demo}&sui=saves`, wait: 8000 },
  { name: "sui-game-owned", q: `tab=games&nosignin&as=${demo}`, wait: 6000, after: async (p) => { await p.evaluate(() => [...document.querySelectorAll("button")].find((b) => b.textContent.includes("Claynosaurz"))?.click()); await pause(4000); } },
];

const only = process.env.ONLY?.split(",").filter(Boolean);
const sizes = (process.env.SIZES ?? "1280x800,1920x1200").split(",").map((s) => s.split("x").map(Number));
const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ["--hide-scrollbars", "--force-device-scale-factor=1"] });
for (const [w, h] of sizes) {
  for (const s of shots) {
    if (only && !only.some((o) => s.name.startsWith(o))) continue;
    const ctx = await browser.createBrowserContext();
    const p = await ctx.newPage();
    p.on("pageerror", (e) => console.error(`${s.name}: ${e.message}`));
    await p.setViewport({ width: w, height: h, deviceScaleFactor: 1 });
    await p.goto(`${base}/?${s.q}`, { waitUntil: "domcontentloaded" });
    await pause(s.wait ?? 4000);
    if (s.after) await s.after(p);
    await pause(600);
    await p.screenshot({ path: `${out}/${s.name}-${w}x${h}.png` });
    console.log(`${s.name}-${w}x${h}`);
    await ctx.close();
  }
}
await browser.close();
