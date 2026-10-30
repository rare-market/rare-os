//! The share link's QR code (marketplace/BuySheet.tsx `Qr`: `qrcode` with margin 1, level M, dark
//! #100C18 on white, crisp edges), drawn at exactly the pixel size it shows at and handed to the
//! image loader as a PNG, so it sits in the scene like any image.

use image::ImageEncoder;
use qrcode::bits::Bits;
use qrcode::canvas::{Canvas, MaskPattern};
use qrcode::ec::construct_codewords;
use qrcode::{Color, EcLevel, Version};

/// Dark and light modules (BuySheet's colours).
const DARK: [u8; 4] = [0x10, 0x0c, 0x18, 255];
const LIGHT: [u8; 4] = [255, 255, 255, 255];

/// npm `qrcode`'s mask penalty (mask-pattern.js N1..N4) over a square matrix.
fn penalty(m: &[bool], n: usize) -> u32 {
    let at = |r: usize, c: usize| m[r * n + c];
    let mut pts = 0u32;
    // N1: runs of five or more in rows and columns.
    for r in 0..n {
        let (mut cr, mut cc) = (0u32, 0u32);
        let (mut lr, mut lc): (Option<bool>, Option<bool>) = (None, None);
        for c in 0..n {
            let v = at(r, c);
            if Some(v) == lc {
                cc += 1;
            } else {
                if cc >= 5 {
                    pts += 3 + (cc - 5);
                }
                lc = Some(v);
                cc = 1;
            }
            let v = at(c, r);
            if Some(v) == lr {
                cr += 1;
            } else {
                if cr >= 5 {
                    pts += 3 + (cr - 5);
                }
                lr = Some(v);
                cr = 1;
            }
        }
        if cc >= 5 {
            pts += 3 + (cc - 5);
        }
        if cr >= 5 {
            pts += 3 + (cr - 5);
        }
    }
    // N2: 2x2 blocks of one colour.
    let mut blocks = 0;
    for r in 0..n - 1 {
        for c in 0..n - 1 {
            let k = at(r, c) as u8 + at(r, c + 1) as u8 + at(r + 1, c) as u8 + at(r + 1, c + 1) as u8;
            if k == 0 || k == 4 {
                blocks += 1;
            }
        }
    }
    pts += blocks * 3;
    // N3: finder-like 1011101 with four light modules on a side.
    let mut finders = 0;
    for r in 0..n {
        let (mut bc, mut br) = (0u32, 0u32);
        for c in 0..n {
            bc = ((bc << 1) & 0x7ff) | at(r, c) as u32;
            if c >= 10 && (bc == 0x5d0 || bc == 0x05d) {
                finders += 1;
            }
            br = ((br << 1) & 0x7ff) | at(c, r) as u32;
            if c >= 10 && (br == 0x5d0 || br == 0x05d) {
                finders += 1;
            }
        }
    }
    pts += finders * 40;
    // N4: dark share away from half.
    let dark = m.iter().filter(|d| **d).count() as f64;
    let k = ((dark * 100.0 / m.len() as f64) / 5.0).ceil() as i32 - 10;
    pts + k.unsigned_abs() * 10
}

/// The QR modules of `text` with a one module margin: (side, dark?) row by row. Encoded the way
/// npm `qrcode` does it for a link: one byte segment, the smallest version, its mask choice.
pub fn modules(text: &str) -> Option<(usize, Vec<bool>)> {
    let (version, bits) = (1..=40).find_map(|v| {
        let version = Version::Normal(v);
        let mut b = Bits::new(version);
        (b.push_byte_data(text.as_bytes()).is_ok() && b.push_terminator(EcLevel::M).is_ok()).then_some((version, b))
    })?;
    let (data, ec) = construct_codewords(&bits.into_bytes(), version, EcLevel::M).ok()?;
    let mut canvas = Canvas::new(version, EcLevel::M);
    canvas.draw_all_functional_patterns();
    canvas.draw_data(&data, &ec);
    let masks = [
        MaskPattern::Checkerboard,
        MaskPattern::HorizontalLines,
        MaskPattern::VerticalLines,
        MaskPattern::DiagonalLines,
        MaskPattern::LargeCheckerboard,
        MaskPattern::Fields,
        MaskPattern::Diamonds,
        MaskPattern::Meadow,
    ];
    let mut best: Option<(u32, Vec<bool>)> = None;
    for p in masks {
        let mut c = canvas.clone();
        c.apply_mask(p);
        let m: Vec<bool> = c.into_colors().into_iter().map(|x| x == Color::Dark).collect();
        let w = (m.len() as f64).sqrt() as usize;
        let score = penalty(&m, w);
        if best.as_ref().is_none_or(|b| score < b.0) {
            best = Some((score, m));
        }
    }
    let m = best?.1;
    let w = (m.len() as f64).sqrt() as usize;
    let n = w + 2;
    let mut out = vec![false; n * n];
    for y in 0..w {
        for x in 0..w {
            out[(y + 1) * n + x + 1] = m[y * w + x];
        }
    }
    Some((n, out))
}

/// RGBA pixels, `px` square: each pixel takes the module under its top-left corner (an SVG with
/// shape-rendering crispEdges snaps the same way).
pub fn pixels(text: &str, px: u32) -> Option<Vec<u8>> {
    let (n, m) = modules(text)?;
    let mut rgba = Vec::with_capacity((px * px * 4) as usize);
    for y in 0..px {
        let my = (y as usize * n) / px as usize;
        for x in 0..px {
            let mx = (x as usize * n) / px as usize;
            rgba.extend_from_slice(if m[my * n + mx] { &DARK } else { &LIGHT });
        }
    }
    Some(rgba)
}

/// The PNG for `text` at `px` square.
pub fn png(text: &str, px: u32) -> Option<Vec<u8>> {
    let rgba = pixels(text, px)?;
    let mut buf = Vec::new();
    image::codecs::png::PngEncoder::new(&mut buf).write_image(&rgba, px, px, image::ExtendedColorType::Rgba8).ok()?;
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_link_is_a_small_qr_with_a_margin() {
        let (n, m) = modules("https://rare.market/s/dev-share-layout-only-3kq9VwT2xR7mYb4N8pLcZ").unwrap();
        assert!((27..=47).contains(&n), "{n}");
        // The margin is light, the finder pattern's corner dark.
        assert!(m[..n].iter().all(|d| !d));
        assert!(m[n + 1]);
        // npm qrcode picks version 5, mask 2 for this link: the same modules.
        let want = include_str!("fixtures/qr-npm.txt");
        let got: String = (1..n - 1).map(|y| (1..n - 1).map(|x| if m[y * n + x] { '#' } else { '.' }).collect::<String>() + "\n").collect();
        assert_eq!(got, want);
        let png = png("https://rare.market/s/x", 240).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
}
