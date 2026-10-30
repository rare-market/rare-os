//! What A does on a focused element: the onClick of the web launcher, as data. The app runs the
//! shared ones; `Local` goes back to the screen that built the element.

use crate::state::{Overlay, Tab};

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    None,
    /// Play, or Resume when it runs (store.ts launch).
    Launch(String),
    /// The launch-failure sheet's Try again (LaunchFail.tsx tryAgain): close what is left of the
    /// run, then start it again.
    Retry(String),
    /// Stream a game from the paired PC (host uuid, app id).
    StreamPlay(String, u32),
    /// A PC game's sheet (host uuid, app id): Play, Wake and play, Resume, Quit.
    PcSheet(String, u32),
    /// A Sui game's page in the Sui tab (slug).
    SuiGame(String),
    /// A page in the Market tab: `collection:<slug>`, `item:<id>`, `token:<coin>` (market::live::open_on).
    Market(String),
    /// The PC streaming setup sheet: find a PC, pair it.
    PcSetup,
    /// The game page.
    OpenGame(String),
    Tab(Tab),
    Open(Overlay),
    Close,
    Toast(String),
    ToggleFav(String),
    /// The "Add to collection" sheet for a game.
    Collections(String),
    /// Puts a game in a collection or takes it out (collection, game).
    CollectionToggle(String, String),
    /// Asks for a name and makes a collection (with the game in it, when given).
    NewCollection(Option<String>),
    /// Hides a game from the library and Home, or brings it back.
    ToggleHidden(String),
    /// Tile Options > Same game as: the sheet of games that may be this one.
    SameGame(String),
    /// Same game as: the games whose name has these words (game, what was typed).
    SameGameFind(String, String),
    /// Two games are one (the player's merge): (game, the other game).
    MergeGames(String, String),
    /// Tile Options > Not the same game: splits two copies, or asks which copy when more.
    NotSame(String),
    /// This copy is a different game from the other copies it was grouped with.
    SplitCopy(String),
    /// Asks before quitting a running game.
    QuitGame(String),
    /// Quits a running game (TERM, then KILL after a few seconds: rare-launch's quit).
    QuitNow(String),
    /// Steam's own UI (native.steamBigPicture).
    SteamBigPicture,
    /// A Steam game's own page in Steam (Open in Steam).
    SteamGamePage(u32),
    /// A Steam game's store page in Steam (Store page).
    SteamStore(u32),
    /// Adds a web game to the library (lib/webgames.ts addWebGame): an id (a Sui game's slug, else
    /// one from the title), the title, the address, its cover and hero art.
    AddWebGame { id: Option<String>, title: String, url: String, cover: Option<String>, hero: Option<String> },
    /// The uninstall sheet for a library game or app, or a ROM ("rom:<system>/<path>").
    Uninstall(String),
    /// Add a Steam library folder (native.steamAddLibrary): the foreign library notice.
    SteamAddLibrary(String),
    /// Rescan the library.
    Scan,
    /// A web page in the browser (native.openWeb).
    OpenWeb(String),
    /// The trophy case, at a Steam game (None: all games).
    Trophies(Option<u32>),
    /// A Sui game's web build in its window without adding it (Play now; rare-launch webgame_open).
    PlayWeb { id: String, title: String, url: String },
    /// The screen that built it handles this code.
    Local(u32),
    /// A press on Home's friends strip, resolved against the friends data.
    HomeFriends(crate::screens::friends::home::HomeAct),
    /// An app tile (apps/tiles.tsx): resume it, open it, or its page when nothing can open it yet.
    OpenApp(String),
    /// A Rare menu or settings action code (`accounts:pick-username`, `rare:home`).
    Legacy(String),
    /// Install a Steam game the account owns: the drive sheet (features/downloads.rs).
    Install(String),
    /// Install a Steam game into a library folder (game id, folder path).
    InstallTo(String, String),
    /// A Downloads control on a Steam app (its row's sheet, pause, resume, first, cancel).
    Download(u32, crate::features::downloads::Ctl),
}
