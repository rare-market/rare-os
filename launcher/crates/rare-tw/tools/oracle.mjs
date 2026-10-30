#!/usr/bin/env node
// The parity oracle: what a browser actually computes for every class and class string in the
// corpus, so `cargo test -p rare-tw-core` can hold the Rust parser to the web launcher's pixels.
//
//   node launcher/crates/rare-tw/tools/oracle.mjs       (after extract.mjs; needs launcher/node_modules)
//
// Builds the launcher's real stylesheet (Tailwind v4 over app.css, every corpus class as a candidate),
// loads it into a headless Chromium (chrome-headless-shell, never a desktop Chrome profile; set
// CHROME=/path to choose one) at 1280x800 so 1rem = 16px, puts one <div> per class and per class
// string inside a display:none wrapper (so lengths come back as computed values: % stays %, auto
// stays auto) and records every tracked property that differs from a plain <div>.
//
// Writes corpus/computed.tsv:  kind <TAB> classes <TAB> prop=value|prop=value...
//   kind is `base` (the plain div, every tracked property), `class` (one class, a variant's bare
//   utility), `string` (a whole class string), `first` / `last` (the element as first or last
//   child), `notlast` (the classes on a parent: its first child against its last, for space-* and
//   divide-*) or `focus-visible` (the state forced through DevTools).
// Colours are normalised to rgba(r,g,b,a) in sRGB 0-255 (Chrome reports color-mix results in oklab).

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const here = path.dirname(fileURLToPath(import.meta.url));
const crate = path.resolve(here, "..");
const launcher = path.resolve(crate, "../..");
const appCss = path.join(launcher, "src/styles/app.css");
const corpus = path.join(crate, "corpus");
const require = createRequire(path.join(launcher, "package.json"));
const { compile } = await import(require.resolve("@tailwindcss/node"));
const puppeteer = (await import(require.resolve("puppeteer-core"))).default;

function findChrome() {
  if (process.env.CHROME) return process.env.CHROME;
  const roots = [path.join(os.homedir(), ".cache/puppeteer/chrome-headless-shell")];
  for (const r of roots) {
    if (!fs.existsSync(r)) continue;
    for (const v of fs.readdirSync(r).sort().reverse()) {
      for (const sub of fs.readdirSync(path.join(r, v))) {
        const p = path.join(r, v, sub, "chrome-headless-shell");
        if (fs.existsSync(p)) return p;
      }
    }
  }
  throw new Error("no chrome-headless-shell found: npx @puppeteer/browsers install chrome-headless-shell@stable, or set CHROME");
}

const readTsv = (f) =>
  fs
    .readFileSync(path.join(corpus, f), "utf8")
    .split("\n")
    .filter((l) => l && !l.startsWith("#"))
    .map((l) => l.split("\t"));
const classes = readTsv("classes.tsv").map((r) => r[1]);
const strings = readTsv("strings.tsv").map((r) => r[2]);

// Variants are checked through their bare utility (the state's declarations are the utility's).
const bare = (c) => {
  let depth = 0, cut = -1;
  for (let i = 0; i < c.length; i++) {
    if (c[i] === "[") depth++;
    else if (c[i] === "]") depth--;
    else if (c[i] === ":" && depth === 0) cut = i;
  }
  return cut < 0 ? c : c.slice(cut + 1).replace(/^!/, "");
};

// The stylesheet the launcher ships: app.css through Tailwind with exactly these candidates.
const cssSrc = fs.readFileSync(appCss, "utf8");
const compiler = await compile(cssSrc, { base: path.dirname(appCss), onDependency() {} });
const sheet = compiler.build([...new Set([...classes, ...classes.map((c) => bare(c))])]);

const PROPS = [
  "display", "position", "top", "right", "bottom", "left", "z-index",
  "margin-top", "margin-right", "margin-bottom", "margin-left",
  "padding-top", "padding-right", "padding-bottom", "padding-left",
  "width", "height", "min-width", "min-height", "max-width", "max-height", "aspect-ratio",
  "flex-grow", "flex-shrink", "flex-basis", "flex-direction", "flex-wrap",
  "align-items", "align-self", "justify-content", "row-gap", "column-gap",
  "grid-template-columns", "grid-template-rows", "grid-column-start", "grid-column-end", "grid-row-start",
  "overflow-x", "overflow-y", "visibility", "pointer-events", "user-select",
  "background-color", "background-image", "background-size", "background-position",
  "border-top-width", "border-right-width", "border-bottom-width", "border-left-width",
  "border-top-color", "border-right-color", "border-bottom-color", "border-left-color",
  "border-top-left-radius", "border-top-right-radius", "border-bottom-right-radius", "border-bottom-left-radius",
  "opacity", "box-shadow", "outline-style", "outline-width", "outline-color", "outline-offset",
  "translate", "scale", "rotate", "transform-origin", "mix-blend-mode", "filter", "backdrop-filter",
  "mask-image", "mask-position", "mask-repeat", "mask-size", "object-fit", "object-position",
  "color", "font-family", "font-size", "line-height", "letter-spacing", "font-weight",
  "text-align", "text-transform", "text-decoration-line", "white-space-collapse", "text-wrap-mode",
  "text-overflow", "-webkit-line-clamp", "word-break", "overflow-wrap",
  "font-variant-numeric", "font-variant-ligatures", "text-shadow",
  "transition-property", "transition-duration", "transition-timing-function",
  "animation-name", "animation-duration", "animation-timing-function", "animation-iteration-count", "animation-delay",
  "content-visibility",
];

const browser = await puppeteer.launch({
  executablePath: findChrome(),
  headless: "shell",
  args: ["--hide-scrollbars", "--force-device-scale-factor=1", "--mute-audio", "--no-first-run"],
});
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
  await page.setContent(`<!doctype html><html><head><style>${sheet}</style></head><body><div id="wrap" style="display:none"></div></body></html>`);
  // Helpers live on window so later evaluate() calls can share them.
  await page.evaluate((PROPS) => {
    // oklab / oklch / color(srgb) / rgb(a) -> "rgba(r,g,b,a)" in sRGB 0-255.
    const lin2s = (x) => {
      const v = x <= 0.0031308 ? 12.92 * x : 1.055 * Math.pow(x, 1 / 2.4) - 0.055;
      return Math.min(255, Math.max(0, v * 255));
    };
    const fmt = (r, g, b, a) => `rgba(${r.toFixed(2)},${g.toFixed(2)},${b.toFixed(2)},${(+a).toFixed(4)})`;
    const num = (s) => (s.endsWith("%") ? parseFloat(s) / 100 : parseFloat(s));
    const color = (s) => {
      let m;
      if ((m = s.match(/^rgba?\(([^)]*)\)$/))) {
        const p = m[1].split(/[\s,\/]+/).filter(Boolean).map(Number);
        return fmt(p[0], p[1], p[2], p.length > 3 ? p[3] : 1);
      }
      if ((m = s.match(/^oklab\(([^)]*)\)$/))) {
        const p = m[1].split(/[\s\/]+/).filter(Boolean);
        const L = num(p[0]), A = parseFloat(p[1]), B = parseFloat(p[2]);
        const alpha = p.length > 3 ? num(p[3]) : 1;
        const l_ = L + 0.3963377774 * A + 0.2158037573 * B;
        const m_ = L - 0.1055613458 * A - 0.0638541728 * B;
        const s_ = L - 0.0894841775 * A - 1.291485548 * B;
        const l = l_ ** 3, mm = m_ ** 3, ss = s_ ** 3;
        const r = 4.0767416621 * l - 3.3077115913 * mm + 0.2309699292 * ss;
        const g = -1.2684380046 * l + 2.6097574011 * mm - 0.3413193965 * ss;
        const b = -0.0041960863 * l - 0.7034186147 * mm + 1.707614701 * ss;
        return fmt(lin2s(r), lin2s(g), lin2s(b), alpha);
      }
      if ((m = s.match(/^color\(srgb ([^)]*)\)$/))) {
        const p = m[1].split(/[\s\/]+/).filter(Boolean);
        return fmt(num(p[0]) * 255, num(p[1]) * 255, num(p[2]) * 255, p.length > 3 ? num(p[3]) : 1);
      }
      return s;
    };
    const norm = (v) => v.replace(/(?:rgba?|oklab|oklch|color)\([^()]*\)/g, (c) => color(c));
    window.__read = (el) => {
      const cs = getComputedStyle(el);
      const o = {};
      for (const p of PROPS) o[p] = norm(cs.getPropertyValue(p));
      return o;
    };
    window.__diff = (v, base) => PROPS.filter((p) => v[p] !== base[p]).map((p) => `${p}=${v[p]}`).join("|");
    const wrap = document.getElementById("wrap");
    const plain = document.createElement("div");
    wrap.appendChild(plain);
    window.__base = window.__read(plain);
    window.__baseLine = PROPS.map((p) => `${p}=${window.__base[p]}`).join("|");
  }, PROPS);

  // Plain rows: the element sits between two siblings, so first: and last: never apply.
  // first / last rows: the element is the first or last of two children.
  // notlast rows: the classes are on a parent; its first child (space-*, divide-* apply) against its last.
  const has = (s, v) => s.split(" ").some((c) => c.startsWith(v));
  const items = [
    ...classes.map((c) => ["class", bare(c)]),
    ...strings.map((s) => ["string", s]),
    ...strings.filter((s) => has(s, "first:")).map((s) => ["first", s]),
    ...strings.filter((s) => has(s, "last:")).map((s) => ["last", s]),
    ...strings.filter((s) => s.split(" ").some((c) => /^-?(space-[xy]|divide)-/.test(c) || c === "divide-y")).map((s) => ["notlast", s]),
  ];
  const out = await page.evaluate((items) => {
    const wrap = document.getElementById("wrap");
    const res = [];
    for (const [kind, cls] of items) {
      const holder = document.createElement("div");
      wrap.appendChild(holder);
      const el = document.createElement("div");
      el.className = cls;
      const sib = () => holder.appendChild(document.createElement("div"));
      let diff;
      if (kind === "first") {
        holder.appendChild(el);
        sib();
        diff = window.__diff(window.__read(el), window.__base);
      } else if (kind === "last") {
        sib();
        holder.appendChild(el);
        diff = window.__diff(window.__read(el), window.__base);
      } else if (kind === "notlast") {
        holder.className = cls;
        const a = sib();
        const b = sib();
        diff = window.__diff(window.__read(a), window.__read(b));
      } else {
        sib();
        holder.appendChild(el);
        sib();
        diff = window.__diff(window.__read(el), window.__base);
      }
      holder.remove();
      res.push([kind, cls, diff]);
    }
    return res;
  }, items);

  // focus-visible rows: the state forced on the element through DevTools (no real focus needed).
  const focusStrings = strings.filter((s) => has(s, "focus-visible:"));
  await page.evaluate((list) => {
    const wrap = document.getElementById("wrap");
    list.forEach((cls, i) => {
      const el = document.createElement("div");
      el.className = cls;
      el.id = `fv${i}`;
      wrap.appendChild(el);
      wrap.appendChild(document.createElement("div"));
    });
  }, focusStrings);
  const cdp = await page.createCDPSession();
  await cdp.send("DOM.enable");
  await cdp.send("CSS.enable");
  const { root } = await cdp.send("DOM.getDocument", { depth: -1 });
  for (let i = 0; i < focusStrings.length; i++) {
    const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: `#fv${i}` });
    await cdp.send("CSS.forcePseudoState", { nodeId, forcedPseudoClasses: ["focus-visible"] });
  }
  const focused = await page.evaluate((n) => [...Array(n).keys()].map((i) => window.__diff(window.__read(document.getElementById(`fv${i}`)), window.__base)), focusStrings.length);
  focusStrings.forEach((s, i) => out.push(["focus-visible", s, focused[i]]));

  const baseLine = await page.evaluate(() => window.__baseLine);
  const result = { base: baseLine, out };
  const lines = ["# kind\tclasses\tcomputed properties that differ from a plain div (generated by tools/oracle.mjs, do not edit)", `base\t\t${result.base}`];
  for (const [kind, cls, diff] of result.out) lines.push(`${kind}\t${cls}\t${diff}`);
  fs.writeFileSync(path.join(corpus, "computed.tsv"), lines.join("\n") + "\n");
  console.log(`computed.tsv: ${result.out.length} rows`);
} finally {
  await browser.close();
}
