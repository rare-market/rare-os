// Rare Render's one pipeline. Every primitive is an instanced quad; the fragment shader evaluates
// a rounded-rect signed distance for coverage, so corners, borders and the focus ring are exact
// at any scale with no tessellation and no MSAA.
//
// kind 0 rect (fill c0, border p.y in c1)      kind 1 linear gradient (c0 @0, c1 @q.z, c2 @1)
// kind 2 image (t1, premultiplied)             kind 3 glyph mask (atlas r * c0)
// kind 4 focus ring (gap p.y in c1, band q.z in c0)   kind 5 image alpha as mask * c0 (icons)
// kind 6 box shadow: rect is the shadow shape (offset + spread), p.x its radius, p.y the blur
//        sigma (CSS blur / 2), c0 the colour; uv/q.x the casting box and radius, cut out below
//        (CSS paints an outer shadow only outside the border box)
// kind 7 linear gradient segment: c0 at q.z, c1 at q.w along the CSS gradient line of direction
//        q.xy (sin a, -cos a); c2 = [first, last, oklab]: the first segment also fills before q.z,
//        the last after q.w; oklab interpolates premultiplied Oklab like `in oklab` (Tailwind v4)
// Corners: `radii` [tl, tr, br, bl] when any is set, else p.x on all four. Borders: `bw`
// [top, right, bottom, left] when any is set, else p.y on all sides (kind 0; kind 6 keeps the
// casting box's corners there). The rounded clip's corners are `cr` (one value in cr.x: all four).
// Colours are straight-alpha sRGB; output is premultiplied (blend One, OneMinusSrcAlpha).

struct Globals {
    viewport: vec2<f32>,
    atlas_size: vec2<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(1) @binding(0) var img: texture_2d<f32>;

struct Inst {
    @location(0) rect: vec4<f32>,
    @location(1) clip: vec4<f32>,
    @location(2) c0: vec4<f32>,
    @location(3) c1: vec4<f32>,
    @location(4) c2: vec4<f32>,
    @location(5) uv: vec4<f32>,
    @location(6) p: vec4<f32>,
    @location(7) q: vec4<f32>,
    @location(8) rclip: vec4<f32>,
    @location(9) cr: vec4<f32>,
    @location(10) radii: vec4<f32>,
    @location(11) bw: vec4<f32>,
};

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) px: vec2<f32>,
    @location(1) @interpolate(flat) rect: vec4<f32>,
    @location(2) @interpolate(flat) clip: vec4<f32>,
    @location(3) @interpolate(flat) c0: vec4<f32>,
    @location(4) @interpolate(flat) c1: vec4<f32>,
    @location(5) @interpolate(flat) c2: vec4<f32>,
    @location(6) @interpolate(flat) uv: vec4<f32>,
    @location(7) @interpolate(flat) p: vec4<f32>,
    @location(8) @interpolate(flat) q: vec4<f32>,
    @location(9) @interpolate(flat) rclip: vec4<f32>,
    @location(10) @interpolate(flat) cr: vec4<f32>,
    @location(11) @interpolate(flat) radii: vec4<f32>,
    @location(12) @interpolate(flat) bw: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, i: Inst) -> VOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let kind = i.p.z;
    var grow = 1.0;
    if (kind == 3.0) {
        grow = 0.0;
    } else if (kind == 4.0) {
        grow = i.p.y + i.q.z + 1.0;
    } else if (kind == 6.0) {
        grow = 3.0 * i.p.y + 1.0;
    }
    var r0 = i.rect.xy - vec2<f32>(grow);
    var size = i.rect.zw + vec2<f32>(2.0 * grow);
    // Never rasterise outside the clip.
    let lo = max(r0, i.clip.xy);
    let hi = min(r0 + size, i.clip.zw);
    let px = mix(lo, max(hi, lo), corner);
    var o: VOut;
    o.pos = vec4<f32>(px.x / g.viewport.x * 2.0 - 1.0, 1.0 - px.y / g.viewport.y * 2.0, 0.0, 1.0);
    o.px = px;
    o.rect = i.rect;
    o.clip = i.clip;
    o.c0 = i.c0;
    o.c1 = i.c1;
    o.c2 = i.c2;
    o.uv = i.uv;
    o.p = i.p;
    o.q = i.q;
    o.rclip = i.rclip;
    o.cr = i.cr;
    o.radii = i.radii;
    o.bw = i.bw;
    return o;
}

fn sd_rrect(p: vec2<f32>, center: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p - center) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// The radius of the corner `d` (offset from the centre, y down) points at: r4 = [tl, tr, br, bl].
fn corner_of(d: vec2<f32>, r4: vec4<f32>) -> f32 {
    let top = select(r4.x, r4.y, d.x > 0.0);
    let bottom = select(r4.w, r4.z, d.x > 0.0);
    return select(top, bottom, d.y > 0.0);
}

fn sd_rrect4(p: vec2<f32>, center: vec2<f32>, half: vec2<f32>, r4: vec4<f32>) -> f32 {
    return sd_rrect(p, center, half, corner_of(p - center, r4));
}

// Per-corner radii of a prim, each at most half the box.
fn radii_of(given: vec4<f32>, uniform: f32, half: vec2<f32>) -> vec4<f32> {
    var r = vec4<f32>(uniform);
    if (any(given > vec4<f32>(0.0))) {
        r = given;
    }
    return min(r, vec4<f32>(min(half.x, half.y)));
}

fn srgb_to_lin(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

fn lin_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, 12.92 * c, c <= vec3<f32>(0.0031308));
}

fn cbrt3(v: vec3<f32>) -> vec3<f32> {
    return sign(v) * pow(abs(v), vec3<f32>(1.0 / 3.0));
}

fn to_oklab(srgb: vec3<f32>) -> vec3<f32> {
    let c = srgb_to_lin(srgb);
    let lms = vec3<f32>(
        0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b,
        0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b,
        0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b,
    );
    let l = cbrt3(lms);
    return vec3<f32>(
        0.2104542553 * l.x + 0.7936177850 * l.y - 0.0040720468 * l.z,
        1.9779984951 * l.x - 2.4285922050 * l.y + 0.4505937099 * l.z,
        0.0259040371 * l.x + 0.7827717662 * l.y - 0.8086757660 * l.z,
    );
}

fn from_oklab(lab: vec3<f32>) -> vec3<f32> {
    let l_ = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
    let m_ = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
    let s_ = lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    let c = vec3<f32>(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    );
    return lin_to_srgb(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)));
}

// CSS `in oklab`: premultiplied Oklab, the result premultiplied sRGB.
fn mix_oklab(a: vec4<f32>, b: vec4<f32>, t: f32) -> vec4<f32> {
    let pa = vec4<f32>(to_oklab(a.rgb) * a.a, a.a);
    let pb = vec4<f32>(to_oklab(b.rgb) * b.a, b.a);
    let m = mix(pa, pb, t);
    if (m.a <= 1e-5) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(from_oklab(m.rgb / m.a) * m.a, m.a);
}

// Box shadow after Evan Wallace's "Fast Rounded Rectangle Shadows": exact Gaussian blur along x
// (erf), numerically integrated along y.
fn erf2(v: vec2<f32>) -> vec2<f32> {
    let s = sign(v);
    let a = abs(v);
    let r1 = 1.0 + (0.278393 + (0.230389 + (0.000972 + 0.078108 * a) * a) * a) * a;
    let r2 = r1 * r1;
    return s - s / (r2 * r2);
}

fn gauss(x: f32, sigma: f32) -> f32 {
    return exp(-(x * x) / (2.0 * sigma * sigma)) / (2.5066283 * sigma);
}

fn blur_x(x: f32, y: f32, sigma: f32, corner: f32, half: vec2<f32>) -> f32 {
    let delta = min(half.y - corner - abs(y), 0.0);
    let curved = half.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
    let integral = 0.5 + 0.5 * erf2((vec2<f32>(x) + vec2<f32>(-curved, curved)) * (0.70710678 / sigma));
    return integral.y - integral.x;
}

fn shadow_alpha(p: vec2<f32>, center: vec2<f32>, half: vec2<f32>, corner: f32, sigma: f32) -> f32 {
    let d = p - center;
    let low = d.y - half.y;
    let high = d.y + half.y;
    let start = clamp(-3.0 * sigma, low, high);
    let end = clamp(3.0 * sigma, low, high);
    let step = (end - start) / 8.0;
    var y = start + step * 0.5;
    var a = 0.0;
    for (var i = 0; i < 8; i = i + 1) {
        a = a + blur_x(d.x, d.y - y, sigma, corner, half) * gauss(y, sigma) * step;
        y = y + step;
    }
    return a;
}

fn premul(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb * c.a, c.a);
}

@fragment
fn fs(v: VOut) -> @location(0) vec4<f32> {
    let kind = v.p.z;
    let op = v.p.w;
    let half = v.rect.zw * 0.5;
    let center = v.rect.xy + half;
    let r4 = radii_of(v.radii, v.p.x, half);
    let r = corner_of(v.px - center, r4);
    let d = sd_rrect(v.px, center, half, r);
    let cover = clamp(0.5 - d, 0.0, 1.0);
    let local = clamp((v.px - v.rect.xy) / max(v.rect.zw, vec2<f32>(1e-3)), vec2<f32>(0.0), vec2<f32>(1.0));
    let tuv = mix(v.uv.xy, v.uv.zw, local);
    // Sampled for every kind (uniform control flow); explicit LOD needs no derivatives.
    let t1 = textureSampleLevel(img, samp, tuv, 0.0);
    let ta = textureSampleLevel(atlas, samp, tuv, 0.0).r;

    var col = vec4<f32>(0.0);
    if (kind == 0.0) {
        var bw = vec4<f32>(v.p.y);
        if (any(v.bw > vec4<f32>(0.0))) {
            bw = v.bw;
        }
        var c = premul(v.c0);
        if (any(bw > vec4<f32>(0.0))) {
            // The padding box: the border box inset by each side, its corners reduced by the
            // borders that meet there.
            let lo = v.rect.xy + vec2<f32>(bw.w, bw.x);
            let hi = v.rect.xy + v.rect.zw - vec2<f32>(bw.y, bw.z);
            let ih = max((hi - lo) * 0.5, vec2<f32>(0.0));
            let ir = max(r4 - vec4<f32>(max(bw.w, bw.x), max(bw.y, bw.x), max(bw.y, bw.z), max(bw.w, bw.z)), vec4<f32>(0.0));
            var inner = 0.0;
            if (ih.x > 0.0 && ih.y > 0.0) {
                inner = clamp(0.5 - sd_rrect4(v.px, lo + ih, ih, min(ir, vec4<f32>(min(ih.x, ih.y)))), 0.0, 1.0);
            }
            c = mix(premul(v.c1), c, inner);
        }
        col = c * cover;
    } else if (kind == 1.0) {
        var t = dot(local, v.q.xy) + v.q.w;
        t = clamp(t, 0.0, 1.0);
        let mid = v.q.z;
        var c: vec4<f32>;
        if (t < mid) {
            c = mix(premul(v.c0), premul(v.c1), t / max(mid, 1e-4));
        } else {
            c = mix(premul(v.c1), premul(v.c2), (t - mid) / max(1.0 - mid, 1e-4));
        }
        col = c * cover;
    } else if (kind == 2.0) {
        col = t1 * cover;
    } else if (kind == 3.0) {
        col = premul(v.c0) * ta;
    } else if (kind == 4.0) {
        let gap = v.p.y;
        let band = v.q.z;
        // Outside the box only: gap in the page colour, then the white band.
        let outside = clamp(d + 0.5, 0.0, 1.0);
        let in_outer = clamp(gap + band + 0.5 - d, 0.0, 1.0);
        let in_gap = clamp(gap + 0.5 - d, 0.0, 1.0);
        let ring = premul(v.c0) * (in_outer - in_gap) + premul(v.c1) * in_gap;
        col = ring * outside;
    } else if (kind == 5.0) {
        col = premul(v.c0) * t1.a * cover;
    } else if (kind == 6.0) {
        let sigma = v.p.y;
        var a = cover;
        if (sigma > 0.05) {
            a = shadow_alpha(v.px, center, half, r, sigma);
        }
        let bh = v.uv.zw * 0.5;
        let br4 = radii_of(v.bw, v.q.x, bh);
        let inside = clamp(0.5 - sd_rrect4(v.px, v.uv.xy + bh, bh, br4), 0.0, 1.0);
        col = premul(v.c0) * a * (1.0 - inside);
    } else if (kind == 7.0) {
        let dir = v.q.xy;
        let len = max(abs(v.rect.z * dir.x) + abs(v.rect.w * dir.y), 1e-3);
        let t = dot(v.px - center, dir) / len + 0.5;
        let t0 = v.q.z;
        let t1g = v.q.w;
        let before = t < t0 && v.c2.x < 0.5;
        let after = t >= t1g && v.c2.y < 0.5;
        if (!before && !after) {
            let f = clamp((t - t0) / max(t1g - t0, 1e-5), 0.0, 1.0);
            var c: vec4<f32>;
            if (v.c2.z > 0.5) {
                c = mix_oklab(v.c0, v.c1, f);
            } else {
                c = mix(premul(v.c0), premul(v.c1), f);
            }
            col = c * cover;
        }
    }
    // Hard clip (rails, panels); the vertex stage already trimmed the quad to it.
    if (v.px.x < v.clip.x || v.px.y < v.clip.y || v.px.x > v.clip.z || v.px.y > v.clip.w) {
        col = vec4<f32>(0.0);
    }
    // Rounded clip (overflow-hidden on a rounded box): anti-aliased corners.
    if (any(v.cr > vec4<f32>(0.0))) {
        var cr4 = v.cr;
        if (all(v.cr.yzw == vec3<f32>(0.0))) {
            cr4 = vec4<f32>(v.cr.x);
        }
        let ch = (v.rclip.zw - v.rclip.xy) * 0.5;
        let cd = sd_rrect4(v.px, v.rclip.xy + ch, ch, min(cr4, vec4<f32>(min(ch.x, ch.y))));
        col = col * clamp(0.5 - cd, 0.0, 1.0);
    }
    return col * op;
}
