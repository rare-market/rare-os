//! The player's profile (launcher/src/sui/ui/ProfilePage.tsx, bits.tsx `ProfileAvatar`): picture and
//! SuiNS name, money (SUI plus collectibles at rare.market's value, today's change), the games
//! played on this device, the Sui games they own on chain, their collectibles, and the picture
//! picker (A on a collectible sets it, X goes back to the default).

use super::catalog::SuiGame;
use super::tab::{Owned, RARE};
use crate::data::model::{source_label, Game};
use crate::kit::format::{NumKind, NumOptions};
use crate::screens::wallet::fmt::{num_locale, time_hm};
use crate::screens::wallet::kit::mimg;
use crate::screens::wallet::market::sui;
use crate::screens::wallet::markup::*;
use crate::screens::wallet::types::ItemCard;
use crate::screens::wallet::ui::{button_el, dot, empty, num, rail, stat_line, ButtonSpec, BVariant, Tone};

/// sui/profile.ts state (what the screens read).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub address: Option<String>,
    /// SuiNS name
    pub name: Option<String>,
    /// the wallet account's label
    pub label: Option<String>,
    /// the Rare username
    pub username: Option<String>,
    /// the NFT picked as the picture
    pub avatar: Option<ItemCard>,
    /// rare.market's avatar for the address
    pub market_avatar: Option<String>,
    /// rare.market's banner for the address (1500 x 500); None until it answers
    pub banner: Option<String>,
    pub sui_mist: Option<i128>,
    pub nft_mist: Option<i128>,
    pub sui_usd: Option<f64>,
}

/// `displayName`: username, SuiNS name, the account label (unless it is an address), the short address.
pub fn display_name(p: &Profile) -> String {
    if let Some(u) = p.username.as_ref().filter(|s| !s.is_empty()) {
        return u.clone();
    }
    if let Some(n) = p.name.as_ref().filter(|s| !s.is_empty()) {
        return n.clone();
    }
    if let Some(l) = p.label.as_ref().filter(|s| !s.is_empty() && !s.starts_with("0x")) {
        return l.clone();
    }
    p.address.as_deref().map(crate::screens::wallet::fmt::addr_short).unwrap_or_default()
}

/// sui/live.ts `totalUsd`: coins (the Trade API's portfolio, else SUI) plus collectibles at the SUI price.
pub fn total_usd(sui_mist: Option<i128>, nft_mist: Option<i128>, sui_usd: Option<f64>, coins_usd: Option<f64>) -> Option<f64> {
    let s = sui_usd.filter(|v| *v != 0.0)?;
    // Collectibles alone are not a total (no $0.00 before the coins are read).
    if sui_mist.is_none() && coins_usd.is_none() {
        return None;
    }
    let coins = coins_usd.unwrap_or(sui_mist.unwrap_or(0) as f64 / 1e9 * s);
    Some(coins + nft_mist.unwrap_or(0) as f64 / 1e9 * s)
}

/// `ProfileAvatar`: the picked picture, else rare.market's, else the account's default.
pub fn profile_avatar<M>(p: &Profile, size: &str, class: &str) -> Option<El<M>> {
    let address = p.address.as_ref()?;
    let src = p.avatar.as_ref().and_then(|a| a.image_url.clone()).or_else(|| p.market_avatar.clone());
    Some(match src {
        None => crate::screens::wallet::kit::account_avatar(address, size),
        Some(s) => span(cn(&["relative inline-block shrink-0 overflow-hidden rounded-full bg-l3", class]))
            .attr("style", format!("width: {size}; height: {size}"))
            .kid(mimg(Some(&s), p.avatar.as_ref().map(|a| a.name.as_str()).unwrap_or("?"), 160, "absolute inset-0 h-full w-full")),
    })
}

fn hours(min: i64) -> String {
    if min < 60 {
        format!("{min} min")
    } else {
        format!("{} h", num_locale((min as f64 / 60.0).round(), 0, 0))
    }
}

/// `sinceLabel`: "24h" after 20 hours, else "Since 3:07 PM".
pub fn since_label(t: i64, now: i64, tz_min: i32) -> String {
    if (now - t) as f64 / 3_600_000.0 >= 20.0 {
        "24h".into()
    } else {
        format!("Since {}", time_hm(t, tz_min))
    }
}

/// This device's value change since a time (sui/live.ts `change`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Change {
    pub usd: f64,
    pub since: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Pick,
    Username,
    Saves,
    /// the trophy case, at a game or the grid
    Trophies(Option<u32>),
    /// a perks panel press
    Perk(crate::screens::perks::Msg),
    Wallet,
    Friends,
    Settings,
    OpenGame(String),
    OpenGameTile(String),
    /// close and open the item in the Marketplace
    ViewItem(String),
    /// the picker: use this NFT (toast "<name> is your picture"), or the default
    UseAvatar(Option<String>),
    ClosePicker,
}

pub struct Props<'a> {
    pub prof: &'a Profile,
    pub games: &'a [SuiGame],
    pub owned: &'a [(String, Owned, Option<String>)],
    pub library: &'a [Game],
    /// rare.market's items for the account (None while loading)
    pub items: Option<&'a [ItemCard]>,
    /// the Trade API's coin total for this address
    pub coins_usd: Option<f64>,
    pub live_sui_usd: Option<f64>,
    pub change: Option<Change>,
    pub now: i64,
    pub tz_min: i32,
    /// PerksSection and ProfileTrophies (perks and trophies modules)
    pub perks: Option<El<Msg>>,
    pub trophies: Option<El<Msg>>,
    pub rareling_badge: Option<El<Msg>>,
}

/// `GameTile` (components/ui.tsx), md: the 2:3 cover with title and store under it.
pub fn game_tile(g: &Game, running: bool, on: Msg) -> El<Msg> {
    let app = g.source == "apps";
    let mut b = button("tile relative block aspect-[2/3] w-full overflow-hidden rounded-card bg-l3 text-left", Some(on)).attr("data-title", g.title.clone()).attr("data-game", g.id.clone()).hint_a("Details");
    if app && g.art.cover.is_empty() {
        let mut inner = span("flex h-[4.5rem] w-[4.5rem] items-center justify-center");
        inner = match g.art.icon.first() {
            Some(i) => inner.kid(img(Some(i), &g.title, "max-h-full max-w-full object-contain").attr("data-art", "logo")),
            None => inner.kid(span("text-[2.4rem] font-semibold text-fg/40").text(g.title.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default())),
        };
        b = b.kid(div("absolute inset-0 flex flex-col items-center justify-center gap-[0.9rem] bg-l2 p-[1rem]").kid(inner));
    } else {
        b = b.kid(img(g.art.cover.first().map(String::as_str), &g.title, "absolute inset-0 h-full w-full object-cover").attr("data-art", "cover").attr("data-sources", g.art.cover.join(" ")));
    }
    if running {
        b = b.kid(dot(Tone::Primary, true, "absolute right-[0.6rem] top-[0.6rem] ring-2 ring-black/40"));
    }
    if g.foreign {
        b = b.kid(dot(Tone::Attention, false, "absolute right-[0.5rem] top-[0.5rem]"));
    }
    div("w-[9rem] shrink-0")
        .key(&g.id)
        .kid(b)
        .kid(div("focus-text mt-[0.85rem] truncate px-[0.1rem] text-meta font-medium text-fg/85").text(&g.title))
        .kid(div("truncate px-[0.1rem] text-label text-muted").text(if running { "Playing" } else { source_label(&g.source) }))
}

fn owned_game_card(g: &SuiGame, own: &Owned, art: Option<&str>) -> El<Msg> {
    let art = art.map(str::to_string).or_else(|| (!g.logo_url.is_empty()).then(|| g.logo_url.clone()));
    button("tile relative aspect-square w-[12rem] shrink-0 overflow-hidden rounded-card border border-hair bg-l1 text-left", Some(Msg::OpenGame(g.slug.clone())))
        .hint_a("Game page")
        .key(&g.slug)
        .kid(mimg(art.as_deref(), &g.name, 320, "absolute inset-0 h-full w-full"))
        .kid(span("absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/95 via-black/50 to-transparent"))
        .kid(
            span("absolute inset-x-[1rem] bottom-[0.9rem]")
                .kid(span("block truncate text-body font-bold text-white").text(&g.name))
                .kid(span("block truncate text-meta text-white/60").text(format!("{} {}", num_locale(own.count as f64, 0, 0), if own.count == 1 { "item" } else { "items" }))),
        )
}

/// `ItemTile`.
pub fn item_tile<M>(item: &ItemCard, hint: &str, chosen: bool, def: bool, class: &str, on: M) -> El<M> {
    let mut b = button(cn(&["tile relative aspect-square shrink-0 overflow-hidden rounded-card border border-hair bg-l3 text-left", if class.is_empty() { "w-[9rem]" } else { class }]), Some(on))
        .hint_a(hint)
        .def(def)
        .key(&item.id)
        .kid(mimg(item.image_url.as_deref(), &item.name, 256, "absolute inset-0 h-full w-full"))
        .kid(span("absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/95 via-black/50 to-transparent"))
        .kid(span("absolute inset-x-[0.6rem] bottom-[0.5rem] block truncate text-label font-semibold text-white").text(&item.name));
    if chosen {
        b = b.kid(span("absolute right-[0.5rem] top-[0.5rem] h-[0.6rem] w-[0.6rem] rounded-full bg-fg").label("Current picture"));
    }
    b
}

pub fn view(pr: &Props) -> El<Msg> {
    let prof = pr.prof;
    let Some(address) = &prof.address else {
        return div("rise fixed inset-0 z-30 flex items-center bg-bg px-[3rem]").attr("data-nav-scope", "").kid(empty(
            "Add your Sui account",
            Some(text("Your SuiNS name, picture, Sui games and collectibles show up here once Rare Wallet has an account.")),
            vec![
                button_el(ButtonSpec::new("Open Rare Wallet").v(BVariant::Primary).def(true).icon("wallet", ""), Some(Msg::Wallet)),
                button_el(ButtonSpec::new("Settings"), Some(Msg::Settings)),
            ],
            "",
        ));
    };
    let mut played: Vec<&Game> = pr.library.iter().filter(|g| g.source != "apps" && (g.playtime_minutes.unwrap_or(0) > 0 || g.last_played.unwrap_or(0) > 0)).collect();
    played.sort_by(|a, b| b.playtime_minutes.unwrap_or(0).cmp(&a.playtime_minutes.unwrap_or(0)).then(b.last_played.unwrap_or(0).cmp(&a.last_played.unwrap_or(0))));
    let minutes: i64 = played.iter().map(|g| g.playtime_minutes.unwrap_or(0)).sum();
    let owned_games: Vec<(&SuiGame, &Owned, Option<&str>)> = pr.games.iter().filter_map(|g| pr.owned.iter().find(|(s, _, _)| *s == g.slug).map(|(_, o, art)| (g, o, art.as_deref()))).collect();
    let usd = total_usd(prof.sui_mist, prof.nft_mist, pr.live_sui_usd.or(prof.sui_usd), pr.coins_usd);

    let mut names = div("min-w-0 flex-1").kid(div("text-label text-secondary").text("Profile")).kid(div("flex items-baseline gap-[1rem]").kid(h1("truncate text-title font-semibold tracking-[-0.02em]").text(display_name(prof))).kid_opt(pr.rareling_badge.clone()));
    if let (Some(_), Some(n)) = (&prof.username, &prof.name) {
        names = names.kid(div("truncate text-body text-muted").text(n));
    }
    names = names.kid(div("num truncate text-meta text-muted").text(address));
    let actions = div("flex gap-[0.75rem]")
        .attr("data-nav-group", "profile-actions")
        .attr("data-nav-remember", "")
        .kid(button_el(ButtonSpec::new("Change picture").def(true).icon("image", "").hint("Pick a picture"), Some(Msg::Pick)))
        .kid(button_el(ButtonSpec::new(if prof.username.is_some() { "Change username" } else { "Pick a username" }), Some(Msg::Username)))
        .kid(button_el(ButtonSpec::new("Cloud saves").icon("cloud", ""), Some(Msg::Saves)))
        .kid(button_el(ButtonSpec::new("Wallet").icon("wallet", ""), Some(Msg::Wallet)))
        .kid(button_el(ButtonSpec::new("Friends"), Some(Msg::Friends)))
        .kid(button_el(ButtonSpec::new("Settings"), Some(Msg::Settings)));
    // The banner (rare.market's, its pick else its default) holds its height while it loads; the
    // picture overlaps its lower edge as on the site.
    let mut banner = div("absolute inset-x-0 top-0 h-[13rem] overflow-hidden bg-l2").attr("data-tag", "banner");
    if let Some(b) = prof.banner.as_deref() {
        banner = banner.kid(mimg(Some(b), "", 1500, "absolute inset-0 h-full w-full").attr("data-height", "500"));
    }
    banner = banner.kid(span("absolute inset-x-0 bottom-0 h-[45%] bg-gradient-to-t from-bg to-transparent"));
    let header = el(Tag::Div, "relative flex items-end gap-[1.5rem] px-[2.5rem] pt-[9.5rem]").attr("data-tag", "header").kid_opt(profile_avatar(prof, "6rem", "border-4 border-bg")).kid(names).kid(actions);
    let top = div("relative w-full shrink-0").kid(banner).kid(header);

    let stats = stat_line(
        vec![
            ("Total value".into(), usd.and_then(|u| num(NumKind::Money, Some(u), NumOptions::default(), "", false, true))),
            (
                pr.change.map(|c| since_label(c.since, pr.now, pr.tz_min)).unwrap_or_else(|| "Today".into()),
                pr.change.and_then(|c| num(NumKind::Pnl, Some(c.usd), NumOptions::default(), "", false, true)),
            ),
            (
                if pr.coins_usd.is_some() { "Coins" } else { "SUI" }.into(),
                match pr.coins_usd {
                    Some(c) => num(NumKind::Money, Some(c), NumOptions::default(), "", false, true),
                    None => prof.sui_mist.map(|m| sui(Some(m), 2, false, true, "")),
                },
            ),
            ("Collectibles".into(), prof.nft_mist.map(|m| sui(Some(m), 2, false, true, ""))),
            ("Games played".into(), (!played.is_empty()).then(|| text(num_locale(played.len() as f64, 0, 0)))),
            ("Time played".into(), (minutes > 0).then(|| text(hours(minutes)))),
        ],
        true,
        true,
        "mx-[2.5rem] mt-[1.75rem] rounded-panel bg-l1 px-[1.25rem] py-[1rem]",
    );

    let mut sections = div("mt-[2rem] flex flex-col gap-[2.25rem]").kid_opt(pr.perks.clone()).kid_opt(pr.trophies.clone());
    // Owned sections show only when there is something in them (no empty shelves).
    if !owned_games.is_empty() {
        let sui_games: Vec<El<Msg>> = owned_games.iter().map(|(g, o, art)| owned_game_card(g, o, *art)).collect();
        sections = sections.kid(rail("profile-sui-games", Some(text("Your Sui games")), Some(text(format!("{} owned on Sui", owned_games.len()))), None, "", sui_games));
    }
    if !played.is_empty() {
        sections = sections.kid(rail("profile-played", Some(text("Games you played")), Some(text("on this device")), None, "", played.iter().take(16).map(|g| game_tile(g, false, Msg::OpenGameTile(g.id.clone()))).collect()));
    }
    let items = pr.items;
    if let Some(i) = items.filter(|i| !i.is_empty()) {
        let note = text(format!("{}{}", i.len(), if i.len() >= 60 { "+" } else { "" }));
        let coll: Vec<El<Msg>> = i.iter().take(24).map(|it| item_tile(it, "View", false, false, "", Msg::ViewItem(it.id.clone()))).collect();
        sections = sections.kid(rail("profile-collectibles", Some(text("Collectibles")), Some(note), None, "", coll));
    }

    let _ = RARE;
    div("rise fixed inset-0 z-30 bg-bg").attr("data-nav-scope", "").kid(div("h-full overflow-y-auto pb-[3rem]").attr("data-scroll", "").kid(top).kid_opt(stats).kid(sections))
}

/// A Rare default picture as a round tile; `chosen` marks the current picture.
pub fn default_tile<M>(id: &str, name: &str, chosen: bool, def: bool, on: M) -> El<M> {
    let mut b = button("tile relative aspect-square w-full shrink-0 overflow-hidden rounded-full bg-l3", Some(on))
        .hint_a("Use this")
        .def(def)
        .key(id)
        .label(name)
        .kid(img(Some(&crate::avatars::url(id)), name, "absolute inset-0 h-full w-full").attr("data-width", "256"));
    if chosen {
        b = b.kid(span("absolute right-[20%] top-[20%] h-[0.6rem] w-[0.6rem] rounded-full bg-fg").label("Current picture"));
    }
    b
}

/// `AvatarPicker`: Rare's default pictures first, then the player's NFTs with art (None while they
/// load). `current` is the picture in use (a default id or an NFT id). A uses one, X goes back to
/// the account's default, B closes. `on` maps a pick (None: the default) to the host's message.
pub fn avatar_picker_with<M: Clone>(items: Option<&[ItemCard]>, current: &str, on: impl Fn(Option<String>) -> M, close: M) -> El<M> {
    let any_def = crate::avatars::is_default(current) || items.is_some_and(|i| i.iter().any(|x| x.id == current));
    let defaults = div("grid grid-cols-10 gap-[1rem]").attr("data-nav-group", "avatar-defaults").kids(crate::avatars::all().enumerate().map(|(k, d)| {
        let def = if any_def { d.id == current } else { k == 0 };
        default_tile(d.id, d.name, d.id == current, def, on(Some(d.id.to_string())))
    }));
    let with_art: Vec<&ItemCard> = items.unwrap_or(&[]).iter().filter(|i| i.image_url.is_some()).collect();
    let nfts = match items {
        None => div("py-[1.5rem] text-body text-muted").text("Loading your NFTs"),
        Some(_) if with_art.is_empty() => div("py-[1.5rem] text-body text-muted").text("Any NFT you hold can be your picture. Buy one in Rare Marketplace and it shows up here."),
        Some(_) => div("grid grid-cols-7 gap-[1rem]").attr("data-nav-group", "avatar-grid").kids(with_art.iter().map(|it| item_tile(it, "Use this", current == it.id, current == it.id, "w-full", on(Some(it.id.clone()))))),
    };
    div("pop fixed inset-x-0 bottom-0 top-[4.25rem] z-40 overflow-y-auto rounded-t-sheet border-t border-hair bg-l1 px-[2.5rem] pb-[2rem] pt-[1.5rem]")
        .attr("data-nav-scope", "")
        .attr("data-scroll", "")
        .on_pad("b", close)
        .on_pad("x", on(None))
        .kid(div("mb-[1.25rem] text-row font-semibold").text("Pick your picture"))
        .kid(div("mb-[0.75rem] text-label text-muted").text("Default"))
        .kid(defaults)
        .kid(div("mb-[0.75rem] mt-[2rem] text-label text-muted").text("Your NFTs"))
        .kid(nfts)
}

/// The profile's picker (`Msg::UseAvatar`, `Msg::ClosePicker`).
pub fn avatar_picker(items: Option<&[ItemCard]>, current: &str) -> El<Msg> {
    avatar_picker_with(items, current, Msg::UseAvatar, Msg::ClosePicker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::sui::catalog::seed;
    use crate::screens::wallet::home::fixtures::{ME, NOW};

    fn prof() -> Profile {
        Profile { address: Some(ME.into()), name: Some("rare.sui".into()), sui_mist: Some(1_204_500_000_000), nft_mist: Some(98_600_000_000), sui_usd: Some(0.906), ..Default::default() }
    }

    #[test]
    fn names_and_money() {
        let mut p = prof();
        assert_eq!(display_name(&p), "rare.sui");
        p.username = Some("playerone".into());
        assert_eq!(display_name(&p), "playerone");
        let q = Profile { address: Some(ME.into()), label: Some("0xabc".into()), ..Default::default() };
        assert_eq!(display_name(&q), "0x7d20...b58e");
        let t = total_usd(Some(1_000_000_000), Some(2_000_000_000), Some(2.0), None).unwrap();
        assert!((t - 6.0).abs() < 1e-9);
        assert_eq!(total_usd(None, None, Some(1.0), None), None);
        assert_eq!(since_label(NOW - 3_600_000, NOW, 0), "Since 2:07 PM");
        assert_eq!(since_label(NOW - 21 * 3_600_000, NOW, 0), "24h");
    }

    #[test]
    fn profile_page() {
        let games = seed();
        let p = prof();
        let owned = vec![("act".to_string(), Owned { count: 3, collections: vec![] }, None)];
        let lib: Vec<Game> = vec![serde_json::from_value(serde_json::json!({ "id": "steam:1", "source": "steam", "title": "Balatro", "playtimeMinutes": 4380 })).unwrap()];
        let pr = Props { prof: &p, games: &games, owned: &owned, library: &lib, items: Some(&[]), coins_usd: None, live_sui_usd: None, change: None, now: NOW, tz_min: 0, perks: None, trophies: None, rareling_badge: None };
        let v = view(&pr);
        let t = v.texts();
        assert_eq!(&t[..3], ["Profile", "rare.sui", ME]);
        assert_eq!(v.landing().as_deref(), Some("Change picture"));
        assert!(t.contains(&"Total value".to_string()));
        assert!(t.contains(&"Games played".to_string()));
        assert!(t.contains(&"73 h".to_string()));
        assert!(t.contains(&"3 items".to_string()));
        // no empty owned shelves
        assert!(!t.contains(&"No collectibles yet".to_string()) && !t.contains(&"Loading collectibles".to_string()));
        let none = Profile::default();
        let v = view(&Props { prof: &none, ..pr });
        assert_eq!(v.texts()[0], "Add your Sui account");
        assert_eq!(v.landing().as_deref(), Some("Open Rare Wallet"));
    }

    #[test]
    fn picker() {
        let items = vec![ItemCard { id: "0x1".into(), name: "Popkin".into(), image_url: Some("https://x/p.png".into()), ..Default::default() }];
        let v = avatar_picker(Some(&items), "fox");
        assert_eq!(v.texts(), ["Pick your picture", "Default", "Your NFTs", "Popkin"]);
        assert_eq!(v.pad("x"), Some(&Msg::UseAvatar(None)));
        // the current default is where the cursor lands, defaults come first
        assert_eq!(v.find_by_attr("data-nav-group", "avatar-defaults").unwrap().kids.len(), crate::avatars::SET.len());
        assert!(v.dump().contains("avatars/512/fox.png"));
        assert!(avatar_picker(Some(&[]), "fox").contains_text("Any NFT you hold can be your picture."));
        assert!(avatar_picker(None, "fox").contains_text("Loading your NFTs"));
    }
}
