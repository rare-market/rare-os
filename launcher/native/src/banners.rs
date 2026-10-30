//! Rare's default profile banners (rare.market tools/banners/gen.mjs draws them; the site serves
//! /banners/{1500,750}/<id>.webp). rare.market answers every account's banner (`banner.url`, its
//! pick else its default); this table gives the default by address before that answer is in, and
//! each default's flat background colour for the space the banner holds while it loads.

const HOST: &str = "https://rare.market/banners/";

/// (id, name, background 0xRRGGBB), in the site's order (the default is by index).
pub const SET: &[(&str, &str, u32)] = &[
    ("dunes", "Dunes", 0x3B2822),
    ("ridge", "Ridge", 0x2A3038),
    ("tide", "Tide", 0x16343A),
    ("orbit", "Orbit", 0x1E2A3E),
    ("bloom", "Bloom", 0x3B1F2A),
    ("pines", "Pines", 0x1C3029),
    ("sunrise", "Sunrise", 0x33243C),
    ("cubes", "Cubes", 0x2A3038),
    ("eclipse", "Eclipse", 0x1A2547),
    ("kites", "Kites", 0x2E3122),
    ("quartz", "Quartz", 0x16343A),
    ("clouds", "Clouds", 0x1E2A3E),
];

/// A default banner at 1500 x 500 (`small`: 750 x 250).
pub fn url(id: &str, small: bool) -> String {
    format!("{HOST}{}/{id}.webp", if small { 750 } else { 1500 })
}

/// The default an address shows until it picks one: FNV-1a over the padded address, as the site.
pub fn starter(address: &str) -> &'static (&'static str, &'static str, u32) {
    let a = address.trim().to_lowercase();
    let hex = a.strip_prefix("0x").unwrap_or(&a);
    let padded = format!("{hex:0>64}");
    let mut h: u32 = 0x811c_9dc5;
    for b in padded.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    &SET[h as usize % SET.len()]
}

/// The flat colour of the default a banner URL points at, else the address's default's.
pub fn bg_of(src: Option<&str>, address: &str) -> u32 {
    src.and_then(|s| s.strip_prefix(HOST))
        .and_then(|r| r.rsplit('/').next())
        .and_then(|f| f.split('.').next())
        .and_then(|id| SET.iter().find(|b| b.0 == id))
        .unwrap_or_else(|| starter(address))
        .2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_default_as_the_site() {
        // api/src/banners.test.ts: 0xabab... gets the same id there (checked against the API)
        let a = format!("0x{}", "ab".repeat(32));
        assert_eq!(starter(&a).0, "pines");
        assert_eq!(url("tide", false), "https://rare.market/banners/1500/tide.webp");
        assert_eq!(bg_of(Some("https://rare.market/banners/1500/tide.webp"), &a), 0x16343A);
    }
}
