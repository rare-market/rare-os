//! Rareling launcher themes (launcher/src/perks/themes.ts): accent variants of the shell, each built
//! only from rare.market's own colour tokens. A theme swaps the brand family (buttons, the current
//! tab, the focus ring, the loading bar); surfaces, text, green and red never change.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub id: &'static str,
    pub label: &'static str,
    /// What it is made of, in rare.market's token names.
    pub note: &'static str,
    /// Token -> bare RGB triplet; empty keeps the default tokens.
    pub vars: &'static [(&'static str, &'static str)],
    pub swatch: &'static str,
}

pub const THEMES: [Theme; 4] = [
    Theme { id: "rare", label: "Rare blue", note: "brand", vars: &[], swatch: "61 90 254" },
    Theme {
        id: "royal",
        label: "Royal",
        note: "text-royalties",
        // The light theme's royalties violet as the brand, the dark theme's as the label tint.
        vars: &[("brand", "122 53 255"), ("brand-darker", "122 53 255"), ("labels-brand", "212 189 255"), ("hero-glow", "122 53 255")],
        swatch: "122 53 255",
    },
    Theme { id: "midnight", label: "Midnight", note: "brand-darker", vars: &[("brand", "47 72 217"), ("brand-darker", "47 72 217"), ("labels-brand", "92 118 255"), ("hero-glow", "47 72 217")], swatch: "47 72 217" },
    Theme { id: "periwinkle", label: "Periwinkle", note: "labels-brand", vars: &[("brand", "92 118 255"), ("brand-darker", "61 90 254"), ("labels-brand", "138 155 255"), ("hero-glow", "138 155 255")], swatch: "92 118 255" },
];

pub fn theme_by_id(id: Option<&str>) -> &'static Theme {
    THEMES.iter().find(|t| Some(t.id) == id).unwrap_or(&THEMES[0])
}

/// The token overrides to put on the shell for a theme (None: back to the default tokens), as
/// (token, "r g b"). The screens apply them to the engine's theme.
pub fn theme_overrides(id: Option<&str>) -> &'static [(&'static str, &'static str)] {
    theme_by_id(id).vars
}

/// The device setting a theme is kept under, per account.
pub fn theme_key(account: &str) -> String {
    format!("rare.perks.theme.{}", account.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_rare_market_triplets_and_a_reset() {
        let allowed = ["61 90 254", "47 72 217", "92 118 255", "138 155 255", "122 53 255", "212 189 255"];
        for t in THEMES {
            for (_, v) in t.vars {
                assert!(allowed.contains(v), "{v}");
            }
        }
        assert_eq!(theme_overrides(Some("royal")).iter().find(|(k, _)| *k == "brand").unwrap().1, "122 53 255");
        assert!(theme_overrides(None).is_empty());
        assert_eq!(theme_by_id(Some("nope")).id, "rare");
    }
}
