// Checks every Rare web game the way the launcher runs it: the game's profile
// (window.__rareProfile) and Rare's runtime (webgame.js) are injected before the page's own
// scripts, then the checker verifies that the page loads, Rare Wallet registers as a Wallet
// Standard wallet, and controller input arrives in the game's input mode.
//
// Engines: Chrome (puppeteer-core) and WebKit (playwright-core). Playwright's WebKit is the
// closest thing to the device's WebKitGTK that runs on a Mac.
//
// Titles: every catalog entry on rare.market whose platforms include "web", plus every
// profile in src/games/profiles.json with a playUrl (ACT ships pre-installed).
// Play URL = profile.playUrl, else catalog.playUrl, else catalog.website.
//
// Usage:
//   node scripts/webgame-check.mjs [slug ...] [--engine chrome|webkit|both] [--shots dir] [--md file] [--json]
//   node scripts/webgame-check.mjs https://some.game/       (one URL, keys mode)
//   node scripts/webgame-check.mjs pawtato-land --as '{"input":"focus"}'   (try a profile change)
import puppeteer from "puppeteer-core";
import { webkit } from "playwright-core";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

// ---------- arguments ----------
const argv = process.argv.slice(2);
const opt = (name, dflt) => {
  const i = argv.indexOf(name);
  if (i < 0) return dflt;
  const v = argv[i + 1];
  argv.splice(i, 2);
  return v;
};
const flag = (name) => {
  const i = argv.indexOf(name);
  if (i >= 0) argv.splice(i, 1);
  return i >= 0;
};
const engineArg = opt("--engine", "both");
const shots = opt("--shots", null);
const mdOut = opt("--md", null);
const asJson = flag("--json");
// --as '{"input":"focus"}' tries a profile change on the selected titles before editing profiles.json.
const asProfile = JSON.parse(opt("--as", "null"));
const filter = argv;
const engines = engineArg === "both" ? ["chrome", "webkit"] : [engineArg];
if (shots) mkdirSync(shots, { recursive: true });

const runtime = readFileSync(new URL("../src-tauri/src/webgame.js", import.meta.url), "utf8");
const profiles = JSON.parse(readFileSync(new URL("../src/games/profiles.json", import.meta.url), "utf8")).games;
const CHROME = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

// ---------- titles ----------
async function titles() {
  if (filter.length === 1 && /^https?:\/\//.test(filter[0])) return [{ slug: new URL(filter[0]).host, name: filter[0], url: filter[0], profile: {} }];
  let catalog = [];
  try {
    catalog = await (await fetch("https://rare.market/games/games.json")).json();
  } catch (e) {
    console.error(`catalog unavailable: ${e}`);
  }
  const out = new Map();
  for (const g of catalog) {
    if (!(g.platforms ?? []).includes("web")) continue;
    out.set(g.slug, { slug: g.slug, name: g.name, status: g.status ?? "", catalog: g });
  }
  for (const [slug, p] of Object.entries(profiles)) {
    if (p.native && !("playUrl" in p)) continue;
    if (!out.has(slug)) out.set(slug, { slug, name: p.title ?? slug, status: p.builtin ? "built in" : "" });
  }
  const list = [];
  for (const t of out.values()) {
    const profile = { ...(profiles[t.slug] ?? {}), ...(asProfile ?? {}) };
    const url = "playUrl" in profile ? profile.playUrl : t.catalog?.playUrl ?? t.catalog?.website ?? null;
    list.push({ ...t, profile, url });
  }
  return list.filter((t) => !filter.length || filter.includes(t.slug));
}

// ---------- probe injected after the runtime ----------
// Records keys, Gamepad API polling, canvas contexts and WebGPU use, and answers the
// wallet-standard app-ready handshake so we see which wallets register.
function probe() {
  window.__chk = { keys: [], polls: 0, contexts: [], gpu: false, wallets: [] };
  addEventListener("keydown", (e) => window.__chk.keys.push(e.key), true);
  const gg = Navigator.prototype.getGamepads;
  Object.defineProperty(Navigator.prototype, "getGamepads", {
    configurable: true,
    value: function () {
      window.__chk.polls++;
      return gg.call(this);
    },
  });
  const gc = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (t, ...r) {
    const c = gc.call(this, t, ...r);
    window.__chk.contexts.push(`${t}:${c ? "ok" : "fail"}`);
    return c;
  };
  if (navigator.gpu) {
    const ra = navigator.gpu.requestAdapter.bind(navigator.gpu);
    navigator.gpu.requestAdapter = (...a) => ((window.__chk.gpu = true), ra(...a));
  }
  const api = {
    register: (...ws) => {
      for (const w of ws) window.__chk.wallets.push({ name: w.name, features: Object.keys(w.features), chains: w.chains });
      return () => {};
    },
  };
  addEventListener("wallet-standard:register-wallet", (e) => e.detail(api));
  setTimeout(() => dispatchEvent(new CustomEvent("wallet-standard:app-ready", { detail: api })), 0);
}

// ---------- engine adapters: one small page API over puppeteer and playwright ----------
async function openEngine(name) {
  if (name === "chrome") {
    const b = await puppeteer.launch({ executablePath: CHROME, headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--autoplay-policy=no-user-gesture-required"] });
    return {
      close: () => b.close(),
      page: async (init) => {
        const p = await b.newPage();
        await p.setViewport({ width: 1280, height: 800 });
        for (const s of init) await p.evaluateOnNewDocument(s);
        return wrap(p, (url) => p.goto(url, { waitUntil: "networkidle2", timeout: 45000 }), () => p.close());
      },
    };
  }
  const b = await webkit.launch({ headless: true });
  const ctx = await b.newContext({ viewport: { width: 1280, height: 800 } });
  return {
    close: () => b.close(),
    page: async (init) => {
      const p = await ctx.newPage();
      for (const s of init) await p.addInitScript(s);
      return wrap(p, (url) => p.goto(url, { waitUntil: "load", timeout: 45000 }).then(async (r) => (await p.waitForLoadState("networkidle", { timeout: 15000 }).catch(() => {}), r)), () => p.close());
    },
  };
}

function wrap(p, goto, close) {
  const errors = [];
  p.on("pageerror", (e) => errors.push(String(e?.message ?? e).slice(0, 160)));
  return {
    errors,
    goto,
    close,
    eval: (fn, arg) => p.evaluate(fn, arg),
    title: () => p.title().catch(() => ""),
    url: () => p.url(),
    shot: (path) => p.screenshot({ path }).catch(() => {}),
  };
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---------- input checks per mode ----------
async function checkKeys(pg, profile) {
  await pg.eval(() => {
    window.__chk.keys.length = 0;
    __rarePad("up", true);
    __rarePad("up", false);
    __rarePad("a", true);
    __rarePad("a", false);
  });
  const keys = await pg.eval(() => window.__chk.keys.slice());
  const want = [profile.keys?.up ?? "ArrowUp", profile.keys?.a ?? "Enter"];
  let pointer = null;
  if (profile.pointer !== false) {
    await pg.eval(() => __rarePadState(new Array(17).fill(0), [0, 0, 0.9, 0.4]));
    await sleep(250);
    await pg.eval(() => __rarePadState(new Array(17).fill(0), [0, 0, 0, 0]));
    pointer = await pg.eval(() => {
      const el = document.getElementById("__rare_ptr");
      return !!el && el.style.opacity === "1";
    });
  }
  const ok = want.every((k) => keys.includes(k)) && pointer !== false;
  return { ok, detail: `keys ${keys.slice(0, 4).join(",") || "none"}${pointer === null ? "" : `, pointer ${pointer ? "moves" : "missing"}`}` };
}

async function checkGamepad(pg) {
  const before = await pg.eval(() => window.__chk.polls);
  const pad = await pg.eval(() => {
    const b = new Array(17).fill(0);
    b[0] = 1;
    __rarePadState(b, [0.5, -0.25, 0, 0]);
    const g = navigator.getGamepads()[0];
    return g && { id: g.id, mapping: g.mapping, axes: g.axes, a: g.buttons[0].pressed, n: g.buttons.length };
  });
  await sleep(1000);
  await pg.eval(() => __rarePadState(new Array(17).fill(0), [0, 0, 0, 0]));
  const polls = (await pg.eval(() => window.__chk.polls)) - before;
  const ok = !!pad && pad.mapping === "standard" && pad.axes[0] === 0.5 && pad.axes[1] === -0.25 && pad.a && pad.n === 17;
  return { ok, detail: `virtual pad ${ok ? "standard, axes ok" : "missing"}, page polled ${polls}x/s` };
}

/** Focus mode: the ring lands on a real control and A clicks it. Word games also get typed into. */
async function checkFocus(pg) {
  const ring = async () =>
    pg.eval(() => {
      const r = document.getElementById("__rare_ring");
      if (!r || r.style.opacity !== "1") return null;
      const b = r.getBoundingClientRect();
      const el = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2);
      return (el?.closest("button,a,[role=button],[data-key],input") ?? el)?.innerText?.trim().slice(0, 20) ?? "?";
    });
  const press = (b) => pg.eval((x) => (__rarePad(x, true), __rarePad(x, false)), b);
  await press("right");
  await sleep(200);
  const first = await ring();
  if (first === null) return { ok: false, detail: "no focus ring" };
  // A clicks the focused control: the page should react (navigate or change its content).
  const snap = () => pg.eval(() => location.href + "#" + (document.body?.innerText ?? "").length + "#" + document.querySelectorAll("*").length);
  const s0 = await snap();
  await press("a");
  await sleep(3000);
  const reacted = (await snap().catch(() => "navigated")) !== s0;
  return { ok: true, detail: `ring on "${first}", A ${reacted ? "clicked it" : "had no visible effect"}` };
}

// ---------- one title on one engine ----------
async function check(eng, engineName, t) {
  const r = { engine: engineName, status: null, finalUrl: null, loaded: false, title: "", errors: [], canvas: 0, contexts: [], webgl2: null, gpu: false, wallet: null, input: null, signals: [] };
  const init = [`window.__rareProfile = ${JSON.stringify(t.profile ?? {})};`, runtime, `(${probe})();`];
  const pg = await eng.page(init);
  try {
    const resp = await pg.goto(t.url).catch((e) => {
      r.errors.push(String(e.message ?? e).slice(0, 160));
      return null;
    });
    r.status = resp?.status?.() ?? null;
    r.loaded = !!resp && r.status < 400;
    await sleep(6000);
    r.finalUrl = pg.url();
    r.title = await pg.title();
    const page = await pg.eval(() => {
      const c = document.createElement("canvas");
      const webgl2 = !!c.getContext("webgl2");
      const text = (document.body?.innerText ?? "").slice(0, 4000);
      return { canvas: document.querySelectorAll("canvas").length, contexts: window.__chk.contexts, gpu: window.__chk.gpu, wallets: window.__chk.wallets, webgl2, text };
    }).catch((e) => ({ canvas: 0, contexts: [], gpu: false, wallets: [], webgl2: null, text: "", err: String(e) }));
    r.canvas = page.canvas;
    r.contexts = [...new Set(page.contexts)];
    r.webgl2 = page.webgl2;
    r.gpu = page.gpu;
    const w = page.wallets.find((x) => x.name === "Rare Wallet");
    r.wallet = w ? w.features.length : 0;
    // Older dapp-kit and wallet-kit builds only list wallets with the pre-2024 "Block" features.
    r.walletLegacy = !!w && w.features.includes("sui:signTransactionBlock");
    const text = page.text.toLowerCase();
    if (/not available in your (region|country)|not available in your jurisdiction|region is not supported/.test(text)) r.signals.push("region");
    // A sign-in wall: the page is mostly a login form and has no game canvas behind it.
    if (/\b(sign in|log in|login|sign up)\b/.test(text) && page.canvas === 0 && text.length < 1500) r.signals.push("sign-in");
    if (/404|page cannot be found|page not found|site not found/.test(`${r.title} ${text.slice(0, 300)}`)) r.signals.push("404");
    if (/webgpu|not supported|unsupported browser|use chrome/.test(`${text} ${r.errors.join(" ")}`)) r.signals.push("unsupported");
    if (r.contexts.some((c) => c.endsWith(":fail"))) r.signals.push("context-fail");
    if (r.gpu) r.signals.push("webgpu");
    const mode = t.profile?.input ?? "keys";
    r.input = { mode, ...(mode === "gamepad" ? await checkGamepad(pg) : mode === "focus" ? await checkFocus(pg) : await checkKeys(pg, t.profile ?? {})) };
    r.errors.push(...pg.errors);
    if (shots) await pg.shot(join(shots, `${t.slug}-${engineName}.png`));
  } catch (e) {
    r.errors.push(String(e.message ?? e).slice(0, 160));
  } finally {
    await pg.close().catch(() => {});
  }
  r.errors = r.errors.slice(0, 3);
  return r;
}

// ---------- verdict ----------
function verdict(t, res) {
  if (!t.url) return ["Offline", t.profile?.unavailable ?? "no play URL"];
  const wk = res.webkit, ch = res.chrome;
  const any = wk ?? ch;
  if (!any.loaded || any.signals.includes("404")) return ["Offline", any.status ? `HTTP ${any.status}` : any.errors[0] ?? "did not load"];
  if (any.signals.includes("region")) return ["Region blocked", "geo-blocked from the test machine"];
  const plays = (r) => r && r.loaded && r.input?.ok && !r.signals.includes("context-fail") && !r.signals.includes("webgpu") && !r.signals.includes("unsupported");
  const gameish = (r) => r.canvas > 0 || (t.profile?.input ?? "keys") !== "keys";
  if (wk && !plays(wk) && ch && plays(ch)) return ["Chrome only", `WebKit: ${wk.signals.join(", ") || wk.input?.detail || wk.errors[0] || "fails"}`];
  // A DOM page in keys mode: arrow keys and Enter have nothing to steer. Either the page is a
  // DOM game that wants focus mode, or the play URL is not a game at all (check the screenshot).
  if (!gameish(any)) return ["Needs focus mode or not a game", "no game canvas; keys mode has nothing to steer"];
  if (plays(wk ?? ch)) return any.signals.includes("sign-in") ? ["Plays (sign-in needed)", "loads, input arrives; account or wallet sign-in in the page"] : [wk ? "Plays in WebKit" : "Plays in Chrome", "loads, input arrives"];
  return ["Check", (wk ?? ch).input?.detail ?? "input check failed"];
}

// ---------- run ----------
const list = await titles();
const open = {};
for (const e of engines) open[e] = await openEngine(e);
const results = [];
for (const t of list) {
  const res = {};
  if (t.url) {
    // One title at a time per engine, both engines in parallel.
    const done = await Promise.all(engines.map((e) => check(open[e], e, t)));
    done.forEach((r) => (res[r.engine] = r));
  }
  const [v, note] = verdict(t, res);
  results.push({ slug: t.slug, name: t.name, status: t.status, url: t.url, mode: t.profile?.input ?? "keys", res, verdict: v, note });
  if (!asJson) console.error(`${t.slug}: ${v} (${note})`);
}
for (const e of engines) await open[e].close();

const cell = (r) => {
  if (!r) return "";
  const parts = [r.loaded ? `HTTP ${r.status}` : "no load"];
  if (r.canvas) parts.push(`canvas${r.contexts.length ? ` ${r.contexts.join(" ")}` : ""}`);
  parts.push(`wallet ${r.wallet} features${r.walletLegacy ? " incl. legacy Block API" : ""}`);
  if (r.input) parts.push(`${r.input.ok ? "input ok" : "input FAIL"} (${r.input.detail})`);
  if (r.signals.length) parts.push(`signals: ${r.signals.join(", ")}`);
  if (r.errors.length) parts.push(`errors: ${r.errors.length}`);
  return parts.join("; ").replace(/\|/g, "/");
};
const md = [
  `| Game | Catalog status | Input mode | Play URL | ${engines.map((e) => (e === "webkit" ? "WebKit" : "Chrome")).join(" | ")} | Verdict |`,
  `|---|---|---|---|${engines.map(() => "---|").join("")}---|`,
  ...results.map((x) => `| ${x.name} (\`${x.slug}\`) | ${x.status || ""} | ${x.mode} | ${x.url ?? "none"} | ${engines.map((e) => cell(x.res[e])).join(" | ")} | ${x.verdict}: ${x.note} |`),
].join("\n");
if (mdOut) writeFileSync(mdOut, md + "\n");
console.log(asJson ? JSON.stringify(results, null, 1) : md);
