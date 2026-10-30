// Docked-mode screenshots: the UI on a 4K TV, a 1080p TV, the internal panel rotated (1920x1200
// logical, what gamescope hands the launcher with ORIENTATION=right) and unrotated portrait
// (1200x1920, what a kernel or session without the rotation would give).
// Usage: npm run dev:ui in another shell, then: node scripts/shots-dock.mjs [outDir]
import puppeteer from "puppeteer-core";
import { mkdirSync } from "node:fs";

const out = process.argv[2] ?? "../docs/screenshots";
mkdirSync(out, { recursive: true });
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const base = process.env.URL ?? "http://localhost:5180";
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const settings = async (p) => {
  await p.evaluate(() => { const b = [...document.querySelectorAll("nav button")].find((x) => x.textContent.startsWith("Display")); b && b.click(); });
  await pause(500);
  await p.evaluate(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent.includes("Larger")); b && b.scrollIntoView({ block: "center" }); });
};
const shots = [
  { name: "dock-4k-home", size: [3840, 2160], q: "tab=home&nosignin&dock=4k" },
  { name: "dock-4k-settings", size: [3840, 2160], q: "tab=settings&nosignin&dock=4k", after: settings },
  { name: "dock-1080p-home", size: [1920, 1080], q: "tab=home&nosignin&dock=1080p" },
  { name: "dock-1080p-settings", size: [1920, 1080], q: "tab=settings&nosignin&dock=1080p", after: settings },
  { name: "dock-1080p-toast", size: [1920, 1080], q: "tab=home&nosignin&dock=1080p&dockin=1", wait: 4500 },
  { name: "dock-internal-settings", size: [1920, 1200], q: "tab=settings&nosignin", after: settings },
  { name: "dock-internal-portrait-home", size: [1200, 1920], q: "tab=home&nosignin" },
  { name: "dock-internal-portrait-settings", size: [1200, 1920], q: "tab=settings&nosignin", after: settings },
];
const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ["--hide-scrollbars", "--force-device-scale-factor=1"] });
for (const s of shots) {
  const ctx = await browser.createBrowserContext();
  const p = await ctx.newPage();
  p.on("pageerror", (e) => console.error(`${s.name}: ${e.message}`));
  await p.setViewport({ width: s.size[0], height: s.size[1], deviceScaleFactor: 1 });
  await p.goto(`${base}/?${s.q}`, { waitUntil: "networkidle2", timeout: 60000 }).catch(() => {});
  await pause(s.wait ?? 2500);
  if (s.after) { await s.after(p); await pause(1200); }
  const file = `${out}/${s.name}-${s.size[0]}x${s.size[1]}.png`;
  await p.screenshot({ path: file });
  console.log(file);
  await ctx.close();
}
await browser.close();
