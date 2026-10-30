// Controller navigation checks in mock mode (WebKit). Run `npm run dev:ui`, then `npm run navcheck`.
// Covers docs/CONTROLS.md: LB/RB tabs, LT/RT sections, B back, focus never lost, no dead ends.
// Exit code 1 when a hard check fails. Reachability is reported (unreachable elements are listed).
import { webkit } from "playwright-core";

const base = process.env.URL ?? "http://localhost:5180";
const W = Number(process.env.W ?? 1920), H = Number(process.env.H ?? 1200);
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const fails = [];
const warns = [];
const ok = (cond, msg) => { if (!cond) { fails.push(msg); console.log("FAIL", msg); } else console.log("ok  ", msg); };

// Screens to walk, and the five tabs LB/RB cycle through (Friends and Settings are destinations; Trade is part of Market).
const TABS = ["home", "library", "games", "market", "wallet", "friends", "settings"];
const CYCLE = ["Home", "Library", "Sui", "Market", "Wallet"];
const browser = await webkit.launch();

async function open(q, wait = 2500) {
  const ctx = await browser.newContext({ viewport: { width: W, height: H } });
  await ctx.addInitScript(() => { const N = function () { return new Proxy({}, { get: () => () => N() }); }; window.AudioContext = window.webkitAudioContext = class { constructor() { return N(); } }; window.Audio = class { play() { return Promise.resolve(); } pause() {} addEventListener() {} }; });
  const p = await ctx.newPage();
  p.on("pageerror", (e) => { fails.push(`pageerror ${q}: ${e.message}`); console.log("PAGEERROR", e.message); });
  await p.goto(`${base}/?${q}`);
  await pause(wait);
  return p;
}
const pad = (p, b) => p.evaluate((x) => window.__pad(x), b);
// What has focus: a descriptor, or null when focus is lost.
const focus = (p) => p.evaluate(() => {
  const a = document.activeElement;
  if (!a || !a.hasAttribute("data-nav") || a.closest("[inert]")) return null;
  const r = a.getBoundingClientRect();
  return { id: (a.dataset.title ?? a.getAttribute("aria-label") ?? a.textContent ?? "").trim().slice(0, 40), x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height), vw: innerWidth, vh: innerHeight };
});
const tab = (p) => p.evaluate(() => document.querySelector("nav [role=tab][aria-selected=true]")?.textContent?.trim() ?? null);
const nav = (p) => p.evaluate(() => [...document.querySelectorAll("[data-nav]")].filter((e) => !e.closest("[inert]") && e.getBoundingClientRect().width > 0).length);

// Focus ring clipping: the focused element's ring (5 px around the scaled box) must sit inside every ancestor that
// clips (overflow other than visible), once the engine's own scrolling has settled.
const clipOf = (p) => p.evaluate(() => {
  const el = document.activeElement;
  if (!el || !el.hasAttribute("data-nav")) return null;
  const r = el.getBoundingClientRect();
  const ring = 5;
  for (let a = el.parentElement; a && a !== document.documentElement; a = a.parentElement) {
    const cs = getComputedStyle(a);
    if (cs.overflowX === "visible" && cs.overflowY === "visible") continue;
    const c = a.getBoundingClientRect();
    const sx = cs.overflowX !== "visible", sy = cs.overflowY !== "visible";
    const cl = (sx && r.left - ring < c.left - 1) ? "left" : (sx && r.right + ring > c.right + 1) ? "right" : (sy && r.top - ring < c.top - 1) ? "top" : (sy && r.bottom + ring > c.bottom + 1) ? "bottom" : null;
    if (cl) return `${(el.dataset.title ?? el.getAttribute("aria-label") ?? el.textContent ?? "").trim().slice(0, 24)} clipped ${cl} by ${a.tagName.toLowerCase()}.${String(a.className).split(" ").slice(0, 2).join(".")}`;
  }
  return null;
});

// 1. Every tab: focus on arrival, never lost while walking, no element clipped off the viewport.
for (const t of TABS) {
  const p = await open(`tab=${t}&nosignin`);
  let f = await focus(p);
  const count = await nav(p);
  if (count === 0) { console.log(`skip ${t}: nothing focusable in mock mode`); await p.context().close(); continue; }
  ok(!!f, `${t}: something is focused on arrival (${count} focusables)`);
  let lost = 0, offscreen = 0, seed = 7;
  const seen = new Set();
  const clips = new Set();
  for (let i = 0; i < 100; i++) {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    await pad(p, ["up", "down", "left", "right"][(seed >> 5) % 4]);
    await pause(260);
    const cl = await clipOf(p);
    if (cl) clips.add(cl);
    f = await focus(p);
    if (!f) { lost++; continue; }
    seen.add(`${f.id}@${f.x},${f.y}`);
    if (f.y + f.h < 0 || f.y > f.vh || f.x + f.w < 0 || f.x > f.vw) {
      // Smooth scrolls are still gliding; judge where it settles.
      await pause(350);
      const g = await focus(p);
      if (g && (g.y + g.h < 0 || g.y > g.vh || g.x + g.w < 0 || g.x > g.vw)) { offscreen++; console.log(`     off screen: ${g.id} at ${g.x},${g.y}`); }
    }
  }
  ok(lost === 0, `${t}: focus never lost in 100 moves (lost ${lost})`);
  ok(offscreen === 0, `${t}: focused element always on screen (off ${offscreen})`);
  ok(clips.size === 0, `${t}: focus ring never clipped${clips.size ? ` (${clips.size}: ${[...clips].slice(0, 3).join("; ")})` : ""}`);
  console.log(`     ${t}: visited ${seen.size} distinct of ${count} focusables`);
  await p.context().close();
}

// 1b. Reachability from the engine's own moves: focus every element, press each direction, and
// check that the graph of landings reaches every focusable from the default one.
async function reachability(p, label) {
  const n = await p.evaluate(() => {
    const els = [...document.querySelectorAll("[data-nav]")].filter((e) => !e.closest("[inert]") && !e.hasAttribute("disabled") && e.getBoundingClientRect().width > 0);
    els.forEach((e, i) => e.setAttribute("data-nav-i", String(i)));
    return els.length;
  });
  const start = await p.evaluate(() => Number(document.activeElement?.getAttribute("data-nav-i") ?? -1));
  const edges = Array.from({ length: n }, () => new Set());
  for (let i = 0; i < n; i++) {
    for (const d of ["up", "down", "left", "right"]) {
      await p.evaluate((k) => document.querySelector(`[data-nav-i="${k}"]`)?.focus({ preventScroll: true }), i);
      await pad(p, d);
      await pause(25);
      const j = await p.evaluate(() => Number(document.activeElement?.getAttribute("data-nav-i") ?? -1));
      if (j >= 0 && j !== i) edges[i].add(j);
    }
  }
  const seen = new Set(start >= 0 ? [start] : []);
  const q = [...seen];
  while (q.length) for (const j of edges[q.shift()]) if (!seen.has(j)) { seen.add(j); q.push(j); }
  // A section that swaps its panel on focus (Settings) detaches the old panel's elements: those are not missing.
  const attached = await p.evaluate((n) => Array.from({ length: n }, (_, i) => !!document.querySelector(`[data-nav-i="${i}"]`)), n);
  const missing = [...Array(n).keys()].filter((i) => !seen.has(i) && attached[i]);
  const names = await p.evaluate((ms) => ms.slice(0, 6).map((i) => { const e = document.querySelector(`[data-nav-i="${i}"]`); return (e?.dataset.title ?? e?.getAttribute("aria-label") ?? e?.textContent ?? "").trim().slice(0, 30); }), missing);
  const dead = [...Array(n).keys()].filter((i) => edges[i].size === 0);
  ok(missing.length === 0, `${label}: every focusable reachable by direction keys (${n - missing.length}/${n})${missing.length ? " missing: " + names.join(" | ") : ""}`);
  if (dead.length) warns.push(`${label}: ${dead.length} focusables where no direction goes anywhere`);
}
if (!process.env.NO_REACH) {
  for (const t of TABS) {
    const p = await open(`tab=${t}&nosignin`);
    await reachability(p, t);
    await p.context().close();
  }
}

// 2. LB/RB cycle through the tabs and come home; focus exists on each tab.
{
  const p = await open("tab=home&nosignin");
  const seq = [];
  for (let i = 0; i < CYCLE.length; i++) { await pad(p, "rb"); await pause(500); seq.push(await tab(p)); const f = await focus(p); if (!f) warns.push(`rb tab ${seq.at(-1)}: no focus after 500 ms`); }
  ok(seq.join(">") === [...CYCLE.slice(1), CYCLE[0]].join(">"), `RB visits all ${CYCLE.length} tabs in order and wraps: ${seq.join(" > ")}`);
  for (let i = 0; i < CYCLE.length; i++) { await pad(p, "lb"); await pause(300); }
  await p.context().close();
}

// 3. B: closes a page, then goes back through tabs, and at Home does nothing harmful.
{
  const p = await open("tab=home&nosignin");
  await pad(p, "x"); await pause(600);
  const gameOpen = await p.evaluate(() => !!document.querySelector("main[inert]"));
  ok(gameOpen, "X on Home opens the game page (A would launch it)");
  await pad(p, "b"); await pause(500);
  ok(!(await p.evaluate(() => !!document.querySelector("main[inert]"))), "B closes the game page");
  ok(!!(await focus(p)), "focus returns after B closes the page");
  await pad(p, "qam"); await pause(700);
  ok(await p.evaluate(() => !!document.querySelector("main[inert]")), "Quick opens quick settings");
  const qf = await focus(p);
  ok(!!qf, "quick settings has focus");
  await pad(p, "b"); await pause(500);
  ok(!(await p.evaluate(() => !!document.querySelector("main[inert]"))), "B closes quick settings");
  await pad(p, "rb"); await pause(400); await pad(p, "rb"); await pause(400);
  await pad(p, "b"); await pause(400);
  ok((await tab(p)) !== null, "B goes back one tab");
  for (let i = 0; i < 5; i++) { await pad(p, "b"); await pause(250); }
  ok(!!(await focus(p)), "B at the end of history keeps focus (no dead end)");
  await p.context().close();
}

// 4. LB/RB with a page open close it first and move the tab.
{
  const p = await open("tab=home&nosignin");
  await pad(p, "x"); await pause(600);
  await pad(p, "rb"); await pause(600);
  ok(!(await p.evaluate(() => !!document.querySelector("main[inert]"))), "RB over a game page closes it");
  ok((await tab(p)) === "Library", "RB over a game page moves to the next tab");
  await p.context().close();
}

// 5. LT/RT: sections on Library change the filter; focus survives.
{
  const p = await open("tab=library&nosignin");
  const before = await p.evaluate(() => document.querySelectorAll("[data-nav]").length);
  await pad(p, "rt"); await pause(500);
  const after = await p.evaluate(() => document.querySelectorAll("[data-nav]").length);
  ok(before !== after, `RT on Library changes the section (${before} -> ${after} focusables)`);
  ok(!!(await focus(p)), "focus kept after RT");
  await pad(p, "lt"); await pause(500);
  ok(!!(await focus(p)), "focus kept after LT");
  await p.context().close();
}

// 6. Focus survives a data refresh (library rescan on window focus).
for (const t of ["home", "library"]) {
  const p = await open(`tab=${t}&nosignin`);
  for (let i = 0; i < 3; i++) { await pad(p, "right"); await pause(120); }
  const a = await focus(p);
  await p.evaluate(() => window.dispatchEvent(new Event("focus")));
  await pause(1500);
  const b = await focus(p);
  ok(!!b, `${t}: focus kept through a library refresh`);
  if (a && b) ok(a.id === b.id, `${t}: same element focused after the refresh (${a.id} / ${b.id})`);
  await p.context().close();
}

// 7. Held direction: a long run of fast repeats keeps focus and does not freeze the page.
{
  const p = await open("tab=library&nosignin");
  const t0 = Date.now();
  for (let i = 0; i < 40; i++) await p.evaluate(() => window.__pad("down", true));
  await pause(600);
  ok(!!(await focus(p)), "40 repeated Down presses keep focus");
  ok(Date.now() - t0 < 4000, `repeats handled promptly (${Date.now() - t0} ms)`);
  await p.context().close();
}

await browser.close();
if (warns.length) console.log("\nWarnings:\n" + warns.map((w) => " - " + w).join("\n"));
console.log(fails.length ? `\n${fails.length} FAILED` : "\nall passed");
process.exit(fails.length ? 1 : 0);
