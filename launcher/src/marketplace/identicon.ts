// Copied from rare.market: market/web/lib/identicon/index.ts (wallet gem avatars).

// Deterministic gem-facet identicons. Pure SVG strings seeded by the wallet address, no network.
// avatarSvg: square mosaic (shown inside a circle). bannerSvg: wide, larger facets, soft blur, darkened toward the bottom.

function seedOf(s: string): number {
  let h = 2166136261;
  const a = s.toLowerCase();
  for (let i = 0; i < a.length; i++) {
    h ^= a.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  h ^= h >>> 15;
  h = Math.imul(h, 2246822519);
  h ^= h >>> 13;
  return h >>> 0;
}

function rng(seed: number): () => number {
  let a = seed || 1;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

type Pal = { hsl: Array<[number, number, number]> };

/** 3 harmonious colours: a base hue plus two neighbours (analogous, sometimes a split-complement accent). */
function palette(r: () => number): Pal {
  const h = r() * 360;
  const spread = 28 + r() * 40;
  const accent = r() < 0.35 ? 150 + r() * 60 : spread;
  return {
    hsl: [
      [h, 62 + r() * 18, 52 + r() * 8],
      [(h + spread) % 360, 58 + r() * 20, 44 + r() * 10],
      [(h + accent + 360) % 360, 55 + r() * 20, 38 + r() * 12],
    ],
  };
}

function mix(p: Pal, t: number, dl: number): string {
  const x = Math.min(0.999, Math.max(0, t)) * 2;
  const i = Math.floor(x);
  const f = x - i;
  const a = p.hsl[i];
  const b = p.hsl[i + 1];
  let dh = b[0] - a[0];
  if (dh > 180) dh -= 360;
  if (dh < -180) dh += 360;
  const h = (a[0] + dh * f + 360) % 360;
  const s = a[1] + (b[1] - a[1]) * f;
  const l = Math.min(86, Math.max(18, a[2] + (b[2] - a[2]) * f + dl));
  return `hsl(${h.toFixed(0)} ${s.toFixed(0)}% ${l.toFixed(0)}%)`;
}

function mosaic(r: () => number, p: Pal, w: number, h: number, cols: number, rows: number, shine: boolean): string {
  const cw = w / cols;
  const ch = h / rows;
  const pts: Array<Array<[number, number]>> = [];
  for (let y = 0; y <= rows; y++) {
    const row: Array<[number, number]> = [];
    for (let x = 0; x <= cols; x++) {
      const edgeX = x === 0 || x === cols;
      const edgeY = y === 0 || y === rows;
      // Points sit slightly outside the box on the edges so facets bleed past the circle crop.
      row.push([x * cw + (edgeX ? 0 : (r() - 0.5) * cw * 0.8), y * ch + (edgeY ? 0 : (r() - 0.5) * ch * 0.8)]);
    }
    pts.push(row);
  }
  const ang = r() * Math.PI * 2;
  const gx = Math.cos(ang);
  const gy = Math.sin(ang);
  const light = (r() - 0.5) * 2; // which way the facets catch light
  let out = "";
  const tri = (a: [number, number], b: [number, number], c: [number, number]) => {
    const cx = (a[0] + b[0] + c[0]) / 3;
    const cy = (a[1] + b[1] + c[1]) / 3;
    const t = 0.5 + ((cx / w - 0.5) * gx + (cy / h - 0.5) * gy) * 0.9 + (r() - 0.5) * 0.18;
    const dl = (r() - 0.5) * 16 + (cx / w - cy / h) * light * 8;
    out += `<path fill="${mix(p, t, dl)}" d="M${a[0].toFixed(1)} ${a[1].toFixed(1)}L${b[0].toFixed(1)} ${b[1].toFixed(1)}L${c[0].toFixed(1)} ${c[1].toFixed(1)}z"/>`;
  };
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) {
      const a = pts[y][x];
      const b = pts[y][x + 1];
      const c = pts[y + 1][x + 1];
      const d = pts[y + 1][x];
      if (r() < 0.5) {
        tri(a, b, c);
        tri(a, c, d);
      } else {
        tri(a, b, d);
        tri(b, c, d);
      }
    }
  }
  if (shine) {
    // Two soft highlight facets and a gloss gradient.
    const hx = w * (0.15 + r() * 0.4);
    const hy = h * (0.1 + r() * 0.3);
    out += `<path fill="#fff" fill-opacity=".16" d="M${hx.toFixed(1)} ${hy.toFixed(1)}l${(w * 0.32).toFixed(1)} ${(h * 0.06).toFixed(1)}l${(-w * 0.14).toFixed(1)} ${(h * 0.3).toFixed(1)}z"/>`;
    out += `<path fill="#fff" fill-opacity=".08" d="M${(w * 0.6).toFixed(1)} ${(h * 0.55).toFixed(1)}l${(w * 0.4).toFixed(1)} ${(-h * 0.1).toFixed(1)}l0 ${(h * 0.5).toFixed(1)}z"/>`;
  }
  return out;
}

const URI = (svg: string) => `data:image/svg+xml,${encodeURIComponent(svg)}`;
const cache = new Map<string, string>();
const CAP = 2000;
function memo(key: string, make: () => string): string {
  let v = cache.get(key);
  if (v === undefined) {
    if (cache.size >= CAP) cache.clear();
    v = make();
    cache.set(key, v);
  }
  return v;
}

export function avatarSvg(address: string): string {
  const r = rng(seedOf(address));
  const p = palette(r);
  const body = mosaic(r, p, 100, 100, 4, 4, false);
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" shape-rendering="geometricPrecision">` +
    `<defs><radialGradient id="g" cx=".3" cy=".25" r=".9"><stop offset="0" stop-color="#fff" stop-opacity=".28"/><stop offset=".5" stop-color="#fff" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity=".22"/></radialGradient></defs>` +
    `${body}<rect width="100" height="100" fill="url(#g)"/></svg>`
  );
}

export function bannerSvg(address: string): string {
  const r = rng(seedOf(address + "#banner"));
  const p = palette(rng(seedOf(address))); // same palette as the avatar
  const body = mosaic(r, p, 1600, 400, 7, 3, true);
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 400" preserveAspectRatio="xMidYMid slice">` +
    `<defs><filter id="b" x="-5%" y="-5%" width="110%" height="110%"><feGaussianBlur stdDeviation="14"/></filter>` +
    `<linearGradient id="d" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#000" stop-opacity=".05"/><stop offset="1" stop-color="#000" stop-opacity=".45"/></linearGradient></defs>` +
    `<g filter="url(#b)">${body}</g><rect width="1600" height="400" fill="url(#d)"/></svg>`
  );
}

export function avatarUri(address: string): string {
  return memo("a" + address, () => URI(avatarSvg(address)));
}

export function bannerUri(address: string): string {
  return memo("b" + address, () => URI(bannerSvg(address)));
}
