//! Which Move type is a Rareling (launcher/src/perks/config.ts). The rarelings package is not
//! published yet: its id will come from the market repo's deploy manifests. Until a type is set
//! every check answers "unavailable", so nothing mistakes "not deployed" for "not held".
//!
//! Set it, first match wins:
//!   1. the device setting `rare.perks.rarelingType` (a full type, or a package id)
//!   2. `RARE_RARELING_TYPE` at build time (same)
//!   3. [`RARELING_PACKAGE`], once the manifest has it
//!
//! A Move type is always named by the package's ORIGINAL id, never an upgrade's.

use serde_json::Value;

/// The original package id of `rarelings`, from market/deploy/contracts once it is published.
pub const RARELING_PACKAGE: Option<&str> = None;
pub const RARELING_NETWORK: &str = "mainnet";
/// Where "Hold a Rareling to unlock" sends people.
pub const RARELINGS_URL: &str = "https://rare.market/launchpad/rarelings";
/// The device setting that names the type.
pub const LOCAL_KEY: &str = "rare.perks.rarelingType";

fn is_hex_id(s: &str) -> bool {
    s.strip_prefix("0x").is_some_and(|h| (1..=64).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn is_word(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// "0xabc" -> "0xabc::rareling::Rareling"; a full type stays as it is; anything else is None.
pub fn as_rareling_type(v: Option<&str>) -> Option<String> {
    let s = v?.trim();
    if s.is_empty() {
        return None;
    }
    if is_hex_id(s) {
        return Some(format!("{}::rareling::Rareling", s.to_lowercase()));
    }
    let parts: Vec<&str> = s.split("::").collect();
    if parts.len() == 3 && is_hex_id(parts[0]) && is_word(parts[1]) && is_word(parts[2]) {
        return Some(s.to_string());
    }
    None
}

fn is_hex(s: &str) -> bool {
    s.strip_prefix("0x").is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn pick(o: Option<&Value>) -> Option<String> {
    let r = o?.as_object()?;
    ["originalId", "originalPackageId", "packageId", "package", "id"].iter().find_map(|k| r.get(*k).and_then(|v| v.as_str()).filter(|s| is_hex(s)).map(String::from))
}

/// The package id in a deploy manifest: `{ packageId }`, `{ rarelings: { packageId } }`,
/// `{ packages: { rarelings: { packageId | id } } }`, or a list of `{ name: "rarelings", packageId }`.
/// `originalId` wins over `packageId` when present.
pub fn package_from_manifest(m: &Value) -> Option<String> {
    if let Some(list) = m.as_array() {
        return pick(list.iter().find(|x| x.is_object() && x.get("name").map(|n| n.as_str().map(String::from).unwrap_or_else(|| n.to_string())).unwrap_or_default().to_lowercase().contains("rareling")));
    }
    let r = m.as_object()?;
    let pkgs = r.get("packages").filter(|p| p.is_object());
    pick(r.get("rarelings"))
        .or_else(|| pick(pkgs.and_then(|p| p.get("rarelings"))))
        .or_else(|| r.get("packages").filter(|p| p.is_array()).and_then(package_from_manifest))
        .or_else(|| pick(Some(m)))
}

/// The one Rarelings switch (the `rarelings` feature).
pub fn rarelings_on() -> bool {
    cfg!(feature = "rarelings")
}

/// [`rareling_type`] with every input given (the switch, the device setting, the build's type).
pub fn rareling_type_with(on: bool, local: Option<&str>, build: Option<&str>) -> Option<String> {
    if !on {
        return None;
    }
    as_rareling_type(local).or_else(|| as_rareling_type(build)).or_else(|| as_rareling_type(RARELING_PACKAGE))
}

/// The Rareling type in effect, or None (always None while Rarelings are off). `local` is the
/// device setting [`LOCAL_KEY`].
pub fn rareling_type(local: Option<&str>) -> Option<String> {
    rareling_type_with(rarelings_on(), local, option_env!("RARE_RARELING_TYPE"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn takes_a_package_id_or_a_full_type() {
        assert_eq!(as_rareling_type(Some("0xABC")).as_deref(), Some("0xabc::rareling::Rareling"));
        assert_eq!(as_rareling_type(Some("0xabc::rareling::Rareling")).as_deref(), Some("0xabc::rareling::Rareling"));
        assert_eq!(as_rareling_type(Some("")), None);
        assert_eq!(as_rareling_type(Some("rareling")), None);
    }

    #[test]
    fn reads_the_package_from_manifest_shapes_original_first() {
        assert_eq!(package_from_manifest(&json!({"packageId": "0x1"})).as_deref(), Some("0x1"));
        assert_eq!(package_from_manifest(&json!({"rarelings": {"packageId": "0x2"}})).as_deref(), Some("0x2"));
        assert_eq!(package_from_manifest(&json!({"packages": {"rarelings": {"id": "0x3"}, "market": {"id": "0x9"}}})).as_deref(), Some("0x3"));
        assert_eq!(package_from_manifest(&json!({"packages": [{"name": "market", "packageId": "0x9"}, {"name": "rarelings", "packageId": "0x4", "originalId": "0x5"}]})).as_deref(), Some("0x5"));
        assert_eq!(package_from_manifest(&json!(null)), None);
    }

    /// "Rarelings are off for this release": no type whatever the build or the device says,
    /// until the switch is on.
    #[test]
    #[cfg(not(feature = "rarelings"))]
    fn off_for_this_release() {
        assert!(!rarelings_on());
        assert_eq!(rareling_type(Some("0xdef")), None);
        assert_eq!(rareling_type_with(false, Some("0xdef"), Some("0xabc")), None);
        assert_eq!(rareling_type_with(true, Some("0xdef"), Some("0xabc")).as_deref(), Some("0xdef::rareling::Rareling"));
        assert_eq!(rareling_type_with(true, None, Some("0xabc")).as_deref(), Some("0xabc::rareling::Rareling"));
    }
}
