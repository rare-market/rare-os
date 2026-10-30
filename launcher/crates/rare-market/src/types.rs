//! Move type and id helpers shared by the market code (from `@mysten/sui/utils` as the web
//! launcher uses them: `normalizeStructTag` via sui/chain.ts `canonType`, `normalizeSuiAddress`).

/// `normalizeSuiAddress`: lowercase, `0x` dropped when present, padded to 64 hex digits. No trimming
/// and no validation, exactly as the SDK.
pub fn norm_sui_address(a: &str) -> String {
    let l = a.to_lowercase();
    let h = l.strip_prefix("0x").unwrap_or(&l);
    format!("0x{h:0>64}")
}

/// Splits `a, b<c, d>, e` at top-level commas.
fn split_params(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn norm_type_tag(t: &str) -> Result<String, String> {
    if t.starts_with("0x") {
        return norm_struct_tag(t);
    }
    if let Some(inner) = t.strip_prefix("vector<").and_then(|r| r.strip_suffix('>')) {
        return Ok(format!("vector<{}>", norm_type_tag(inner.trim())?));
    }
    Ok(t.to_string())
}

/// `normalizeStructTag`: every address in the tag at full length, type parameters joined with ",".
/// Errors on anything that is not `address::module::Name<...>`.
pub fn norm_struct_tag(t: &str) -> Result<String, String> {
    let mut parts = t.splitn(3, "::");
    let (Some(addr), Some(module), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!("Invalid struct tag: {t}"));
    };
    let (name, params) = match rest.find('<') {
        Some(i) => {
            let end = rest.rfind('>').ok_or_else(|| format!("Invalid struct tag: {t}"))?;
            if end < i {
                return Err(format!("Invalid struct tag: {t}"));
            }
            let ps = split_params(&rest[i + 1..end]).into_iter().map(|p| norm_type_tag(p.trim())).collect::<Result<Vec<_>, _>>()?;
            (&rest[..i], ps)
        }
        None => (rest, Vec::new()),
    };
    if module.is_empty() || name.is_empty() {
        return Err(format!("Invalid struct tag: {t}"));
    }
    let head = format!("{}::{module}::{name}", norm_sui_address(addr));
    Ok(if params.is_empty() { head } else { format!("{head}<{}>", params.join(",")) })
}

/// sui/chain.ts `canonType`: the normalised struct tag, or the trimmed input when it is not one.
pub fn canon_type(t: &str) -> String {
    norm_struct_tag(t).unwrap_or_else(|_| t.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Outputs of @mysten/sui 2.38's normalizeStructTag for the same inputs.
    #[test]
    fn struct_tags_normalise_like_the_sdk() {
        let two = format!("0x{}2", "0".repeat(63));
        let three = format!("0x{}3", "0".repeat(63));
        assert_eq!(canon_type("0x2::coin::Coin<0x2::sui::SUI>"), format!("{two}::coin::Coin<{two}::sui::SUI>"));
        assert_eq!(canon_type("0x2::a::B<u64,vector<0x3::x::Y>>"), format!("{two}::a::B<u64,vector<{three}::x::Y>>"));
        assert_eq!(canon_type("0x2::a::B<0x2::c::D<u8, bool>>"), format!("{two}::a::B<{two}::c::D<u8,bool>>"));
        assert!(norm_struct_tag("u8").is_err());
        assert_eq!(canon_type(" u8 "), "u8");
    }
}
