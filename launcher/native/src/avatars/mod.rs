//! Rare's default profile pictures (tools/avatars/gen.mjs draws them; rare.market hosts the same
//! ids at /avatars/{512,128}/{id}.{png,webp}). A default pick is only its id; the launcher draws
//! the SVG itself, so the hosted URL never needs the network here.

mod set;

pub use set::{SET, STARTERS};

/// The hosted picture rare.market hands to other players (friends, parties, presence).
pub const HOST: &str = "https://rare.market/avatars/";

pub struct Pic {
    pub id: &'static str,
    pub name: &'static str,
    pub svg: &'static str,
}

fn def(t: &'static (&'static str, &'static str, &'static str)) -> Pic {
    Pic { id: t.0, name: t.1, svg: t.2 }
}

pub fn all() -> impl Iterator<Item = Pic> {
    SET.iter().map(def)
}

pub fn get(id: &str) -> Option<Pic> {
    SET.iter().find(|t| t.0 == id).map(def)
}

pub fn is_default(id: &str) -> bool {
    get(id).is_some()
}

/// The hosted 512 px PNG of a default.
pub fn url(id: &str) -> String {
    format!("{HOST}512/{id}.png")
}

/// The default a picture URL points at (any hosted size or format).
pub fn id_of_src(src: &str) -> Option<&'static str> {
    let rest = src.strip_prefix(HOST)?;
    let file = rest.rsplit('/').next()?;
    let id = file.split('.').next()?;
    SET.iter().find(|t| t.0 == id).map(|t| t.0)
}

/// FNV-1a over the address (lower case, no 0x, padded to 64), the same as gen.mjs and rare.market,
/// so every surface hands an account the same default until it picks one.
pub fn starter(address: &str) -> &'static str {
    let a = address.trim().to_lowercase();
    let hex = a.strip_prefix("0x").unwrap_or(&a);
    let padded = format!("{hex:0>64}");
    let mut h: u32 = 0x811c_9dc5;
    for b in padded.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    SET[(h as usize) % STARTERS].0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_starters() {
        assert!(SET.len() >= STARTERS && STARTERS == 24);
        assert!(all().all(|d| d.svg.starts_with("<svg") && !d.name.is_empty()));
        assert_eq!(id_of_src("https://rare.market/avatars/128/fox.webp"), Some("fox"));
        assert_eq!(id_of_src(&url("owl")), Some("owl"));
        assert_eq!(id_of_src("https://rare.market/avatars/512/nope.png"), None);
        // The same picks as gen.mjs starterIndex (10 and 18) for these addresses.
        assert_eq!(starter("0x0000000000000000000000000000000000000000000000000000000000000001"), starter("0x1"));
        assert_eq!(starter("0x7d20dcdb2bca4f5000000000000000000000000000000000000000000000b58e"), SET[10].0);
        assert_eq!(starter("0x1"), SET[18].0);
    }
}
