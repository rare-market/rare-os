//! Library art through the shared image pipeline (rare-images): which pictures a screen draws
//! first, as the (source, size) pairs `Images::preload` takes. Both front ends call this after a
//! scan or when a screen opens, so the first row of covers is decoded before it is painted.
//! Art that ships inside the UI bundle (relative paths like `games/act/cover.jpg`) is not a
//! source the pipeline can fetch and is skipped.

use crate::model::{Game, Library};
use rare_images::{Size, Source};

/// The cover, hero and logo of one game, at the sizes Library, Home and the game page draw them.
pub fn game_items(g: &Game) -> Vec<(Source, Size)> {
    let mut out = Vec::new();
    for (list, size) in [(&g.art.cover, Size::Tile), (&g.art.hero, Size::Hero), (&g.art.logo, Size::Card), (&g.art.header, Size::Card)] {
        if let Some(src) = list.iter().find_map(|u| Source::parse(u)) {
            out.push((src, size));
        }
    }
    out
}

/// Covers (and the heroes of the first few) for the first `n` games of the library, which is
/// already sorted most recent first: what Home and the top of Library show.
pub fn preload_items(lib: &Library, n: usize, heroes: usize) -> Vec<(Source, Size)> {
    let mut out = Vec::new();
    for (i, g) in lib.games.iter().take(n).enumerate() {
        if let Some(src) = g.art.cover.iter().find_map(|u| Source::parse(u)) {
            out.push((src, Size::Tile));
        }
        if i < heroes {
            if let Some(src) = g.art.hero.iter().find_map(|u| Source::parse(u)) {
                out.push((src, Size::Hero));
            }
            if let Some(src) = g.art.logo.iter().find_map(|u| Source::parse(u)) {
                out.push((src, Size::Card));
            }
        }
    }
    out
}

/// The `rimg://localhost/<path>` path for an art URL at a size, for the web UI.
pub fn rimg_path(url: &str, size: Size) -> Option<String> {
    Source::parse(url).map(|_| rare_images::uri_path(url, size))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Art, LaunchSpec, Source as Src};

    fn game(id: &str, cover: &str, hero: &str) -> Game {
        Game {
            id: id.into(),
            source: Src::Steam,
            title: id.into(),
            installed: true,
            install_dir: None,
            size_bytes: None,
            last_played: None,
            playtime_minutes: None,
            art: Art { cover: vec![cover.into()], hero: vec![hero.into()], ..Default::default() },
            launch: LaunchSpec::Steam { appid: 1 },
            origin: String::new(),
            foreign: false,
        }
    }

    fn lib(games: Vec<Game>) -> Library {
        Library { games, sources: vec![], foreign_steam_libraries: vec![], owned: vec![], scanned_at: 0, emulation: None, emulation_app: None, keys: Default::default() }
    }

    #[test]
    fn local_files_and_urls_are_sources_bundle_paths_are_not() {
        let g = game("a", "/home/deck/c.jpg", "games/act/hero.jpg");
        let items = game_items(&g);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].1, Size::Tile);
        let g = game("b", "https://cdn.example/c.jpg", "https://cdn.example/h.jpg");
        assert_eq!(game_items(&g).len(), 2);
    }

    #[test]
    fn preload_takes_heroes_for_the_first_few_only() {
        let l = lib(vec![game("a", "/c1.jpg", "/h1.jpg"), game("b", "/c2.jpg", "/h2.jpg"), game("c", "/c3.jpg", "/h3.jpg")]);
        let items = preload_items(&l, 2, 1);
        assert_eq!(items.iter().filter(|(_, s)| *s == Size::Tile).count(), 2);
        assert_eq!(items.iter().filter(|(_, s)| *s == Size::Hero).count(), 1);
    }

    #[test]
    fn rimg_path_round_trips() {
        let p = rimg_path("/home/deck/c.jpg", Size::Tile).unwrap();
        let (src, size) = rare_images::parse_uri_path(&p).unwrap();
        assert_eq!(src, Source::File("/home/deck/c.jpg".into()));
        assert_eq!(size, Size::Tile);
        assert!(rimg_path("games/act/cover.jpg", Size::Tile).is_none());
    }
}
