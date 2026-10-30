//! Addresses and Move type tags in one canonical form, so ids and types from rare.market, the Trade API
//! and the chain compare equal (`normalizeSuiAddress` / `normalizeStructTag` in @mysten/sui).

use sui_sdk_types::{StructTag, TypeTag};

/// `0x2::sui::SUI` in its long form.
pub const SUI_TYPE: &str = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
/// `0x2::sui::SUI` as the chain's short form (chain.ts `SUI_TYPE`).
pub const SUI_TYPE_SHORT: &str = "0x2::sui::SUI";
pub const KIOSK_OWNER_CAP_TYPE: &str = "0x0000000000000000000000000000000000000000000000000000000000000002::kiosk::KioskOwnerCap";

/// `normalizeSuiAddress`: lower case, `0x`, left-padded to 64 hex digits. Like the TS it never
/// fails: an odd input is padded as is.
pub fn canon_addr(a: &str) -> String {
    let a = a.trim().to_ascii_lowercase();
    let h = a.strip_prefix("0x").unwrap_or(&a);
    format!("0x{h:0>64}")
}

/// the Trade API keys coin types by the short outer address (`0x2::sui::SUI`): `tradeKey` in wallet/data/prices.ts.
pub fn short_key(t: &str) -> String {
    match t.strip_prefix("0x") {
        Some(rest) => {
            let r = rest.trim_start_matches('0');
            format!("0x{r}")
        }
        None => t.to_string(),
    }
}

fn render_tag(t: &TypeTag, out: &mut String) {
    match t {
        TypeTag::Struct(s) => render_struct(s, out),
        TypeTag::Vector(i) => {
            out.push_str("vector<");
            render_tag(i, out);
            out.push('>');
        }
        other => out.push_str(&other.to_string()),
    }
}

fn render_struct(s: &StructTag, out: &mut String) {
    out.push_str(&s.address().to_string());
    out.push_str("::");
    out.push_str(s.module().as_str());
    out.push_str("::");
    out.push_str(s.name().as_str());
    let p = s.type_params();
    if !p.is_empty() {
        out.push('<');
        for (i, t) in p.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            render_tag(t, out);
        }
        out.push('>');
    }
}

/// `normalizeStructTag`: long addresses everywhere, type params joined by `,`. Errors on
/// anything that is not a struct tag.
pub fn normalize_struct_tag(t: &str) -> Option<String> {
    let s: StructTag = t.trim().parse().ok()?;
    let mut out = String::new();
    render_struct(&s, &mut out);
    Some(out)
}

/// Canonical form of a Move type tag so types from different sources compare equal; an
/// unparsable type is only trimmed (`canonType` in chain.ts).
pub fn canon_type(t: &str) -> String {
    normalize_struct_tag(t).unwrap_or_else(|| t.trim().to_string())
}

/// `0xpkg::m::T` with the outer address in its long form (licence.ts's `canonType`), which
/// leaves type arguments as they are.
pub fn canon_outer(t: &str) -> String {
    let mut parts = t.split("::");
    let a = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        return canon_addr(a);
    }
    format!("{}::{}", canon_addr(a), rest.join("::"))
}

pub fn is_sui(t: &str) -> bool {
    canon_type(t) == SUI_TYPE
}

/// A struct tag parsed for the transaction builder.
pub fn struct_tag(t: &str) -> crate::Result<StructTag> {
    t.trim().parse().map_err(|_| crate::Error::invalid(format!("not a Move struct type: {t}")))
}

/// An address parsed for the transaction builder.
pub fn address(a: &str) -> crate::Result<sui_sdk_types::Address> {
    a.trim().parse().map_err(|_| crate::Error::invalid(format!("not a Sui address: {a}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_pad_and_lowercase() {
        assert_eq!(canon_addr("0x2"), "0x0000000000000000000000000000000000000000000000000000000000000002");
        assert_eq!(canon_addr("0xABC"), "0x0000000000000000000000000000000000000000000000000000000000000abc");
        assert_eq!(canon_addr("abc"), canon_addr("0xabc"));
    }

    #[test]
    fn struct_tags_normalize_like_the_ts_sdk() {
        assert_eq!(normalize_struct_tag("0x2::sui::SUI").unwrap(), SUI_TYPE);
        assert_eq!(
            canon_type("0xee::suifrens::SuiFren<0x88::capy::Capy, u64>"),
            format!("{}::suifrens::SuiFren<{}::capy::Capy,u64>", canon_addr("0xee"), canon_addr("0x88"))
        );
        assert_eq!(canon_type("  not a type "), "not a type");
        assert!(is_sui("0x0002::sui::SUI"));
        assert_eq!(short_key(SUI_TYPE), "0x2::sui::SUI");
    }

    #[test]
    fn outer_only() {
        assert_eq!(canon_outer("0xabc::licence::Licence"), format!("{}::licence::Licence", canon_addr("0xabc")));
    }
}
