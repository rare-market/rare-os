//! Home's Sui shelf (launcher/src/sui/ui/HomeSui.tsx): the profile card (picture, name, value and
//! today's change, Sui games owned), Cloud saves, Share, what is minting on rare.market's
//! launchpad, and the Launchpad tile. Network numbers live in the status bar, not here.

use super::profile::{display_name, profile_avatar, Profile};
use crate::kit::format::{NumKind, NumOptions};
use crate::screens::wallet::kit::mimg;
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::{num, rail};

/// sui/live.ts `Mint`.
#[derive(Clone, Debug, PartialEq)]
pub struct Mint {
    pub key: String,
    pub name: String,
    pub image_url: Option<String>,
    pub sub: String,
    pub href: String,
}

pub struct Props<'a> {
    pub prof: &'a Profile,
    /// None until the launchpad answered
    pub mints: Option<&'a [Mint]>,
    /// total value: this device's (coins plus collectibles), else the Trade API's
    pub value: Option<f64>,
    /// today's change: this device's, else the Trade API's
    pub delta: Option<f64>,
    pub owned_games: usize,
    /// share/store.ts `newCount`
    pub new_captures: usize,
    /// Rarelings art on the Launchpad tile (off in the shipping launcher)
    pub rarelings: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Profile,
    OpenWallet,
    Saves,
    Share,
    OpenWeb(String),
}

pub fn view(pr: &Props) -> El<Msg> {
    let prof = pr.prof;
    let card = if prof.address.is_some() {
        let mut bottom = span("flex items-end justify-between gap-[1rem]").kid(
            span("min-w-0")
                .kid(span("block text-label text-secondary").text("Total value"))
                .kid(match pr.value.and_then(|v| num(NumKind::Money, Some(v), NumOptions::default(), "text-row font-semibold", false, true)) {
                    Some(n) => n,
                    None => span("text-row text-muted").text("--"),
                }),
        );
        if let Some(d) = pr.delta {
            bottom = bottom.kid(span("text-right").kid(span("block text-label text-secondary").text("Today")).kid_opt(num(NumKind::Pnl, Some(d), NumOptions::default(), "text-small font-medium", false, true)));
        }
        let n = pr.owned_games;
        button("tile tile-wide flex h-[12rem] w-[19rem] shrink-0 flex-col justify-between rounded-card border border-hair bg-l1 p-[1.1rem] text-left", Some(Msg::Profile))
            .hint_a("Profile")
            .kid(
                span("flex items-center gap-[0.75rem]").kid_opt(profile_avatar(prof, "2.75rem", "")).kid(
                    span("min-w-0")
                        .kid(span("block truncate text-body font-semibold text-fg").text(display_name(prof)))
                        .kid(span("block text-meta text-muted").text(if n > 0 { format!("{n} Sui {} owned", if n == 1 { "game" } else { "games" }) } else { "Your profile".into() })),
                ),
            )
            .kid(bottom)
    } else {
        button("tile tile-wide flex h-[12rem] w-[19rem] shrink-0 flex-col justify-between rounded-card border border-hair bg-l1 p-[1.1rem] text-left", Some(Msg::OpenWallet)).hint_a("Open Wallet").kid(
            span("flex h-full flex-col justify-between gap-[0.75rem]")
                .kid(span("block text-body font-semibold text-fg").text("Your Sui profile"))
                .kid(span("block text-small text-muted").text("Add a Rare Wallet account to see your name, picture, games you own and cloud saves.")),
        )
    };
    let saves = button("tile flex aspect-square w-[12rem] shrink-0 flex-col justify-end gap-[0.2rem] rounded-card border border-hair bg-l1 p-[1rem] text-left", Some(Msg::Saves))
        .hint_a("Cloud saves")
        .kid(icon("cloud", "mb-auto text-icon-2").attr("size", "1.6rem"))
        .kid(span("block text-body font-bold text-fg").text("Cloud saves"))
        .kid(span("block text-meta text-muted").text("Emulator saves on Walrus"));
    let fresh = pr.new_captures;
    let share = button("tile flex aspect-square w-[12rem] shrink-0 flex-col justify-end gap-[0.2rem] rounded-card border border-hair bg-l1 p-[1rem] text-left", Some(Msg::Share))
        .hint_a("Share")
        .kid(icon("images", "mb-auto text-icon-2").attr("size", "1.6rem"))
        .kid(span("block text-body font-bold text-fg").text("Share"))
        .kid(span("block text-meta text-muted").text(if fresh > 0 { format!("{fresh} new {}", if fresh == 1 { "capture" } else { "captures" }) } else { "Screenshots and clips".into() }));
    let mut tiles = vec![card, saves, share];
    for m in pr.mints.unwrap_or(&[]) {
        tiles.push(
            button("tile relative aspect-square w-[12rem] shrink-0 overflow-hidden rounded-card border border-hair bg-l3 text-left", Some(Msg::OpenWeb(m.href.clone())))
                .hint_a("Mint")
                .key(&m.key)
                .kid(mimg(m.image_url.as_deref(), &m.name, 320, "absolute inset-0 h-full w-full"))
                .kid(span("absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/95 via-black/50 to-transparent"))
                .kid(span("absolute inset-x-[1rem] bottom-[0.9rem]").kid(span("block truncate text-body font-bold text-white").text(&m.name)).kid(span("block truncate text-meta text-white/60").text(&m.sub))),
        );
    }
    let sub = if pr.mints.is_some_and(|m| m.is_empty()) { "No live mints" } else { "New drops" };
    let href = format!("{}/launchpad", super::tab::RARE);
    let pad = if pr.rarelings {
        button("tile relative aspect-square w-[12rem] shrink-0 overflow-hidden rounded-card border border-hair bg-l1 text-left", Some(Msg::OpenWeb(href)))
            .hint_a("Open")
            .kid(img(Some("home/launchpad-rarelings.webp"), "", "absolute inset-0 h-full w-full object-cover"))
            .kid(span("absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/95 via-black/50 to-transparent"))
            .kid(span("absolute inset-x-[1rem] bottom-[0.9rem]").kid(span("block truncate text-body font-bold text-white").text("Launchpad")).kid(span("block truncate text-meta text-white/60").text(sub)))
    } else {
        // No art: the rocket on top, like Cloud saves and Share.
        button("tile flex aspect-square w-[12rem] shrink-0 flex-col justify-end gap-[0.2rem] rounded-card border border-hair bg-l1 p-[1rem] text-left", Some(Msg::OpenWeb(href)))
            .hint_a("Open")
            .kid(icon("rocket", "mb-auto text-icon-2").attr("size", "1.6rem"))
            .kid(span("block text-body font-bold text-fg").text("Launchpad"))
            .kid(span("block text-meta text-muted").text(sub))
    };
    tiles.push(pad);
    let note = pr.mints.filter(|m| !m.is_empty()).map(|m| text(format!("{} minting now", m.len())));
    rail("sui-shelf", Some(text("On Sui")), note, None, "", tiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::home::fixtures::ME;

    #[test]
    fn shelf() {
        let prof = Profile { address: Some(ME.into()), name: Some("rare.sui".into()), ..Default::default() };
        let mints = vec![Mint { key: "m".into(), name: "Popkins".into(), image_url: None, sub: "2 SUI".into(), href: "https://rare.market/launchpad/popkins".into() }];
        let v = view(&Props { prof: &prof, mints: Some(&mints), value: Some(1284.07), delta: Some(12.4), owned_games: 1, new_captures: 0, rarelings: false });
        let t = v.texts();
        assert_eq!(&t[..4], ["On Sui", "1 minting now", "rare.sui", "1 Sui game owned"]);
        assert!(t.contains(&"Emulator saves on Walrus".to_string()));
        assert!(t.contains(&"Screenshots and clips".to_string()));
        assert!(t.contains(&"New drops".to_string()));
        let out = Profile::default();
        let v = view(&Props { prof: &out, mints: Some(&[]), value: None, delta: None, owned_games: 0, new_captures: 2, rarelings: false });
        let t = v.texts();
        assert!(t.contains(&"Your Sui profile".to_string()));
        assert!(t.contains(&"2 new captures".to_string()));
        assert!(t.contains(&"No live mints".to_string()));
        assert!(v.find_by_attr("data-nav-group", "sui-shelf").is_some());
    }
}
