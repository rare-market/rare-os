#!/usr/bin/env node
// Rare default profile pictures: flat SVG avatars drawn from code, one per entry in AVATARS.
//
//   node tools/avatars/gen.mjs                       SVGs + the launcher's list (launcher/native)
//   node tools/avatars/gen.mjs --market <repo>       also PNG/WebP at 512 and 128 + manifests for rare.market
//   node tools/avatars/gen.mjs --sheet <file.png>    a contact sheet of the set (headless Chrome, muted)
//
// To add one: append to AVATARS (never rename or reorder: ids are saved on accounts, and the first
// STARTERS entries pick each account's default by address hash). Keep the style: a flat mid-dark
// background, two or three flat colours, no gradients, glows or outlines, everything inside the
// inscribed circle (the picture is shown round).

import { mkdirSync, writeFileSync, readFileSync, rmSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

// ---------- palette ----------
const BG = {
  ink: "#1E2A3E",
  pine: "#1C3029",
  plum: "#33243C",
  clay: "#3B2822",
  slate: "#2A3038",
  teal: "#16343A",
  olive: "#2E3122",
  wine: "#3B1F2A",
  navy: "#1A2547",
  umber: "#352B1E",
};
const C = {
  bone: "#ECE6D9",
  sand: "#D9BC8F",
  coral: "#E37B5D",
  sage: "#94B69C",
  sky: "#8FB9DE",
  lilac: "#B4A6DE",
  gold: "#E7B54E",
  rose: "#E198A8",
  mint: "#80CDB7",
  rust: "#C0583B",
  moss: "#6F905F",
  dusk: "#5D71AA",
  night: "#121826",
};

// ---------- helpers ----------
const circle = (cx, cy, r, f) => `<circle cx="${cx}" cy="${cy}" r="${r}" fill="${f}"/>`;
const rect = (x, y, w, h, f, rx = 0) => `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${rx}" fill="${f}"/>`;
const path = (d, f) => `<path d="${d}" fill="${f}"/>`;
const poly = (pts, f) => `<polygon points="${pts.map((p) => p.join(",")).join(" ")}" fill="${f}"/>`;
const ellipse = (cx, cy, rx, ry, f, rot = 0) => `<ellipse cx="${cx}" cy="${cy}" rx="${rx}" ry="${ry}" fill="${f}"${rot ? ` transform="rotate(${rot} ${cx} ${cy})"` : ""}/>`;
const stroke = (d, c, w, cap = "round") => `<path d="${d}" fill="none" stroke="${c}" stroke-width="${w}" stroke-linecap="${cap}" stroke-linejoin="round"/>`;
// a sine band from x0 to x1 at y with amplitude a and wavelength l
const wave = (y, a, l, x0 = 0, x1 = 512) => {
  let d = `M${x0} ${y}`;
  for (let x = x0; x < x1; x += l) d += ` q${l / 4} ${-a} ${l / 2} 0 t${l / 2} 0`;
  return d;
};
const eyes = (cx, cy, gap, r, f) => circle(cx - gap, cy, r, f) + circle(cx + gap, cy, r, f);

// ---------- the set ----------
// [id, name, background, body]. The first STARTERS are the ones handed out as defaults.
export const AVATARS = [
  ["dune", "Dune", BG.clay, () => circle(318, 200, 56, C.gold) + path("M40 340q110-90 230-30t230 10v200H40z", C.sand) + path("M40 400q130-70 250-10t220 0v130H40z", C.rust)],
  ["peak", "Peak", BG.slate, () => circle(340, 170, 38, C.coral) + poly([[100, 380], [236, 176], [372, 380]], C.dusk) + poly([[236, 176], [192, 242], [214, 232], [236, 252], [258, 232], [280, 242]], C.bone) + poly([[250, 380], [338, 250], [426, 380]], C.sky)],
  ["orbit", "Orbit", BG.ink, () => `<g transform="rotate(-22 256 256)"><ellipse cx="256" cy="256" rx="170" ry="44" fill="none" stroke="${C.bone}" stroke-width="16"/>${circle(256, 256, 104, C.lilac)}<ellipse cx="256" cy="256" rx="170" ry="44" fill="none" stroke="${C.bone}" stroke-width="16" clip-path="url(#front)"/></g>` + circle(392, 150, 12, C.gold)],
  ["bloom", "Bloom", BG.wine, () => circle(256, 176, 68, C.rose) + circle(336, 256, 68, C.rose) + circle(256, 336, 68, C.rose) + circle(176, 256, 68, C.rose) + circle(256, 256, 46, C.gold)],
  ["prism", "Prism", BG.navy, () => poly([[256, 120], [388, 352], [124, 352]], C.sky) + poly([[256, 120], [256, 352], [124, 352]], C.lilac) + poly([[256, 268], [318, 352], [194, 352]], C.bone)],
  ["arch", "Arch", BG.umber, () => path("M108 380a148 148 0 0 1 296 0z", C.coral) + path("M152 380a104 104 0 0 1 208 0z", C.sand) + path("M196 380a60 60 0 0 1 120 0z", C.sage) + rect(96, 380, 320, 10, C.bone, 5)],
  ["sunrise", "Sunrise", BG.plum, () => `<g clip-path="url(#sun)">${circle(256, 300, 140, C.gold)}</g>` + rect(96, 300, 320, 14, C.coral, 7) + rect(126, 336, 260, 14, C.coral, 7) + rect(166, 372, 180, 14, C.coral, 7)],
  ["pebble", "Pebble", BG.pine, () => ellipse(256, 352, 120, 46, C.sage) + ellipse(256, 280, 88, 38, C.sand) + ellipse(256, 218, 58, 30, C.bone) + ellipse(262, 170, 30, 20, C.mint)],
  ["leaf", "Leaf", BG.pine, () => path("M256 104c96 70 120 200 0 304c-120-104-96-234 0-304z", C.sage) + stroke("M256 160v236", BG.pine, 12) + stroke("M256 250l52-40M256 310l-52-40", BG.pine, 10)],
  ["cube", "Cube", BG.slate, () => poly([[256, 112], [384, 184], [256, 256], [128, 184]], C.bone) + poly([[128, 184], [256, 256], [256, 400], [128, 328]], C.sky) + poly([[256, 256], [384, 184], [384, 328], [256, 400]], C.dusk)],
  ["eclipse", "Eclipse", BG.ink, () => circle(256, 256, 128, C.gold) + circle(300, 226, 118, BG.ink) + circle(176, 330, 10, C.gold)],
  ["quartz", "Quartz", BG.teal, () => poly([[256, 100], [336, 180], [336, 330], [256, 410], [176, 330], [176, 180]], C.mint) + poly([[256, 100], [336, 180], [256, 230], [176, 180]], C.bone) + poly([[256, 230], [336, 180], [336, 330], [256, 410]], C.sage)],
  ["mushroom", "Mushroom", BG.olive, () => rect(214, 260, 84, 140, C.bone, 34) + path("M96 280a160 150 0 0 1 320 0z", C.rust) + circle(196, 214, 22, C.bone) + circle(290, 186, 16, C.bone) + circle(332, 244, 14, C.bone)],
  ["cloud", "Cloud", BG.ink, () => circle(320, 188, 64, C.gold) + path("M150 352a62 62 0 0 1 8-122a82 82 0 0 1 156-22a64 64 0 0 1 48 144z", C.bone)],
  ["owl", "Owl", BG.umber, () => path("M136 170l40 40h160l40-40v170a120 110 0 0 1-240 0z", C.sand) + circle(208, 252, 46, C.bone) + circle(304, 252, 46, C.bone) + circle(214, 256, 20, C.night) + circle(298, 256, 20, C.night) + poly([[240, 300], [272, 300], [256, 330]], C.coral)],
  ["cat", "Cat", BG.plum, () => poly([[140, 236], [156, 116], [236, 180]], C.lilac) + poly([[372, 236], [356, 116], [276, 180]], C.lilac) + circle(256, 276, 130, C.lilac) + ellipse(206, 264, 14, 24, C.night) + ellipse(306, 264, 14, 24, C.night) + poly([[244, 312], [268, 312], [256, 326]], C.rose)],
  ["fox", "Fox", BG.pine, () => poly([[120, 140], [256, 236], [392, 140], [372, 290], [256, 400], [140, 290]], C.coral) + poly([[140, 290], [256, 400], [372, 290], [256, 320]], C.bone) + circle(208, 262, 14, C.night) + circle(304, 262, 14, C.night) + circle(256, 384, 14, C.night)],
  ["bear", "Bear", BG.slate, () => circle(160, 160, 46, C.sand) + circle(352, 160, 46, C.sand) + circle(256, 272, 136, C.sand) + ellipse(256, 322, 62, 48, C.bone) + circle(208, 252, 13, C.night) + circle(304, 252, 13, C.night) + ellipse(256, 304, 22, 15, C.night)],
  ["frog", "Frog", BG.navy, () => circle(186, 186, 54, C.sage) + circle(326, 186, 54, C.sage) + ellipse(256, 300, 156, 108, C.sage) + circle(186, 186, 26, C.bone) + circle(326, 186, 26, C.bone) + circle(190, 190, 12, C.night) + circle(322, 190, 12, C.night) + stroke("M206 322q50 34 100 0", C.night, 12)],
  ["robot", "Robot", BG.wine, () => rect(250, 96, 12, 50, C.bone, 6) + circle(256, 96, 16, C.coral) + rect(132, 146, 248, 220, C.bone, 56) + rect(168, 202, 176, 96, C.night, 40) + circle(214, 250, 20, C.mint) + circle(298, 250, 20, C.mint) + rect(212, 322, 88, 12, C.night, 6)],
  ["bird", "Bird", BG.teal, () => circle(240, 260, 120, C.gold) + path("M236 230a96 96 0 0 0 80 140a128 128 0 0 1-176-40z", C.coral) + poly([[356, 238], [416, 256], [356, 276]], C.coral) + circle(296, 226, 14, C.night)],
  ["star", "Star", BG.plum, () => path("M256 104q18 134 152 152q-134 18-152 152q-18-134-152-152q134-18 152-152z", C.gold) + path("M366 132q6 40 46 46q-40 6-46 46q-6-40-46-46q40-6 46-46z", C.bone)],
  ["dice", "Dice", BG.wine, () => rect(136, 136, 240, 240, C.bone, 44) + circle(196, 196, 22, C.rust) + circle(316, 196, 22, C.rust) + circle(256, 256, 22, C.rust) + circle(196, 316, 22, C.rust) + circle(316, 316, 22, C.rust)],
  ["sprout", "Sprout", BG.umber, () => path("M176 316h160l-20 92h-120z", C.rust) + rect(164, 300, 184, 30, C.coral, 8) + stroke("M256 300v-90", C.sage, 14) + path("M256 222c-10-60-60-86-110-80c6 54 50 86 110 80z", C.sage) + path("M256 196c14-56 62-78 110-70c-8 52-54 80-110 70z", C.moss)],
  ["pine", "Pine", BG.pine, () => poly([[256, 100], [346, 222], [166, 222]], C.sage) + poly([[256, 160], [370, 312], [142, 312]], C.moss) + rect(236, 312, 40, 76, C.sand, 6) + circle(380, 140, 24, C.bone)],
  ["kite", "Kite", BG.olive, () => poly([[256, 100], [356, 220], [256, 380], [156, 220]], C.coral) + poly([[256, 100], [356, 220], [256, 220]], C.gold) + poly([[156, 220], [256, 220], [256, 380]], C.sand) + stroke("M256 380q-30 30 0 56", C.bone, 8)],
  ["seed", "Seed", BG.clay, () => circle(256, 256, 140, C.sand) + [0, 60, 120, 180, 240, 300].map((a) => `<ellipse cx="256" cy="172" rx="18" ry="40" fill="${C.rust}" transform="rotate(${a} 256 256)"/>`).join("") + circle(256, 256, 30, C.gold)],
  ["tide", "Tide", BG.teal, () => `<g clip-path="url(#inner)">${stroke(wave(196, 30, 112, 0, 560), C.mint, 30)}${stroke(wave(262, 30, 112, -56, 560), C.sky, 30)}${stroke(wave(328, 30, 112, 0, 560), C.bone, 30)}</g>`],
  ["bolt", "Bolt", BG.slate, () => poly([[290, 96], [156, 284], [246, 284], [222, 416], [356, 222], [266, 222]], C.gold)],
];

export const STARTERS = 24;

const DEFS = `<defs><clipPath id="inner"><circle cx="256" cy="256" r="196"/></clipPath><clipPath id="sun"><rect x="0" y="0" width="512" height="300"/></clipPath><clipPath id="front"><rect x="0" y="256" width="512" height="256"/></clipPath></defs>`;

export function svg([id, , bg, body]) {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">${DEFS}<rect width="512" height="512" fill="${bg}"/>${body()}</svg>`;
}

/** FNV-1a over the normalised address, the same in Rust (launcher), TypeScript (API and web). */
export function starterIndex(address, n = STARTERS) {
  const hex = String(address).toLowerCase().replace(/^0x/, "").padStart(64, "0");
  let h = 0x811c9dc5;
  for (const ch of hex) {
    h ^= ch.charCodeAt(0);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h % n;
}

// ---------- outputs ----------
const args = process.argv.slice(2);
const flag = (k) => {
  const i = args.indexOf(k);
  return i >= 0 ? args[i + 1] : null;
};

const ids = new Set();
for (const a of AVATARS) {
  if (ids.has(a[0]) || !/^[a-z][a-z0-9-]{1,23}$/.test(a[0])) throw new Error(`bad or repeated id ${a[0]}`);
  ids.add(a[0]);
}

const native = join(ROOT, "launcher/native/assets/avatars");
rmSync(native, { recursive: true, force: true });
mkdirSync(native, { recursive: true });
for (const a of AVATARS) writeFileSync(join(native, `${a[0]}.svg`), svg(a) + "\n");
writeFileSync(
  join(ROOT, "launcher/native/src/avatars/set.rs"),
  `// Generated by tools/avatars/gen.mjs. Do not edit.\n\n/// (id, name, svg). The first \`STARTERS\` are handed out as defaults by address.\npub const SET: &[(&str, &str, &str)] = &[\n${AVATARS.map(([id, name]) => `    ("${id}", "${name}", include_str!("../../assets/avatars/${id}.svg")),`).join("\n")}\n];\n\npub const STARTERS: usize = ${STARTERS};\n`,
);
console.log(`${AVATARS.length} avatars -> ${native}`);

const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
function chrome(htmlFile, extra) {
  const profile = join(tmpdir(), `rare-avatars-chrome-${process.pid}`);
  try {
    return execFileSync(CHROME, ["--headless=new", "--disable-gpu", "--mute-audio", "--no-first-run", "--hide-scrollbars", `--user-data-dir=${profile}`, "--allow-file-access-from-files", ...extra, `file://${htmlFile}`], { maxBuffer: 1 << 30, stdio: ["ignore", "pipe", "ignore"] }).toString();
  } finally {
    rmSync(profile, { recursive: true, force: true });
  }
}

const market = flag("--market");
if (market) {
  const dir = resolve(market);
  const pub = join(dir, "web/public/avatars");
  rmSync(pub, { recursive: true, force: true });
  for (const s of [512, 128]) mkdirSync(join(pub, String(s)), { recursive: true });
  const work = join(tmpdir(), `rare-avatars-${process.pid}.html`);
  const list = AVATARS.map((a) => [a[0], "data:image/svg+xml;base64," + Buffer.from(svg(a)).toString("base64")]);
  writeFileSync(
    work,
    `<!doctype html><body><pre id="o"></pre><script>
const list = ${JSON.stringify(list)};
(async () => {
  const out = {};
  for (const [id, src] of list) {
    const im = new Image(); im.src = src; await im.decode();
    for (const s of [512, 128]) {
      const c = document.createElement("canvas"); c.width = c.height = s;
      const x = c.getContext("2d"); x.imageSmoothingQuality = "high"; x.drawImage(im, 0, 0, s, s);
      out[id + "@" + s + ".png"] = c.toDataURL("image/png");
      out[id + "@" + s + ".webp"] = c.toDataURL("image/webp", 0.9);
    }
  }
  const mark = "AVATARS" + "_OUT"; document.getElementById("o").textContent = mark + JSON.stringify(out) + mark;
})();
</script></body>`,
  );
  const dom = chrome(work, ["--virtual-time-budget=20000", "--dump-dom"]);
  rmSync(work, { force: true });
  const m = dom.match(/AVATARS_OUT(.*?)AVATARS_OUT/s);
  if (!m) throw new Error("Chrome did not render the set");
  const out = JSON.parse(m[1].replace(/&quot;/g, '"').replace(/&amp;/g, "&"));
  for (const [k, v] of Object.entries(out)) {
    const [id, rest] = k.split("@");
    const [size, ext] = rest.split(".");
    writeFileSync(join(pub, size, `${id}.${ext}`), Buffer.from(v.split(",")[1], "base64"));
  }
  for (const a of AVATARS) writeFileSync(join(pub, `${a[0]}.svg`), svg(a) + "\n");
  const manifest = { starters: STARTERS, avatars: AVATARS.map(([id, name]) => ({ id, name })) };
  writeFileSync(join(pub, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  for (const f of ["api/src/avatars.json", "web/lib/avatars.json"]) writeFileSync(join(dir, f), JSON.stringify(manifest, null, 2) + "\n");
  console.log(`PNG/WebP 512 and 128 -> ${pub}`);
}

const sheet = flag("--sheet");
if (sheet) {
  const work = join(tmpdir(), `rare-avatars-sheet-${process.pid}.html`);
  const cols = 6;
  const rows = Math.ceil(AVATARS.length / cols);
  const cell = 190;
  const W = cols * cell + 64;
  const H = rows * (cell + 26) + 132;
  const tiles = AVATARS.map((a, i) => {
    const src = "data:image/svg+xml;base64," + Buffer.from(svg(a)).toString("base64");
    return `<figure><div class="r"><img src="${src}"><img src="${src}" class="sm"></div><figcaption>${a[0]}</figcaption></figure>`;
  }).join("");
  writeFileSync(
    work,
    `<!doctype html><html><head><style>
body{margin:0;background:#0E0F11;color:#9A9DA3;font:500 13px/1.2 -apple-system,Inter,system-ui,sans-serif;padding:32px}
h1{margin:0 0 24px;color:#ECECEE;font-size:18px;font-weight:600}
.g{display:grid;grid-template-columns:repeat(${cols},${cell - 14}px);gap:26px 14px}
figure{margin:0;display:flex;flex-direction:column;align-items:center;gap:8px}
img{width:${cell - 52}px;height:${cell - 52}px;border-radius:50%;display:block}
.r{display:flex;align-items:flex-end;gap:6px}img.sm{width:28px;height:28px}
</style></head><body><h1>Rare default pictures (${AVATARS.length})</h1><div class="g">${tiles}</div></body></html>`,
  );
  chrome(work, [`--window-size=${W},${H}`, `--screenshot=${resolve(sheet)}`]);
  rmSync(work, { force: true });
  console.log(`sheet -> ${resolve(sheet)}`);
}
