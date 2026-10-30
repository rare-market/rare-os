// Trade tab screenshots with live data (browser mode, the Vite proxy reaches the Trade API).
// Usage: npm run dev:ui (in another shell), then: node scripts/shots-trade.mjs [outDir]
// ONLY=trade-token shoots one; SIZES=1920x1200 one size.
import puppeteer from "puppeteer-core";
import { mkdirSync } from "node:fs";

const out = process.argv[2] ?? "../docs/screenshots";
mkdirSync(out, { recursive: true });
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const base = process.env.URL ?? "http://localhost:5180";
// The same watch-only demo wallet as the other screenshots.
const demo = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const DEEP = "0xdeeb7a4662eec9f2f3def03fb937a663dddaa2e215b8078a284d026b7946c270::deep::DEEP";
// BTC/USD perp.
const PERP = process.env.PERP ?? "";
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const q = (tr) => `tab=trade&nosignin&watch=${demo}${tr ? `&tr=${tr}` : ""}`;

const shots = [
  { name: "trade-home", q: q(""), wait: 7000 },
  { name: "trade-token", q: q(`token/${encodeURIComponent(DEEP)}`), wait: 9000 },
  { name: "trade-token-sell", q: q(`sell/${encodeURIComponent(DEEP)}`), wait: 9000, after: async (p) => { await p.keyboard.press("PageDown"); } },
  { name: "trade-review", q: q(`token/${encodeURIComponent(DEEP)}`), wait: 9000, after: async (p) => { await p.keyboard.press("Enter"); await pause(7000); } },
  // Confirm hands the trade to Rare Wallet's review sheet. ?devsign: the watch account "signs" with
  // a dummy and the wallet only simulates (never submits).
  { name: "trade-sign", q: `${q(`token/${encodeURIComponent(DEEP)}`)}&devsign`, wait: 9000, after: async (p) => { await p.keyboard.press("Enter"); await pause(7000); await p.keyboard.press("Enter"); await pause(6000); } },
  { name: "trade-swap", q: q("swap"), wait: 8000 },
  { name: "trade-perps", q: q("perps"), wait: 6000 },
  { name: "trade-perp", q: null, wait: 9000 },
  { name: "trade-perp-review", q: null, wait: 9000, after: async (p) => { await p.keyboard.press("PageDown"); await pause(1500); await p.keyboard.press("Enter"); await pause(7000); } },
  { name: "trade-positions", q: q("positions"), wait: 7000 },
];

// The perp market page needs a market id: the biggest market by volume.
async function perpId() {
  if (PERP) return PERP;
  const r = await fetch("https://rare.market/v1/trade/ext/perps/markets");
  const j = await r.json();
  const btc = j.items.find((m) => m.symbol === "BTC");
  return (btc ?? [...j.items].sort((a, b) => (b.volumeUsd ?? 0) - (a.volumeUsd ?? 0))[0]).marketId;
}

const only = process.env.ONLY?.split(",").filter(Boolean);
const sizes = (process.env.SIZES ?? "1280x800,1920x1200").split(",").map((s) => s.split("x").map(Number));
const market = await perpId();
const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ["--hide-scrollbars", "--force-device-scale-factor=1"] });
try {
  for (const [w, h] of sizes) {
    for (const s of shots) {
      if (only && !only.some((o) => s.name === o)) continue;
      const ctx = await browser.createBrowserContext();
      const p = await ctx.newPage();
      p.on("pageerror", (e) => console.error(`${s.name}: ${e.message}`));
      await p.setViewport({ width: w, height: h, deviceScaleFactor: 1 });
      await p.goto(`${base}/?${s.q ?? q(`perp/${market}`)}`, { waitUntil: "networkidle2", timeout: 60000 }).catch(() => {});
      await pause(s.wait ?? 4000);
      if (s.after) {
        await s.after(p);
        await pause(1500);
      }
      const file = `${out}/${s.name}-${w}x${h}.png`;
      await p.screenshot({ path: file });
      console.log(file);
      await ctx.close();
    }
  }
} finally {
  await browser.close();
}
