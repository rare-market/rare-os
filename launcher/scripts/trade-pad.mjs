// Scripted pad run for the Trade tab in the dev UI: presses buttons through window.__pad (the same
// dispatch the controller uses), checks what changed, and takes screenshots of search, perp icons
// and chart mode. Usage: npm run dev:ui -- --port 5192, then: URL=http://localhost:5192 node scripts/trade-pad.mjs
import puppeteer from "puppeteer-core";
import { mkdirSync } from "node:fs";

const out = process.argv[2] ?? "../docs/screenshots";
mkdirSync(out, { recursive: true });
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const base = process.env.URL ?? "http://localhost:5192";
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const SUI = "0x2::sui::SUI";
let fails = 0;
const check = (name, ok, extra = "") => {
  console.log(`${ok ? "ok  " : "FAIL"} ${name} ${extra}`);
  if (!ok) fails++;
};

const browser = await puppeteer.launch({ executablePath: chrome, headless: "new", args: ["--no-sandbox"] });
for (const [w, h] of [[1920, 1200], [1280, 800]]) {
  const page = await browser.newPage();
  await page.setViewport({ width: w, height: h });
  const pad = async (b, n = 1) => {
    for (let i = 0; i < n; i++) {
      await page.evaluate((x) => window.__pad(x), b);
      await pause(120);
    }
  };
  const shot = (name) => page.screenshot({ path: `${out}/${name}-${w}x${h}.png` });
  const focused = () => page.evaluate(() => document.activeElement?.getAttribute("data-hint-a") || document.activeElement?.textContent?.trim().slice(0, 30) || "");
  const text = (sel) => page.evaluate((s) => document.querySelector(s)?.textContent ?? "", sel);
  const logos = () => page.evaluate(() => [...document.querySelectorAll("[data-nav-group] img")].filter((i) => i.complete && i.naturalWidth > 0).length);

  // Perp icons: markets list.
  await page.goto(`${base}/?tab=trade&nosignin&tr=perps`, { waitUntil: "networkidle2" });
  await pause(5000);
  const n = await logos();
  check(`[${w}] perp icons loaded`, n >= 5, `(${n})`);
  await shot("trade-perps-list");
  await page.goto(`${base}/?tab=trade&nosignin&tr=home`, { waitUntil: "networkidle2" });
  await pause(5000);
  await shot("trade-perps-home");

  // Search from Trade home with Y.
  await pad("y");
  await pause(500);
  // Type "deep" on the on-screen keyboard with the pad: q row 1 col 0, a row 2, z row 3.
  let at = [1, 0];
  const key = async (r, c) => {
    while (at[0] < r) (await pad("down"), at[0]++);
    while (at[0] > r) (await pad("up"), at[0]--);
    while (at[1] < c) (await pad("right"), at[1]++);
    while (at[1] > c) (await pad("left"), at[1]--);
    await pad("a");
  };
  await key(2, 2); // d
  await key(1, 2); // e
  await key(1, 2); // e
  await key(1, 9); // p
  await pad("start");
  await pause(3500);
  const rows = await page.evaluate(() => document.querySelectorAll("[data-nav-group=search-coins] [data-nav]").length);
  check(`[${w}] search shows coins`, rows > 0, `(${rows})`);
  await shot("trade-search-results");
  await pad("b");
  await pause(300);

  // Token page: side switch, amount stepper, chart mode.
  await page.goto(`${base}/?tab=trade&nosignin&tr=token/${encodeURIComponent(SUI)}`, { waitUntil: "networkidle2" });
  await pause(5000);
  const side = () => page.evaluate(() => document.querySelector("[data-nav-group=tr-side] [aria-checked=true]")?.textContent ?? "");
  await pad("x");
  check(`[${w}] X flips Buy to Sell`, (await side()).includes("Sell"), `(${await side()})`);
  await shot("trade-controls-token");
  // Reach the Buy/Sell switch with the D-pad and press A.
  for (let i = 0; i < 8 && !(await focused()).startsWith("Switch to"); i++) await pad("up");
  check(`[${w}] D-pad reaches the Buy/Sell switch`, (await focused()).startsWith("Switch to"), await focused());
  await pad("a");
  check(`[${w}] A on the switch changes side`, (await side()).includes("Buy"), await side());
  // Chart mode.
  await page.evaluate(() => document.querySelector("[data-area=chart]")?.focus());
  await pause(200);
  await pad("a");
  await pause(600);
  for (let i = 0; i < 20; i++) await pad("left");
  await pad("up", 2);
  await page.evaluate(() => window.__stick(0.8, 0));
  await pause(500);
  await page.evaluate(() => window.__stick(0, 0));
  await pad("y");
  await pause(500);
  await shot("trade-chart-mode");
  const readout = await page.evaluate(() => document.querySelector("[data-area=chart]")?.parentElement?.textContent ?? "");
  check(`[${w}] chart readout shows OHLC`, /O \$/.test(readout) && /C \$/.test(readout));
  await pad("rt");
  await pause(1500);
  await pad("x");
  await pad("a");
  await pause(500);
  await shot("trade-chart-mode-indicators");
  await pad("b");
  check(`[${w}] B leaves chart mode`, !(await page.evaluate(() => !!document.querySelector(".ring-1.ring-border-light"))));
  await page.close();
}
await browser.close();
process.exit(fails ? 1 : 0);
