// Renders specimen.txt in headless browsers with the launcher's own fonts and text CSS
// (launcher/public/fonts, app.css: -webkit-font-smoothing antialiased, .num ligatures off).
// Usage: node web.mjs [outDir]   (default: ./out)
// Engines: chrome-headless-shell from the puppeteer cache (what scripts/shots.mjs draws with,
// headless, never the owner's Chrome) and Playwright's WebKit when present. Writes
// web-<engine>.png at deviceScaleFactor 1. NODE_FROM points at a launcher dir with node_modules
// (default: the main checkout next to this worktree).
import { createRequire } from "node:module";
import { readFileSync, readdirSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(process.argv[2] ?? join(here, "out"));
mkdirSync(out, { recursive: true });
const launcher = resolve(here, "../../..");
const nodeFrom = process.env.NODE_FROM ?? [launcher].find((d) => existsSync(join(d, "node_modules/puppeteer-core")));
const require = createRequire(join(nodeFrom, "package.json"));

export function parseSpec(text) {
  const lines = [];
  for (const raw of text.split("\n")) {
    if (!raw.trim() || raw.startsWith("#")) continue;
    const bar = raw.indexOf(" | ");
    const [family, weight, size, line, tracking, features, color] = raw.slice(0, bar).trim().split(/\s+/);
    let text = raw.slice(bar + 3);
    let box = null;
    const m = text.match(/^@(\d+)([twc])(\d*) /);
    if (m) {
      box = { w: +m[1], mode: m[2], n: +(m[3] || 1) };
      text = text.slice(m[0].length);
    }
    lines.push({ family, weight: +weight, size: +size, line: +line, tracking: +tracking, features, color, text, box });
  }
  return lines;
}

// Same placement as examples/text_parity.rs.
export const X = 24;
export const GAP = 12;
export const WIDTH = 1600;
const COLORS = { fg: "rgb(242 242 243)", muted: "rgb(133 127 148)", pos: "rgb(42 223 114)", neg: "rgb(234 65 65)", white: "#fff" };

const spec = parseSpec(readFileSync(join(here, "specimen.txt"), "utf8"));
let y = 16;
const divs = spec.map((l) => {
  const top = y;
  y += l.line * (l.box?.n ?? 1) + GAP;
  const fam = l.family === "fira" ? "'Fira Code'" : "'Inter'";
  const extra = l.family === "fira" ? "font-variant-ligatures:none;font-variant-numeric:tabular-nums;" : l.features === "tnum" ? "font-variant-numeric:tabular-nums;" : "";
  const text = l.text.replace(/&/g, "&amp;").replace(/</g, "&lt;");
  const box = !l.box
    ? ""
    : l.box.mode === "t"
      ? `width:${l.box.w}px;overflow:hidden;text-overflow:ellipsis;`
      : l.box.mode === "w"
        ? `width:${l.box.w}px;white-space:normal;`
        : `width:${l.box.w}px;white-space:normal;display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:${l.box.n};overflow:hidden;`;
  return `<div style="top:${top}px;font:${l.weight} ${l.size}px/${l.line}px ${fam};letter-spacing:${l.tracking}em;color:${COLORS[l.color]};${extra}${box}">${text}</div>`;
});
const height = y + 16;
const fonts = pathToFileURL(join(launcher, "public/fonts")).href;
const html = `<!doctype html><html><head><meta charset="utf-8"><style>
@font-face{font-family:"Inter";src:url("${fonts}/inter-var.woff2") format("woff2");font-weight:100 900;font-display:block}
@font-face{font-family:"Fira Code";src:url("${fonts}/fira-code-400.woff2") format("woff2");font-weight:400;font-display:block}
@font-face{font-family:"Fira Code";src:url("${fonts}/fira-code-500.woff2") format("woff2");font-weight:500;font-display:block}
html,body{margin:0;background:rgb(16 12 24);-webkit-font-smoothing:antialiased;text-rendering:auto}
div{position:absolute;left:${X}px;white-space:pre}
</style></head><body>${divs.join("")}</body></html>`;
const page = join(out, "specimen.html");
writeFileSync(page, html);
const url = pathToFileURL(page).href;

async function settle(p) {
  await p.evaluate(async () => {
    await document.fonts.ready;
    await Promise.all([...document.fonts].map((f) => f.load()));
  });
  await new Promise((r) => setTimeout(r, 300));
}

// Layout widths of each line's box (fractional CSS px), one per line: text drives flex layout.
async function widths(p, engine) {
  const w = await p.evaluate(() => [...document.querySelectorAll("body > div")].map((d) => d.getBoundingClientRect().width));
  writeFileSync(join(out, `widths-${engine}.txt`), w.join("\n") + "\n");
}

async function chrome() {
  const puppeteer = require("puppeteer-core");
  const base = join(homedir(), ".cache/puppeteer/chrome-headless-shell");
  const ver = readdirSync(base).sort().pop();
  const dir = readdirSync(join(base, ver)).find((d) => d.startsWith("chrome-headless-shell"));
  const exe = join(base, ver, dir, "chrome-headless-shell");
  const b = await puppeteer.launch({ executablePath: exe, headless: true, args: ["--hide-scrollbars", "--force-device-scale-factor=1", "--allow-file-access-from-files"] });
  const p = await b.newPage();
  await p.setViewport({ width: WIDTH, height, deviceScaleFactor: 1 });
  await p.goto(url);
  await settle(p);
  await p.screenshot({ path: join(out, "web-chrome.png") });
  await widths(p, "chrome");
  await b.close();
  console.log(`chrome ${ver}: ${join(out, "web-chrome.png")}`);
}

async function webkit() {
  let pw;
  try {
    pw = require("playwright-core");
  } catch {
    return;
  }
  const b = await pw.webkit.launch({ headless: true });
  const p = await b.newPage({ viewport: { width: WIDTH, height }, deviceScaleFactor: 1 });
  await p.goto(url);
  await settle(p);
  await p.screenshot({ path: join(out, "web-webkit.png") });
  await widths(p, "webkit");
  await b.close();
  console.log(`webkit: ${join(out, "web-webkit.png")}`);
}

await chrome();
await webkit().catch((e) => console.log(`webkit skipped: ${e.message.split("\n")[0]}`));
