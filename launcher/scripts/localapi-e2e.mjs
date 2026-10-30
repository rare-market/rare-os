// End to end, in mock mode: a sample web game page calls `sell` through rare:platform, and the
// launcher shows its sell sheet over the game and then Rare Wallet's approval (review) sheet.
// Nothing is signed: the review is cancelled, the sheet closed, and the game must get
// { status: "cancelled" }.
//
// Two headless pages stand in for the device:
//   - the launcher UI (Vite dev server, browser mode: ?devsign lets the demo watch account reach the
//     review sheet; rare.market data is live through the dev proxy);
//   - the sample game (scripts/fixtures/localapi-game.html) on another origin, with Rare's real
//     runtime (src-tauri/src/webgame.js) injected the way the launcher injects it. Its rare-wallet://
//     requests are relayed to the launcher page's window.__rareLocalApiDev, standing in for Rust.
// Silent by rule: Chromium runs with --mute-audio and AudioContext is stubbed in both pages.
//
// Usage: node scripts/localapi-e2e.mjs        (exit 0 on pass)
// Env: HOLDER (a wallet that holds the game's items), GAME (library id, default web:pawtato-land),
//      CHROMIUM (browser binary). Always headless.
import { chromium } from "playwright-core";
import { createServer as createHttp } from "node:http";
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer as createVite } from "vite";

const root = fileURLToPath(new URL("..", import.meta.url));
const HOLDER = process.env.HOLDER ?? "0x7aa0c2206ef649591f9b12db3baa4572e01a30727b949f7749fc50e08f5ad784";
const GAME = process.env.GAME ?? "web:pawtato-land";
const runtime = readFileSync(join(root, "src-tauri/src/webgame.js"), "utf8");
const gameHtml = readFileSync(join(root, "scripts/fixtures/localapi-game.html"), "utf8");

/** Headless Chrome (installed) or Playwright's Chromium; always headless, always muted. */
function chromiumPath() {
  if (process.env.CHROMIUM) return process.env.CHROMIUM;
  const chrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
  if (existsSync(chrome)) return chrome;
  const base = join(homedir(), "Library/Caches/ms-playwright");
  const dirs = existsSync(base) ? readdirSync(base).filter((d) => d.startsWith("chromium-")).sort().reverse() : [];
  for (const d of dirs) {
    const p = join(base, d, "chrome-mac/Chromium.app/Contents/MacOS/Chromium");
    if (existsSync(p)) return p;
  }
  return undefined;
}

const SILENT = `(() => {
  const Silent = function () { return { close() {}, resume() { return Promise.resolve(); }, suspend() { return Promise.resolve(); }, createOscillator() { return { connect() {}, start() {}, stop() {}, frequency: { setValueAtTime() {} } }; }, createGain() { return { connect() {}, gain: { setValueAtTime() {}, linearRampToValueAtTime() {}, exponentialRampToValueAtTime() {} } }; }, destination: {}, currentTime: 0, state: "suspended" }; };
  window.AudioContext = Silent; window.webkitAudioContext = Silent;
  HTMLMediaElement.prototype.play = function () { return Promise.resolve(); };
})();`;

const fail = (m) => {
  console.error(`FAIL ${m}`);
  process.exitCode = 1;
};

const vite = await createVite({ root, configFile: join(root, "vite.config.ts"), server: { port: 0, strictPort: false, host: "127.0.0.1" }, logLevel: "error", clearScreen: false });
await vite.listen();
const launcherUrl = `http://127.0.0.1:${vite.httpServer.address().port}`;
const gameServer = createHttp((_req, res) => {
  res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  res.end(gameHtml);
});
await new Promise((r) => gameServer.listen(0, "localhost", r));
// Another origin than the launcher's (localhost vs 127.0.0.1), as a real web game is.
const gameUrl = `http://localhost:${gameServer.address().port}/`;

const browser = await chromium.launch({ executablePath: chromiumPath(), headless: true, args: ["--mute-audio", "--autoplay-policy=user-gesture-required"] });
try {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  await ctx.addInitScript(SILENT);
  const launcher = await ctx.newPage();
  launcher.on("pageerror", (e) => console.error("[launcher]", e.message));
  await launcher.goto(`${launcherUrl}/?tab=home&nosignin&devsign&watch=${HOLDER}&running=${encodeURIComponent(GAME)}`);
  await launcher.waitForFunction(() => !!window.__rareLocalApiDev, null, { timeout: 30_000 });
  console.log("launcher up (mock mode)");

  const game = await ctx.newPage();
  const lines = [];
  game.on("console", (m) => m.type() === "error" && console.error("[game]", m.text()));
  // The game's runtime, as webgame.rs injects it: profile and this launch's API first, then webgame.js.
  // __rareTransport (test only, set before the runtime) carries rare-wallet:// requests to us.
  await game.exposeBinding("__e2eRelay", async (_src, raw) => {
    const msg = JSON.parse(raw);
    if (msg.method !== "platform") return;
    const { call, args } = msg.params;
    const r = await launcher.evaluate(([g, c, a]) => window.__rareLocalApiDev.call(g, c, a), [GAME, call, args]);
    await game.evaluate(([id, ok, v]) => window.__rareWalletReply(id, ok, v), [msg.id, r.ok, r.ok ? r.value : r.value?.message ?? String(r.value)]);
  });
  await game.addInitScript(
    `window.__rareProfile = {}; window.__rareApi = ${JSON.stringify({ url: null, token: "e2e", game: GAME.replace(/^web:/, "") })};` +
      `window.__rareTransport = (m) => window.__e2eRelay(JSON.stringify(m));\n` +
      runtime,
  );
  await game.goto(gameUrl);
  const log = () => game.evaluate(() => document.getElementById("log").textContent);

  // 1. The game finds rare:platform and reads the player and this game's items.
  await game.waitForFunction(() => /items \d+|no items|error/.test(document.getElementById("log").textContent), null, { timeout: 60_000 });
  console.log((await log()).trim().split("\n").map((l) => `  game: ${l}`).join("\n"));
  if (!/game \S+/.test(await log())) fail("rare:platform was not found in the injected wallet");
  if (/no items|error/.test(await log())) throw new Error("the holder has no items from this game; set HOLDER");

  // 2. sell: the launcher shows the sell sheet over the game (overlay backdrop, the game's name).
  const sellSheet = launcher.locator('[role=dialog]').filter({ hasText: /asked Rare/ });
  await sellSheet.waitFor({ timeout: 60_000 });
  console.log(`sell sheet: "${(await sellSheet.locator("h2").first().textContent())?.trim()}"`);

  // 3. The player picks List: Rare Wallet's approval sheet comes up with the transaction to review.
  const list = sellSheet.getByRole("button", { name: /^(Re)?[Ll]ist for / });
  await list.waitFor({ timeout: 30_000 });
  await list.click();
  const review = launcher.locator('[role=dialog]').filter({ hasText: /Rare Marketplace ·/ });
  await review.waitFor({ timeout: 60_000 });
  const reviewTitle = (await review.locator("h2").first().textContent())?.trim();
  console.log(`approval sheet: "${reviewTitle}"`);
  if (!/^List /.test(reviewTitle ?? "")) fail(`approval sheet title was "${reviewTitle}"`);
  const approve = review.getByRole("button", { name: /^(Approve|Checking)$/ });
  if (!(await approve.count())) fail("approval sheet has no Approve button");

  // 4. Nothing gets signed: Cancel the review, then B closes the sell sheet; the game hears "cancelled".
  await review.getByRole("button", { name: "Cancel" }).click();
  await review.waitFor({ state: "detached", timeout: 10_000 });
  await launcher.keyboard.press("Escape");
  await game.waitForFunction(() => /sell \S+/.test(document.getElementById("log").textContent), null, { timeout: 20_000 });
  const last = (await log()).trim().split("\n").pop();
  console.log(`  game: ${last}`);
  if (last !== "sell cancelled") fail(`the game got "${last}"`);
  if (await launcher.evaluate(() => window.__rareLocalApiDev.sheet())) fail("the sheet is still open");
  if (!process.exitCode) console.log("PASS sell from a web game shows the sell sheet, then the approval sheet; cancelling answers cancelled");
} catch (e) {
  fail(e?.message ?? String(e));
} finally {
  await browser.close();
  gameServer.close();
  await vite.close();
}
