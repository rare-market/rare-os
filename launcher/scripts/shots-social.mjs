// Top traders, the trader page, Mirror, and Rareling perks, with live data (browser mode, the Vite
// proxy reaches the Trade API). Mirror runs on the watch-only demo wallet: the review builds and
// dry-runs the swap, and a watch account can never sign, so nothing is sent.
// Usage: npm run dev:ui (in another shell), then: URL=http://localhost:5191 node scripts/shots-social.mjs [outDir]
// ONLY=traders-page shoots one; SIZES=1920x1200 one size.
import puppeteer from "puppeteer-core";
import { mkdirSync } from "node:fs";

const out = process.argv[2] ?? "../docs/screenshots";
mkdirSync(out, { recursive: true });
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const base = process.env.URL ?? "http://localhost:5180";
const demo = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
// The demo wallet holds Popkins in a kiosk: standing in for the Rareling type until it is published.
const STAND_IN = "0xb908f3c6fea6865d32e2048c520cdfe3b5c5bbcebb658117c41bad70f52b7ccc::popkins_nft::Popkins";
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const q = (tr) => `tab=trade&nosignin&watch=${demo}${tr ? `&tr=${tr}` : ""}`;
const click = (text) => (p) => p.evaluate((t) => [...document.querySelectorAll("button[data-nav]")].find((b) => b.textContent.trim() === t)?.click(), text);

// The trader page needs a trader: the top of the 7 day board.
async function topTrader() {
  if (process.env.TRADER) return process.env.TRADER;
  const r = await fetch("https://rare.market/v1/trade/leaderboard?window=7d");
  const j = await r.json();
  return j.rows.find((x) => x.trades >= 10 && x.volumeUsd >= 1000)?.user.address ?? j.rows[0].user.address;
}
const trader = await topTrader();

const follows = JSON.stringify([{ address: trader, name: `${trader.slice(0, 6)}...${trader.slice(-4)}`, since: Date.now() }]);
const shots = [
  { name: "traders-top", q: q("traders"), wait: 8000 },
  { name: "traders-winrate", q: q("traders"), wait: 8000, after: click("Win rate") },
  { name: "traders-following", q: q("traders"), wait: 8000, storage: { [`rare.traders.follow.${demo}`]: follows } },
  { name: "traders-page", q: q(`traders,trader/${trader}`), wait: 9000 },
  {
    name: "traders-mirror",
    q: q(`traders,trader/${trader}`),
    wait: 9000,
    after: async (p) => {
      await p.evaluate(() => document.querySelector("[data-nav-group^=tr-trader-trades] button")?.click());
      await pause(9000);
    },
  },
  { name: "perks-locked", q: `tab=home&nosignin&watch=${demo}&sui=profile`, wait: 7000 },
  { name: "perks-unlocked", q: `tab=home&nosignin&watch=${demo}&sui=profile`, wait: 8000, storage: { "rare.perks.rarelingType": STAND_IN } },
  { name: "perks-theme", q: `tab=home&nosignin&watch=${demo}&sui=profile`, wait: 8000, storage: { "rare.perks.rarelingType": STAND_IN, [`rare.perks.theme.${demo}`]: "royal" } },
];

const only = process.env.ONLY?.split(",").filter(Boolean);
const sizes = (process.env.SIZES ?? "1280x800,1920x1200").split(",").map((s) => s.split("x").map(Number));
const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ["--hide-scrollbars", "--force-device-scale-factor=1"] });
try {
  for (const [w, h] of sizes) {
    for (const s of shots) {
      if (only && !only.some((o) => s.name === o)) continue;
      const ctx = await browser.createBrowserContext();
      const p = await ctx.newPage();
      p.on("pageerror", (e) => console.error(`${s.name}: ${e.message}`));
      await p.setViewport({ width: w, height: h, deviceScaleFactor: 1 });
      if (s.storage) {
        await p.goto(`${base}/?tab=home&nosignin`, { waitUntil: "domcontentloaded", timeout: 60000 }).catch(() => {});
        await p.evaluate((kv) => Object.entries(kv).forEach(([k, v]) => localStorage.setItem(k, v)), s.storage);
      }
      await p.goto(`${base}/?${s.q}`, { waitUntil: "networkidle2", timeout: 60000 }).catch(() => {});
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
