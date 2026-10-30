//! Rare Render's engine/src/theme.rs is generated from app.css. This fails while the engine's copy
//! is stale; `RARE_TW_BLESS=1` rewrites it.

use rare_tw_core::theme::{engine_theme_rs, ENGINE_THEME};

#[test]
fn engine_theme_is_generated_from_app_css() {
    let want = engine_theme_rs().expect("app.css tokens");
    let path = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), ENGINE_THEME);
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    if have != want {
        if std::env::var_os("RARE_TW_BLESS").is_some() {
            std::fs::write(&path, &want).unwrap();
            return;
        }
        panic!("{path} is stale: app.css changed. Run\n  RARE_TW_BLESS=1 cargo test --manifest-path launcher/crates/Cargo.toml -p rare-tw-core --test theme\n");
    }
}
