//! Icons: lucide (components/icons.tsx draws them at stroke 1.75, the set rare.market uses) and the
//! launcher's own marks (the Sui droplet, Steam, the verified badge), as SVG rasterised by the
//! image workers at the size they are drawn and tinted at paint time.

use super::lucide;
use super::ui::{s, Ui, S};
use rare_render::view::{Content, El};
use rare_render::{Color, Fit, ImgReq, Src};
use std::collections::HashMap;
use std::sync::Mutex;

/// An SVG's bytes, kept for the process (image requests carry a static source).
pub fn svg_src(name: &str, svg: String) -> Src {
    static CACHE: Mutex<Option<HashMap<String, (&'static str, &'static [u8])>>> = Mutex::new(None);
    let mut g = CACHE.lock().unwrap();
    let map = g.get_or_insert_with(HashMap::new);
    let key = format!("{name}|{svg}");
    let (n, b) = *map.entry(key).or_insert_with(|| {
        let n: &'static str = Box::leak(format!("{name}.svg").into_boxed_str());
        let b: &'static [u8] = Box::leak(svg.into_bytes().into_boxed_slice());
        (n, b)
    });
    Src::Static(n, b)
}

/// A lucide icon's SVG, drawn white (tinted when painted).
pub fn lucide_svg(name: &str, stroke: f32, fill: bool) -> String {
    let body = lucide::body(name).unwrap_or_else(|| {
        // A name the kit does not carry draws nothing: say so once (add it to tools/lucide.mjs).
        static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let mut seen = SEEN.lock().unwrap();
        if !seen.iter().any(|n| n == name) {
            seen.push(name.to_string());
            log::warn!("icon: no lucide icon named {name:?} in kit/lucide.rs");
        }
        ""
    });
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="{}" stroke="white" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#,
        if fill { "white" } else { "none" }
    )
}

/// The loader key of an SVG drawn `w` x `h` rem.
pub fn svg_key(ui: &mut Ui, name: &str, svg: String, w: f32, h: f32) -> u64 {
    let (pw, ph) = (ui.r(w).round().max(1.0) as u32, ui.r(h).round().max(1.0) as u32);
    ui.img.request(ImgReq { src: svg_src(name, svg), w: pw, h: ph, fit: Fit::Contain })
}

/// A lucide icon `size` rem square in `color` (stroke 1.75, like the launcher's wrap()).
pub fn icon(ui: &mut Ui, name: &str, size: f32, color: Color) -> El {
    icon_s(ui, name, size, color, 1.75, false)
}

/// A lucide icon with its own stroke width, optionally filled (IconPlay).
pub fn icon_s(ui: &mut Ui, name: &str, size: f32, color: Color, stroke: f32, fill: bool) -> El {
    let k = svg_key(ui, &format!("lucide-{name}"), lucide_svg(name, stroke, fill), size, size);
    El::icon(k, color).with_style(s().size(size).to_style(ui.rem))
}

/// A white SVG drawn `w` x `h` rem, tinted.
pub fn svg_tinted(ui: &mut Ui, name: &str, svg: String, w: f32, h: f32, color: Color) -> El {
    let k = svg_key(ui, name, svg, w, h);
    El::icon(k, color).with_style(s().wh(w, h).to_style(ui.rem))
}

/// An SVG in its own colours, `w` x `h` rem, laid out with `st`.
pub fn svg_box(ui: &mut Ui, name: &str, svg: String, w: f32, h: f32, st: S) -> El {
    let k = svg_key(ui, name, svg, w, h);
    El { content: Content::Image { key: k, tint: None, contain: Some([0.5, 0.5]) }, ..Default::default() }.with_style(st.wh(w, h).to_style(ui.rem))
}

pub const SUI_DROP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 383.5"><path fill="#4DA2FF" fill-rule="evenodd" d="M240.057 159.914C255.698 179.553 265.052 204.39 265.052 231.407C265.052 258.424 255.414 284.019 239.362 303.768L237.971 305.475L237.608 303.31C237.292 301.477 236.929 299.613 236.502 297.749C228.46 262.421 202.265 232.134 159.148 207.597C130.029 191.071 113.361 171.195 108.985 148.586C106.157 133.972 108.258 119.294 112.318 106.717C116.379 94.1569 122.414 83.6187 127.549 77.2831L144.328 56.7754C147.267 53.1731 152.781 53.1731 155.719 56.7754L240.073 159.914H240.057ZM266.584 139.422L154.155 1.96703C152.007 -0.655678 147.993 -0.655678 145.845 1.96703L33.4316 139.422L33.0683 139.881C12.3868 165.555 0 198.181 0 233.698C0 316.408 67.1635 383.461 150 383.461C232.837 383.461 300 316.408 300 233.698C300 198.181 287.613 165.555 266.932 139.896L266.568 139.438L266.584 139.422ZM60.3381 159.472L70.3866 147.164L70.6868 149.439C70.9237 151.24 71.2239 153.041 71.5715 154.858C78.0809 189.001 101.322 217.456 140.173 239.496C173.952 258.724 193.622 280.828 199.278 305.064C201.648 315.176 202.059 325.129 201.032 333.835L200.969 334.372L200.479 334.609C185.233 342.05 168.09 346.237 149.984 346.237C86.4546 346.237 34.9484 294.826 34.9484 231.391C34.9484 204.153 44.4439 179.142 60.3065 159.44L60.3381 159.472Z"/></svg>"##;

pub const STEAM: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="white"><path d="M11.979 0C5.678 0 .511 4.86.022 11.037l6.432 2.658c.545-.371 1.203-.59 1.912-.59.063 0 .125.004.188.006l2.861-4.142V8.91c0-2.495 2.028-4.524 4.524-4.524 2.494 0 4.524 2.031 4.524 4.527s-2.03 4.525-4.524 4.525h-.105l-4.076 2.911c0 .052.004.105.004.159 0 1.875-1.515 3.396-3.39 3.396-1.635 0-3.016-1.173-3.331-2.727L.436 15.27C1.862 20.307 6.486 24 11.979 24c6.627 0 11.999-5.373 11.999-12S18.605 0 11.979 0zM7.54 18.21l-1.473-.61c.262.543.714.999 1.314 1.25 1.297.539 2.793-.076 3.332-1.375.263-.63.264-1.319.005-1.949s-.75-1.121-1.377-1.383c-.624-.26-1.29-.249-1.878-.03l1.523.63c.956.4 1.409 1.5 1.009 2.455-.397.957-1.497 1.41-2.454 1.012H7.54zm11.415-9.303c0-1.662-1.353-3.015-3.015-3.015-1.665 0-3.015 1.353-3.015 3.015 0 1.665 1.35 3.015 3.015 3.015 1.663 0 3.015-1.35 3.015-3.015zm-5.273-.005c0-1.252 1.013-2.266 2.265-2.266 1.249 0 2.266 1.014 2.266 2.266 0 1.251-1.017 2.265-2.266 2.265-1.253 0-2.265-1.014-2.265-2.265z"/></svg>"#;

pub const VERIFIED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path fill="#3D5AFE" d="M8 .8l1.7 1.3 2.1-.2.8 2 2 .8-.2 2.1L15.2 8l-1.3 1.7.2 2.1-2 .8-.8 2-2.1-.2L8 15.2l-1.7-1.3-2.1.2-.8-2-2-.8.2-2.1L.8 8l1.3-1.7-.2-2.1 2-.8.8-2 2.1.2z"/><path d="M5.2 8.2 7.1 10l3.8-4" fill="none" stroke="#fff" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// The Sui droplet `size` rem high (ui/sui.tsx SuiLogo; width is 0.7823 of the height).
pub fn sui_logo(ui: &mut Ui, size: f32) -> El {
    svg_box(ui, "sui-drop", SUI_DROP.to_string(), size * 0.7823, size, s())
}

/// Steam's mark (simple-icons), tinted.
pub fn steam(ui: &mut Ui, size: f32, color: Color) -> El {
    svg_tinted(ui, "steam", STEAM.to_string(), size, size, color)
}

/// lucide `battery*` for a charge (IconBattery picks the variant by percent).
pub fn battery_name(percent: u8, charging: bool) -> &'static str {
    if charging {
        "battery-charging"
    } else if percent >= 80 {
        "battery-full"
    } else if percent >= 45 {
        "battery-medium"
    } else if percent >= 15 {
        "battery-low"
    } else {
        "battery"
    }
}
