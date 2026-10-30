//! The Rare menu over Steam's own UI (Big Picture, opened from Rare as the `app:steam` app): Steam's
//! pages through steam:// URLs, in the Steam client that is already up. Only these pages; the menu
//! closes and Steam's UI keeps the screen.

/// The steam:// URL of a page the Rare menu offers.
pub fn page_url(page: &str) -> Option<&'static str> {
    Some(match page {
        "library" => "steam://open/games",
        "store" => "steam://open/store",
        "downloads" => "steam://open/downloads",
        "settings" => "steam://open/settings",
        _ => return None,
    })
}

/// Closes the Rare menu and opens a Steam page in the running Steam client.
pub fn open_page(page: &str) -> Result<(), String> {
    let url = page_url(page).ok_or("Unknown Steam page")?;
    super::close()?;
    crate::host::host().steam_url(url)?;
    crate::host::log(&format!("[overlay] Steam page {url}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_known_pages() {
        assert_eq!(super::page_url("library"), Some("steam://open/games"));
        assert_eq!(super::page_url("settings"), Some("steam://open/settings"));
        assert_eq!(super::page_url("steam://run/1"), None);
    }
}
