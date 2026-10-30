// Screenshots of Rare usernames: the first-run step with the picker, add a friend by username,
// the friends list with usernames and SuiNS as a second line, and the profile. Mock friends API
// (?friends=mock) and a watched demo address; availability answers come from the mock.
// Usage: URL=http://localhost:5257 node scripts/shots-username.mjs [outDir]   (npm run dev:ui -- --port 5257 in another shell)
import puppeteer from "puppeteer-core";
import { mkdirSync } from "node:fs";

const out = process.argv[2] ?? "../docs/screenshots";
mkdirSync(out, { recursive: true });
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const base = process.env.URL ?? "http://localhost:5257";
const demo = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const press = async (p, keys) => {
  for (const k of keys) {
    await p.keyboard.press(k);
    await pause(80);
  }
};
const click = (text) => (p) => p.evaluate((t) => [...document.querySelectorAll("button")].find((x) => x.textContent.trim().startsWith(t))?.click(), text);
const focusText = (text) => (p) =>
  p.evaluate((t) => {
    const b = [...document.querySelectorAll("button[data-nav]")].find((x) => x.textContent.trim().startsWith(t));
    b?.focus();
    b?.scrollIntoView({ block: "center" });
  }, text);

const shots = [
  {
    name: "username-firstrun",
    q: `tab=home&firstrun&step=2&named&devsign&friends=mock&watch=${demo}`,
    wait: 3500,
    after: async (p) => {
      await click("Change")(p);
      await pause(500);
      await p.keyboard.type("mira_plays", { delay: 30 });
      await pause(1200);
    },
  },
  {
    name: "username-add-friend",
    q: `tab=friends&nosignin&friends=mock&watch=${demo}`,
    wait: 3000,
    after: async (p) => {
      await click("Add a friend")(p);
      await pause(500);
      await click("Username, name.sui")(p);
      await pause(500);
      await p.keyboard.type("@Mira", { delay: 30 });
      await pause(1200);
    },
  },
  { name: "username-friends", q: `tab=friends&nosignin&friends=mock&watch=${demo}`, wait: 3000, after: focusText("mira") },
  { name: "username-profile", q: `tab=home&nosignin&sui=profile&as=${demo}&username=null_dev&friends=mock`, wait: 6000 },
];
const [w, h] = (process.env.SIZE ?? "1280x800").split("x").map(Number);
const only = process.env.ONLY?.split(",").filter(Boolean);
const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ["--hide-scrollbars", "--force-device-scale-factor=1"] });
try {
  for (const s of shots) {
    if (only && !only.some((o) => s.name.startsWith(o))) continue;
    const ctx = await browser.createBrowserContext();
    const p = await ctx.newPage();
    p.on("pageerror", (e) => console.error(`${s.name}: ${e.message}`));
    await p.setViewport({ width: w, height: h, deviceScaleFactor: 1 });
    await p.goto(`${base}/?${s.q}`, { waitUntil: "networkidle2", timeout: 60000 }).catch(() => {});
    await pause(s.wait ?? 2500);
    if (s.after) {
      await s.after(p);
      await pause(1200);
    }
    await p.screenshot({ path: `${out}/${s.name}.png` });
    console.log(`${out}/${s.name}.png`);
    await ctx.close();
  }
} finally {
  await browser.close();
}
