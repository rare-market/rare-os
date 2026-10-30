// Staked items in the Wallet inventory (mock data, ?inv=mock): the Staked label and "Unstake on <project> to list or
// send" line on the card, no list/send in the sheet, and selecting does not pick a staked item. Silent: headless
// shell browser (never the player's Chrome), --mute-audio, AudioContext stubbed.
//   npm run dev (port 1420 by default) then: node scripts/shots-staked.mjs [baseUrl]
import { mkdirSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import assert from "node:assert/strict";
import puppeteer from "puppeteer-core";

const BASE = process.argv[2] || "http://localhost:1420";
const OUT = new URL("../../docs/shots/staked/", import.meta.url).pathname;
mkdirSync(OUT, { recursive: true });
const root = `${homedir()}/.cache/puppeteer/chrome-headless-shell`;
const exe = process.env.CHROMIUM || `${root}/${readdirSync(root).sort().pop()}/chrome-headless-shell-mac-arm64/chrome-headless-shell`;
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const watch = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

const browser = await puppeteer.launch({ executablePath: exe, headless: "shell", args: ["--mute-audio", "--hide-scrollbars"] });
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 800 });
  await page.evaluateOnNewDocument(() => {
    const C = class { constructor() { this.state = "suspended"; this.destination = {}; } resume() { return Promise.resolve(); } close() { return Promise.resolve(); } createGain() { return { gain: { value: 0, setValueAtTime() {}, linearRampToValueAtTime() {} }, connect() {}, disconnect() {} }; } createOscillator() { return { frequency: { value: 0, setValueAtTime() {} }, connect() {}, start() {}, stop() {}, disconnect() {} }; } };
    window.AudioContext = C;
    window.webkitAudioContext = C;
  });
  const click = (text) => page.evaluate((t) => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent.trim().startsWith(t)); b?.click(); return !!b; }, text);
  await page.goto(`${BASE}/?tab=wallet&nosignin&watch=${watch}&devsign&inv=mock`, { waitUntil: "networkidle2" });
  await pause(1500);
  await click("Collectibles");
  await page.waitForSelector('[data-testid="staked-hint"]', { timeout: 15000 });
  const hints = await page.$$eval('[data-testid="staked-hint"]', (n) => n.map((x) => x.textContent));
  assert.deepEqual(hints, ["Unstake on Popkins to list or send"]);
  assert.match(await page.evaluate(() => document.body.innerText), /Staked/);
  await page.evaluate(() => document.querySelector('[data-testid="staked-hint"]').scrollIntoView({ block: "center" }));
  await pause(300);
  await page.screenshot({ path: `${OUT}inventory.png` });

  // Selecting: the staked card is not picked.
  await click("Select");
  await pause(300);
  await page.evaluate(() => { for (const b of document.querySelectorAll("[data-nav]")) if (/Popkins #77|#77/.test(b.textContent)) b.click(); });
  await pause(300);
  assert.match(await page.evaluate(() => document.body.innerText), /Pick items to send/, "a staked item is not selected");
  await click("Done");

  // The sheet: no sell, list or send, just the line.
  await page.evaluate(() => { for (const b of document.querySelectorAll("[data-nav]")) if (/#77/.test(b.textContent)) b.click(); });
  await pause(1500);
  const sheet = await page.evaluate(() => document.body.innerText);
  assert.match(sheet, /Unstake on Popkins to list or send/);
  assert.doesNotMatch(sheet, /List for sale|Sell now|Cancel listing|Pick a price/, "no list or sell on a staked item");
  const buttons = await page.$$eval("[role=dialog] button, [data-sheet] button", (n) => n.map((x) => x.textContent.trim()));
  assert.ok(!buttons.includes("Send"), "no send in the sheet");
  await page.screenshot({ path: `${OUT}sheet.png` });
  console.log("ok: staked card, selection, sheet");
} finally {
  await browser.close();
}
