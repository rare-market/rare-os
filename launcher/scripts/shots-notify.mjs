// Screenshots of notifications (toast on Home, the centre in Quick settings, Settings toggles, the
// price alert sheet, the count while a game runs), with the mock notify stream (?notify=mock).
// Usage: npx vite --port 5187 --strictPort (in another shell), then:
//   URL=http://localhost:5187 node scripts/shots-notify.mjs [outDir]
import puppeteer from "puppeteer-core";
import { mkdirSync } from "node:fs";

const out = process.argv[2] ?? "../docs/screenshots";
mkdirSync(out, { recursive: true });
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const base = process.env.URL ?? "http://localhost:5187";
const demo = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const SUI = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const toast = async (p) => {
  await p.waitForSelector("button.pop", { timeout: 20000 });
  await pause(900);
};

const shots = [
  { name: "notify-toast-home", q: `tab=home&nosignin&notify=mock&notifyAt=4000&watch=${demo}`, after: toast },
  {
    name: "notify-centre",
    q: `tab=home&nosignin&notify=mock&notifyAt=1500&watch=${demo}`,
    after: async (p) => {
      await toast(p);
      await pause(6500);
      await p.keyboard.press("Tab");
      await pause(600);
      await p.evaluate(() => document.querySelector("[data-nav-group=qam-notify] button")?.focus());
    },
  },
  {
    name: "notify-settings",
    q: "tab=settings&nosignin&notify=mock&notifyAt=600000",
    after: async (p) => {
      for (const k of ["ArrowDown", "ArrowDown"]) {
        await p.keyboard.press(k);
        await pause(250);
      }
    },
  },
  {
    name: "notify-settings-alerts",
    q: "tab=settings&nosignin&notify=mock&notifyAt=600000",
    after: async (p) => {
      for (const k of ["ArrowDown", "ArrowDown"]) {
        await p.keyboard.press(k);
        await pause(250);
      }
      await pause(600);
      await p.evaluate(() => {
        const b = [...document.querySelectorAll("button")].find((x) => x.textContent?.includes("Delete"));
        b?.focus();
        b?.scrollIntoView({ block: "center" });
      });
    },
  },
  {
    name: "notify-price-alert",
    q: `tab=trade&nosignin&notify=mock&notifyAt=600000&tr=token/${encodeURIComponent(SUI)}`,
    wait: 6000,
    after: async (p) => {
      await p.evaluate(() => [...document.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Price alert")?.click());
      await pause(500);
      await p.keyboard.press("PageDown");
      await p.keyboard.press("PageDown");
    },
  },
  { name: "notify-playing", q: `tab=home&nosignin&notify=mock&notifyAt=1500&running=steam:1145350&watch=${demo}`, wait: 5000 },
];

const only = process.env.ONLY?.split(",").filter(Boolean);
const [w, h] = (process.env.SIZE ?? "1280x800").split("x").map(Number);
const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ["--hide-scrollbars", "--force-device-scale-factor=1"] });
for (const s of shots) {
  if (only && !only.some((o) => s.name.startsWith(o))) continue;
  const ctx = await browser.createBrowserContext();
  const p = await ctx.newPage();
  p.on("pageerror", (e) => console.error(`${s.name}: ${e.message}`));
  p.on("console", (m) => m.text().startsWith("[notify]") && console.log(`${s.name}: ${m.text()}`));
  await p.setViewport({ width: w, height: h, deviceScaleFactor: 1 });
  await p.goto(`${base}/?${s.q}`, { waitUntil: "domcontentloaded", timeout: 60000 }).catch(() => {});
  await pause(s.wait ?? 1500);
  if (s.after) await s.after(p);
  await pause(400);
  const file = `${out}/${s.name}.png`;
  await p.screenshot({ path: file });
  console.log(file);
  await ctx.close();
}
await browser.close();
