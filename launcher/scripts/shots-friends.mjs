// Screenshots of Friends (the list, a friend playing, a request, add by name or QR, privacy, a
// party with chat, the invite picker), Home's party and "Friends playing" row, and a party invite
// notice, with the mock friends API (?friends=mock) at both sizes.
// Usage: npm run dev:ui -- --port 5197 (in another shell), then:
//   URL=http://localhost:5197 node scripts/shots-friends.mjs [outDir]
// ONLY=friends-party shoots one; SIZES=1920x1200 one size.
import puppeteer from "puppeteer-core";
import { mkdirSync } from "node:fs";

const out = process.argv[2] ?? "../docs/screenshots";
mkdirSync(out, { recursive: true });
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const base = process.env.URL ?? "http://localhost:5197";
const demo = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const q = (extra = "") => `tab=friends&nosignin&friends=mock&watch=${demo}${extra}`;

/** Focuses the list row (or any button) whose text starts with `text`. */
const focusText = (text, sel = "button[data-nav]") => (p) =>
  p.evaluate(
    (t, s) => {
      const b = [...document.querySelectorAll(s)].find((x) => x.textContent.trim().startsWith(t));
      b?.focus();
      b?.scrollIntoView({ block: "center" });
    },
    text,
    sel,
  );
const click = (text) => (p) => p.evaluate((t) => [...document.querySelectorAll("button[data-nav]")].find((x) => x.textContent.trim().startsWith(t))?.click(), text);

const shots = [
  { name: "friends-list", q: q(), after: focusText("mira.sui") },
  { name: "friends-steam", q: q(), after: focusText("kenji.sui") },
  { name: "friends-request", q: q(), after: focusText("dune.sui") },
  { name: "friends-add", q: q(), after: focusText("Add a friend") },
  { name: "friends-privacy", q: q(), after: focusText("Privacy") },
  {
    name: "friends-party",
    q: q("&party"),
    after: async (p) => {
      await focusText("mira.sui's party")(p);
      await pause(800);
    },
  },
  {
    name: "friends-invite",
    q: q("&party"),
    after: async (p) => {
      await focusText("mira.sui's party")(p);
      await pause(500);
      await click("Invite friends")(p);
      await pause(500);
      await p.evaluate(() => {
        const rows = [...document.querySelectorAll("[data-nav-group=friends-detail] button[data-nav]")].filter((x) => /kenji|lowkey/.test(x.textContent));
        rows.forEach((r) => r.click());
        rows[0]?.focus();
      });
    },
  },
  {
    name: "friends-home",
    q: `tab=home&nosignin&friends=mock&party&watch=${demo}`,
    wait: 3500,
    after: async (p) => {
      await p.evaluate(() => {
        const b = document.querySelector("[data-friend-join]");
        b?.focus();
        b?.closest("section")?.scrollIntoView({ block: "center" });
      });
      await pause(700);
    },
  },
  {
    name: "friends-invite-notice",
    q: `tab=home&nosignin&friends=mock&friendsping&watch=${demo}`,
    wait: 1500,
    after: async (p) => {
      await p.waitForSelector("button.pop", { timeout: 20000 }).catch(() => {});
      await pause(900);
    },
  },
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
    await p.goto(`${base}/?${s.q}`, { waitUntil: "domcontentloaded", timeout: 60000 }).catch(() => {});
    await pause(s.wait ?? 2500);
    if (s.after) await s.after(p);
    await pause(700);
    const file = `${out}/${s.name}-${w}x${h}.png`;
    await p.screenshot({ path: file });
    console.log(file);
    await ctx.close();
  }
}
await browser.close();
