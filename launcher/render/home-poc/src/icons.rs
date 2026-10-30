//! lucide icons (the web launcher's icon set) and the SUI droplet, as SVG, rasterised by the image
//! workers at the size they are drawn and tinted at paint time.

use rare_render::{Fit, ImgReq, Src};

pub static MARK_PNG: &[u8] = include_bytes!("../../assets/brand/mark.png");

const OPEN: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#;

pub const WIFI: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<path d="M12 20h.01"/><path d="M2 8.82a15 15 0 0 1 20 0"/><path d="M5 12.859a10 10 0 0 1 14 0"/><path d="M8.5 16.429a5 5 0 0 1 7 0"/></svg>"#
);
pub const LAYOUT_GRID: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<rect width="7" height="7" x="3" y="3" rx="1"/><rect width="7" height="7" x="14" y="3" rx="1"/><rect width="7" height="7" x="14" y="14" rx="1"/><rect width="7" height="7" x="3" y="14" rx="1"/></svg>"#
);
pub const PLAY: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="white" stroke="white" stroke-width="2" stroke-linejoin="round"><path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/></svg>"#;
pub const MENU: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<path d="M4 5h16"/><path d="M4 12h16"/><path d="M4 19h16"/></svg>"#
);
pub const ELLIPSIS: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="white" stroke="white" stroke-width="2.5">"#,
    r#"<circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/></svg>"#
);
pub const SUI_DROP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 383.5"><path fill="#4DA2FF" fill-rule="evenodd" d="M240.057 159.914C255.698 179.553 265.052 204.39 265.052 231.407C265.052 258.424 255.414 284.019 239.362 303.768L237.971 305.475L237.608 303.31C237.292 301.477 236.929 299.613 236.502 297.749C228.46 262.421 202.265 232.134 159.148 207.597C130.029 191.071 113.361 171.195 108.985 148.586C106.157 133.972 108.258 119.294 112.318 106.717C116.379 94.1569 122.414 83.6187 127.549 77.2831L144.328 56.7754C147.267 53.1731 152.781 53.1731 155.719 56.7754L240.073 159.914H240.057ZM266.584 139.422L154.155 1.96703C152.007 -0.655678 147.993 -0.655678 145.845 1.96703L33.4316 139.422L33.0683 139.881C12.3868 165.555 0 198.181 0 233.698C0 316.408 67.1635 383.461 150 383.461C232.837 383.461 300 316.408 300 233.698C300 198.181 287.613 165.555 266.932 139.896L266.568 139.438L266.584 139.422ZM60.3381 159.472L70.3866 147.164L70.6868 149.439C70.9237 151.24 71.2239 153.041 71.5715 154.858C78.0809 189.001 101.322 217.456 140.173 239.496C173.952 258.724 193.622 280.828 199.278 305.064C201.648 315.176 202.059 325.129 201.032 333.835L200.969 334.372L200.479 334.609C185.233 342.05 168.09 346.237 149.984 346.237C86.4546 346.237 34.9484 294.826 34.9484 231.391C34.9484 204.153 44.4439 179.142 60.3065 159.44L60.3381 159.472Z"/></svg>"##;

/// lucide `battery` with a fill bar for the charge (the web launcher's IconBattery).
pub fn battery(pct: u8) -> String {
    let w = 12.0 * (pct.min(100) as f32 / 100.0);
    format!(r#"{OPEN}<rect x="2" y="6" width="16" height="12" rx="2"/><path d="M22 14v-4"/><rect x="4" y="8" width="{w:.2}" height="8" rx="1" fill="white" stroke="none"/></svg>"#)
}

/// An SVG drawn at `px` square (contain), leaked so the request can carry a static source.
pub fn svg_req(name: &str, svg: &str, px: f32) -> ImgReq {
    let name: &'static str = Box::leak(format!("{name}.svg").into_boxed_str());
    let bytes: &'static [u8] = Box::leak(svg.as_bytes().to_vec().into_boxed_slice());
    let p = px.round().max(1.0) as u32;
    ImgReq { src: Src::Static(name, bytes), w: p, h: p, fit: Fit::Contain }
}
