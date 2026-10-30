// Screenshots of the launcher UI (browser mode, mock native side) at handheld resolutions.
// Usage: npm run dev:ui (in another shell), then: node scripts/shots.mjs [outDir]
import puppeteer from "puppeteer-core";
import { mkdirSync, readdirSync } from "node:fs";

const out = process.argv[2] ?? "../docs/screenshots";
mkdirSync(out, { recursive: true });
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const base = process.env.URL ?? "http://localhost:5180";
const demo = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
// A listed Popkins item for the marketplace item and buy shots (listings move: pick a new one if it sells).
const item = process.env.ITEM ?? "0x89d249c6a43ff4490902247a950e2e69ba61b74089151166169cfe3fbfea8611";
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
// Real holders for the in-game overlay shots (items, offers, listings move: pick others if they sell out).
const pawtatoHolder = process.env.PAWTATO ?? "0x7aa0c2206ef649591f9b12db3baa4572e01a30727b949f7749fc50e08f5ad784";
const warpedHolder = process.env.WARPED ?? "0xdb5e5c9e796385d324cedf2fd0d72b31570535cc7e13a2ad3b66a35b60221c01";

const shots = [
  { name: "home", q: "tab=home&nosignin" },
  { name: "home-wallet", q: `tab=home&nosignin&watch=${demo}`, wait: 6000 },
  { name: "home-shelf", q: "tab=home&nosignin", after: async (p) => { for (const k of ["ArrowRight", "ArrowRight"]) { await p.keyboard.press(k); await pause(250); } } },
  { name: "home-market", q: "tab=home&nosignin", wait: 4000, after: async (p) => { for (const k of ["ArrowDown", "ArrowDown"]) { await p.keyboard.press(k); await pause(350); } } },
  { name: "library", q: "tab=library&nosignin" },
  { name: "game", q: "tab=home&nosignin", keys: ["Enter"], after: async (p) => { await p.keyboard.press("ArrowRight"); await p.keyboard.press("Enter"); } },
  { name: "anticheat", q: "tab=library&nosignin", after: async (p) => { await p.evaluate(() => { const b = [...document.querySelectorAll("button")].find((x) => (x.dataset.title ?? x.textContent).includes("PUBG")); b && b.click(); }); } },
  { name: "search", q: "tab=home&nosignin", after: async (p) => { await p.keyboard.press("y"); await new Promise((r) => setTimeout(r, 400)); for (const k of "ha") await p.keyboard.press(k); await p.keyboard.press("Tab"); } },
  { name: "sui-games", q: "tab=games&nosignin" },
  // Rare Store (src/store): live rare.market data, and with storedemo the demo titles from src/store/mock.ts
  // (storedemo=sell shows deployed package ids, =both also owns a licence).
  { name: "store", q: "tab=store&nosignin&storedemo", wait: 3500 },
  { name: "store-live", q: "tab=store&nosignin", wait: 3500 },
  { name: "store-search", q: "tab=store&nosignin&storedemo&storeq=orchard", wait: 3500 },
  { name: "store-game-link", q: "tab=store&nosignin&storegame=act", wait: 3500 },
  { name: "store-game-coming", q: "tab=store&nosignin&storedemo&storegame=driftline", wait: 3500 },
  { name: "store-game-buy", q: `tab=store&nosignin&storedemo=sell&watch=${demo}&storegame=hollow-keep`, wait: 5000 },
  { name: "store-game-owned", q: `tab=store&nosignin&storedemo=both&watch=${demo}&storegame=driftline`, wait: 5000 },
  { name: "store-library", q: `tab=library&nosignin&storedemo=both&watch=${demo}`, wait: 5000 },
  { name: "sui-game-page", q: "tab=games&nosignin", wait: 3500, after: async (p) => { await p.keyboard.press("Enter"); } },
  { name: "web-game-page", q: "tab=library&nosignin", after: async (p) => { await p.evaluate(() => { const b = [...document.querySelectorAll("button")].find((x) => (x.dataset.title ?? x.textContent).includes("ACT")); b && b.click(); }); } },
  { name: "wallet", q: `tab=wallet&nosignin&watch=${demo}`, wait: 6000 },
  { name: "wallet-onboarding", q: "tab=wallet&nosignin", fresh: true },
  // Native Rare Marketplace, live rare.market data through the Vite proxy.
  { name: "marketplace-home", q: `tab=wallet&nosignin&watch=${demo}`, wait: 5000, after: async (p) => { await p.keyboard.press("e"); await pause(4000); } },
  { name: "marketplace-collection", q: "tab=market&nosignin&mk=collection/popkins", wait: 6000 },
  { name: "marketplace-filters", q: "tab=market&nosignin&mk=collection/popkins", after: async (p) => { await p.keyboard.press("x"); } },
  { name: "marketplace-activity", q: "tab=market&nosignin&mk=collection/popkins", after: async (p) => { await p.keyboard.press("e"); } },
  { name: "marketplace-holders", q: "tab=market&nosignin&mk=collection/popkins", after: async (p) => { await p.keyboard.press("e"); await pause(300); await p.keyboard.press("e"); } },
  { name: "marketplace-item", q: `tab=market&nosignin&mk=collection/popkins,item/${item}`, wait: 6000 },
  { name: "marketplace-buy", q: `tab=market&nosignin&mk=item/${item}`, after: async (p) => { await p.keyboard.press("Enter"); } },
  { name: "marketplace-search", q: "tab=market&nosignin&mk=search/pop" },
  { name: "quick-settings", q: "tab=home&nosignin", after: async (p) => { await p.keyboard.press("Tab"); } },
  { name: "keyboard", q: "tab=wallet&nosignin&osk" },
  { name: "settings-system", q: "tab=settings&nosignin", wait: 700, after: async (p) => { for (let i = 0; i < 0; i++) { await p.keyboard.press("ArrowDown"); await pause(120); } await pause(900); } },
  { name: "settings-display", q: "tab=settings&nosignin", wait: 700, after: async (p) => { for (let i = 0; i < 1; i++) { await p.keyboard.press("ArrowDown"); await pause(120); } await pause(900); } },
  { name: "settings-sound", q: "tab=settings&nosignin", wait: 700, after: async (p) => { for (let i = 0; i < 2; i++) { await p.keyboard.press("ArrowDown"); await pause(120); } await pause(900); } },
  { name: "settings-controls", q: "tab=settings&nosignin", wait: 700, after: async (p) => { for (let i = 0; i < 3; i++) { await p.keyboard.press("ArrowDown"); await pause(120); } await pause(900); } },
  { name: "settings-power", q: "tab=settings&nosignin", wait: 700, after: async (p) => { for (let i = 0; i < 4; i++) { await p.keyboard.press("ArrowDown"); await pause(120); } await pause(900); } },
  { name: "settings-network", q: "tab=settings&nosignin", wait: 700, after: async (p) => { for (let i = 0; i < 5; i++) { await p.keyboard.press("ArrowDown"); await pause(120); } await pause(900); } },
  { name: "settings-accounts", q: "tab=settings&nosignin", wait: 700, after: async (p) => { for (let i = 0; i < 6; i++) { await p.keyboard.press("ArrowDown"); await pause(120); } await pause(900); } },
  { name: "settings-storage", q: "tab=settings&nosignin", wait: 700, after: async (p) => { for (let i = 0; i < 7; i++) { await p.keyboard.press("ArrowDown"); await pause(120); } await pause(900); } },
  { name: "settings-notifications", q: "tab=settings&nosignin", wait: 700, after: async (p) => { for (let i = 0; i < 8; i++) { await p.keyboard.press("ArrowDown"); await pause(120); } await pause(900); } },
  { name: "settings-emulators", q: "tab=settings&nosignin", wait: 700, after: async (p) => { for (let i = 0; i < 9; i++) { await p.keyboard.press("ArrowDown"); await pause(120); } await pause(900); } },
  { name: "settings-about", q: "tab=settings&nosignin", wait: 700, after: async (p) => { for (let i = 0; i < 10; i++) { await p.keyboard.press("ArrowDown"); await pause(120); } await pause(900); } },
  { name: "settings-licences", q: "tab=settings&nosignin", wait: 700, after: async (p) => { for (let i = 0; i < 11; i++) { await p.keyboard.press("ArrowDown"); await pause(120); } await pause(900); } },
  { name: "settings-search-brightness", q: "tab=settings&nosignin", after: async (p) => { await p.evaluate(() => document.querySelector("input[placeholder=\"Search settings\"]").focus()); await p.keyboard.type("bright"); await pause(400); } },
  { name: "settings-search-wifi", q: "tab=settings&nosignin", after: async (p) => { await p.evaluate(() => document.querySelector("input[placeholder=\"Search settings\"]").focus()); await p.keyboard.type("wifi"); await pause(400); } },
  { name: "settings-search-idle", q: "tab=settings&nosignin", after: async (p) => { await p.evaluate(() => document.querySelector("input[placeholder=\"Search settings\"]").focus()); await p.keyboard.type("idle"); await pause(400); } },
  { name: "settings-search-jump", q: "tab=settings&nosignin", after: async (p) => { await p.evaluate(() => document.querySelector("input[placeholder=\"Search settings\"]").focus()); await p.keyboard.type("fan"); await pause(300); await p.keyboard.press("Enter"); await pause(900); } },
  { name: "settings-search-none", q: "tab=settings&nosignin", after: async (p) => { await p.evaluate(() => document.querySelector("input[placeholder=\"Search settings\"]").focus()); await p.keyboard.type("zzzz"); await pause(400); } },
  { name: "settings-licences", q: "tab=settings&nosignin", after: async (p) => { for (let i = 0; i < 4; i++) await p.keyboard.press("ArrowDown"); } },
  // OS updates (src/screens/settings, src/notify/OsUpdateNotice): ?update=staged|failed, ?restore=none.
  { name: "os-update-notice", q: "tab=home&nosignin&update=staged", wait: 2500 },
  { name: "os-update-staged", q: "tab=settings&nosignin&update=staged", after: async (p) => { await pause(500); await p.evaluate(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent === "Restart now"); b && (b.scrollIntoView({ block: "center" }), b.focus()); }); await pause(400); } },
  { name: "os-update-restart", q: "tab=settings&nosignin&update=staged", after: async (p) => { await pause(500); await p.evaluate(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent === "Restart now"); b && b.click(); }); await pause(400); } },
  { name: "os-restore-confirm", q: "tab=settings&nosignin", after: async (p) => { await pause(500); await p.evaluate(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent === "Restore"); b && b.click(); }); await pause(400); } },
  { name: "os-boot-menu-row", q: "tab=settings&nosignin", after: async (p) => { await pause(500); await p.evaluate(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent === "Show next start"); b && (b.scrollIntoView({ block: "center" }), b.focus()); }); await pause(400); } },
  { name: "sign-in", q: "tab=home&nosignin&signin" },
  { name: "boot", q: "tab=home&nosignin&boot", wait: 1600 },
  { name: "boot-slow", q: "tab=home&nosignin&boot=slow", wait: 1600 },
  { name: "first-run-1-welcome", q: "tab=home&firstrun&step=0", wait: 3500 },
  { name: "first-run-2-wifi", q: "tab=home&firstrun&step=1", wait: 3000 },
  { name: "first-run-3-passkey", q: "tab=home&firstrun&step=2", wait: 3000 },
  { name: "first-run-3b-named", q: `tab=home&firstrun&step=2&named&devsign&watch=${pawtatoHolder}`, wait: 3500 },
  { name: "first-run-4-steam", q: "tab=home&firstrun&step=3", wait: 3000 },
  { name: "home-move-card", q: `tab=home&nosignin&setupdone&devsign&watch=${pawtatoHolder}`, wait: 3500, after: async (p) => { await p.evaluate(() => document.querySelector("[data-scroll]")?.scrollTo(0, 560)); await pause(600); } },
  // Offline first run: the passkey step says so and points back to Wi-Fi; Steam says it can wait; the other ways to sign in open inside setup.
  { name: "first-run-offline-passkey", q: "tab=home&firstrun&step=2&offline&nogames", wait: 3000 },
  { name: "first-run-offline-steam", q: "tab=home&firstrun&step=3&offline&nogames", wait: 3000 },
  { name: "first-run-other-ways", q: "tab=home&firstrun&step=2&nogames", wait: 3000, after: async (p) => { for (const k of ["ArrowDown", "ArrowDown", "ArrowLeft", "Enter"]) { await p.keyboard.press(k); await pause(250); } } },
  { name: "first-run-language", q: "tab=home&firstrun&nogames", wait: 3000, after: async (p) => { for (const k of ["ArrowRight", "Enter"]) { await p.keyboard.press(k); await pause(250); } } },
  { name: "home-nothing-installed", q: "tab=home&nosignin&nogames&offline", wait: 3000 },
  // First-run steps from src/onboarding (Wi-Fi offline with networks, Steam signed in).
  { name: "onboarding-wifi", q: "tab=home&firstrun&step=1&offline", wait: 3000 },
  { name: "onboarding-steam", q: "tab=home&firstrun&step=3&steam=rareplayer", wait: 3000 },
  // In-game overlay (src/overlay): hold View over a game. Live rare.market data for real Pawtato and Warped Pioneers holders.
  { name: "overlay-web-items", q: `tab=home&nosignin&watch=${pawtatoHolder}&running=web:pawtato-land&overlay=pawtato-land`, wait: 9000 },
  // ?devsign lets the watch account reach the review sheet; it only simulates, nothing is signed or sent.
  { name: "overlay-web-item", q: `tab=home&nosignin&devsign&watch=${pawtatoHolder}&running=web:pawtato-land&overlay=pawtato-land`, wait: 9000, after: async (p) => { for (let i = 0; i < 3; i++) { await p.keyboard.press("ArrowDown"); await pause(200); } await p.keyboard.press("Enter"); await pause(3500); } },
  { name: "overlay-web-market", q: `tab=home&nosignin&watch=${pawtatoHolder}&running=web:pawtato-land&overlay=pawtato-land`, wait: 9000, after: async (p) => { await p.keyboard.press("e"); await pause(2500); } },
  { name: "overlay-web-notifications", q: `tab=home&nosignin&watch=${pawtatoHolder}&running=web:pawtato-land&overlay=pawtato-land`, wait: 9000, after: async (p) => { for (let i = 0; i < 3; i++) { await p.keyboard.press("e"); await pause(250); } await pause(2000); } },
  { name: "overlay-native-items", q: `tab=home&nosignin&watch=${warpedHolder}&running=steam:3277880&overlay=steam:3277880`, wait: 9000 },
  { name: "overlay-native-sell", q: `tab=home&nosignin&devsign&watch=${warpedHolder}&running=steam:3277880&overlay=steam:3277880`, wait: 9000, after: async (p) => { await p.keyboard.press("Enter"); await pause(5000); await p.keyboard.press("Enter"); await pause(10000); } },
  { name: "overlay-steam-trophies", q: `tab=home&nosignin&watch=${warpedHolder}&running=steam:1145350&overlay=steam:1145350`, wait: 9000 },
  // Games: every source launching and playing well (launcher/src/games).
  { name: "games-library", q: "tab=library&nosignin" },
  { name: "games-esde", q: "tab=library&nosignin", after: async (p) => { await p.evaluate(() => { const b = [...document.querySelectorAll("button[data-nav]")].find((x) => (x.dataset.title ?? x.parentElement?.textContent ?? "").includes("ES-DE")); b && b.click(); }); } },
  { name: "games-running", q: "tab=library&nosignin&running=steam:1145350", after: async (p) => { await p.evaluate(() => { const b = [...document.querySelectorAll("button[data-nav]")].find((x) => (x.dataset.title ?? x.parentElement?.textContent ?? "").includes("Hades II")); b && b.click(); }); } },
  { name: "games-act", q: "tab=library&nosignin", after: async (p) => { await p.evaluate(() => { const b = [...document.querySelectorAll("button[data-nav]")].find((x) => x.parentElement?.textContent.trim().startsWith("ACT")); b && b.click(); }); } },
  { name: "games-sui", q: "tab=games&nosignin", wait: 3500 },
  { name: "games-sui-coming", q: "tab=games&nosignin", wait: 3500, after: async (p) => { await p.evaluate(() => { const s = document.querySelector("[data-nav-group=suigames-coming]"); s && s.scrollIntoView({ block: "center" }); }); } },
  { name: "games-sui-native", q: "tab=games&nosignin", wait: 3500, after: async (p) => { await p.evaluate(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent.includes("XOCIETY")); b && b.click(); }); } },
  { name: "games-sui-web", q: "tab=games&nosignin", wait: 3500, after: async (p) => { await p.evaluate(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent.includes("ACT")); b && b.click(); }); } },
  // Apps (launcher/src/apps): the Home row, Library > Apps, app pages, Guide held.
  { name: "apps-home", q: "tab=home&nosignin&running=app:telegram", after: async (p) => { await p.evaluate(() => { const b = document.querySelector("[data-app=netflix]"); b && b.focus(); b && b.scrollIntoView({ block: "center" }); }); } },
  { name: "apps-library", q: "tab=library&nosignin&filter=apps&running=app:telegram&installing=discord" },
  { name: "apps-netflix-get", q: "tab=home&nosignin&app=netflix&flatpaks=org.chromium.Chromium" },
  { name: "apps-netflix-installing", q: "tab=home&nosignin&app=netflix&flatpaks=org.chromium.Chromium&installing=netflix" },
  { name: "apps-telegram-running", q: "tab=home&nosignin&app=telegram&running=app:telegram" },
  { name: "apps-x", q: "tab=home&nosignin&app=x" },
  { name: "apps-discord-web", q: "tab=home&nosignin&app=discord" },
  { name: "apps-guide-hold", q: "tab=home&nosignin&guidehold&running=app:netflix" },
  // PC streaming (src/stream): a mocked Sunshine PC, paired, with an unpaired one on the network.
  { name: "stream-home", q: "tab=home&nosignin", wait: 3000, after: async (p) => { await p.evaluate(() => { const b = document.querySelector("[data-nav-group=stream-pc] button"); b && b.focus(); b && b.scrollIntoView({ block: "center" }); }); } },
  { name: "stream-library", q: "tab=library&nosignin&filter=stream&running=stream:5F6A2B10-71C3-4E7D-9A55-0B1C2D3E4F50:1245620", wait: 3000 },
  { name: "stream-library-empty", q: "tab=library&nosignin&filter=stream&stream=none" },
  { name: "stream-setup", q: "tab=library&nosignin&filter=stream&streamsetup", wait: 3500 },
  { name: "stream-setup-moonlight", q: "tab=library&nosignin&filter=stream&stream=nomoonlight&streamsetup", wait: 3500 },
  { name: "stream-pair", q: "tab=library&nosignin&filter=stream&streampair", wait: 3000 },
  { name: "stream-game", q: "tab=library&nosignin&filter=stream&streamapp=1091500", wait: 3000 },
  { name: "stream-game-running", q: "tab=library&nosignin&filter=stream&streamapp=1091500&running=stream:5F6A2B10-71C3-4E7D-9A55-0B1C2D3E4F50:1091500", wait: 3000 },
  { name: "stream-game-busy", q: "tab=library&nosignin&filter=stream&streamapp=1091500&streambusy", wait: 3000 },
  { name: "stream-game-asleep", q: "tab=library&nosignin&filter=stream&streamapp=1091500&stream=asleep", wait: 3000 },
  { name: "stream-guide-hold", q: "tab=home&nosignin&guidehold&running=stream:5F6A2B10-71C3-4E7D-9A55-0B1C2D3E4F50:1091500", wait: 3000 },
  // Share (src/share): mock captures; clips are a few seconds of recorded canvas, so give them time.
  { name: "share-grid", q: `tab=home&nosignin&watch=${demo}&share=grid`, wait: 8000 },
  { name: "share-detail", q: `tab=home&nosignin&watch=${demo}&share=detail`, wait: 9000 },
  { name: "share-clip", q: `tab=home&nosignin&watch=${demo}&share=clip`, wait: 10000 },
  { name: "share-uploading", q: `tab=home&nosignin&watch=${demo}&share=uploading`, wait: 8000 },
  { name: "share-done", q: `tab=home&nosignin&watch=${demo}&share=done`, wait: 8000 },
  { name: "share-clips-off", q: "tab=home&nosignin&share=grid&shareclips=off", wait: 6000 },
  { name: "share-quick-settings", q: "tab=home&nosignin", after: async (p) => { await p.keyboard.press("Tab"); await pause(600); await p.evaluate(() => { const b = document.querySelector("[data-nav-group=qam-share] button"); b && b.scrollIntoView({ block: "center" }); b && b.focus(); }); } },
  // Trophies (src/trophies): local Steam achievements (mock), and with ?trophychain a fake deployed package.
  { name: "trophies-case", q: "tab=home&nosignin&trophies", wait: 3000 },
  { name: "trophies-case-chain", q: `tab=home&nosignin&trophies&trophychain&as=${demo}`, wait: 4500 },
  { name: "trophies-game", q: `tab=home&nosignin&trophies=367520&trophychain&as=${demo}`, wait: 4500 },
  { name: "trophies-game-locked", q: `tab=home&nosignin&trophies=367520&trophychain&as=${demo}`, wait: 4500, after: async (p) => { await p.keyboard.press("x"); await pause(200); await p.keyboard.press("x"); } },
  { name: "trophies-game-local", q: "tab=home&nosignin&trophies=504230", wait: 3000 },
  { name: "trophies-profile", q: `tab=home&nosignin&sui=profile&trophychain&as=${demo}`, wait: 5000 },
  { name: "trophies-popup", q: "tab=home&nosignin&trophytoast", wait: 1700 },
  { name: "trophies-claim", q: `tab=home&nosignin&trophies=1145350&trophychain&as=${demo}`, wait: 4500, after: async (p) => { await p.keyboard.press("Enter"); await pause(2600); } },
];

// Rare Wallet screens: each scripts/shots-wallet-*.mjs exports a list of entries like the ones above.
for (const f of readdirSync(new URL(".", import.meta.url)).filter((f) => /^shots-wallet-.*\.mjs$/.test(f)).sort()) {
  shots.push(...(await import(new URL(f, import.meta.url))).default);
}

// ONLY=marketplace shoots just the shots whose name starts with that prefix (comma separated).
const only = process.env.ONLY?.split(",").filter(Boolean);
const sizes = (process.env.SIZES ?? "1280x800,1920x1200").split(",").map((s) => s.split("x").map(Number));
const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ["--hide-scrollbars", "--force-device-scale-factor=1", "--mute-audio"] });
for (const [w, h] of sizes) {
  for (const s of shots) {
    if (only && !only.some((o) => s.name.startsWith(o))) continue;
    const ctx = await browser.createBrowserContext();
    const p = await ctx.newPage();
    await p.evaluateOnNewDocument(() => { const stub = function () { return { createGain: () => ({ gain: {}, connect() {} }), createOscillator: () => ({ frequency: {}, connect() {}, start() {}, stop() {} }), destination: {}, resume: async () => {}, close: async () => {} }; }; window.AudioContext = stub; window.webkitAudioContext = stub; });
    p.on("pageerror", (e) => console.error(`${s.name}: ${e.message}`));
    await p.setViewport({ width: w, height: h, deviceScaleFactor: 1 });
    await p.goto(`${base}/?${s.q}`, { waitUntil: "networkidle2", timeout: 60000 }).catch(() => {});
    if (s.fresh) await p.evaluate(() => { localStorage.clear(); localStorage.setItem("rare.mock.config", JSON.stringify({ gameFolders: [], onboarded: false, webGames: [] })); });
    if (s.fresh) await p.reload({ waitUntil: "networkidle2" }).catch(() => {});
    await new Promise((r) => setTimeout(r, s.wait ?? 2500));
    if (s.after) { await s.after(p); await new Promise((r) => setTimeout(r, 1500)); }
    const file = `${out}/${s.name}-${w}x${h}.png`;
    await p.screenshot({ path: file });
    console.log(file);
    await ctx.close();
  }
}
await browser.close();
