//! rare.market's item pieces the wallet and inventory screens use (launcher/src/marketplace/parts.tsx
//! `ItemCardView`, `RarityBadge`, `Verified`, `VenueMark`, `Sui`; format.ts `shortName`, `TIER`).
//! The Marketplace screens are workstream 15's; these are the same markup for Phase 3.

use super::fmt::{fmt_sui, DASH};
use super::kit::mimg;
use super::markup::*;
use super::types::{ItemCard, Rarity};

pub fn tier_name(t: &str) -> String {
    match t {
        "common" => "Common",
        "uncommon" => "Uncommon",
        "rare" => "Rare",
        "epic" => "Epic",
        "legendary" => "Legendary",
        "mythic" => "Mythic",
        other => other,
    }
    .to_string()
}

/// `shortName`: the item name without its collection's prefix.
pub fn short_name(card: &ItemCard) -> String {
    match &card.collection {
        Some(c) => {
            let prefix = format!("{} ", c.name);
            card.name.strip_prefix(&prefix).unwrap_or(&card.name).to_string()
        }
        None => card.name.clone(),
    }
}

/// `venueName` and the venue's icon.
pub fn venue(v: &str) -> Option<(&'static str, &'static str)> {
    Some(match v {
        "market" => ("Rare", "/venues/market.png"),
        "tradeport" => ("Tradeport", "/venues/tradeport.svg"),
        "drip" => ("Drip", "/venues/drip.svg"),
        "kiosk" => ("Kiosk", "/venues/kiosk.svg"),
        _ => return None,
    })
}

/// `RarityBadge` (md).
pub fn rarity_badge<M>(r: Option<&Rarity>, small: bool, class: &str) -> Option<El<M>> {
    let r = r?;
    Some(
        span(cn(&["inline-flex items-center rounded-sm bg-labels-neutral/90 font-medium text-label-neutral-text tabular-nums", if small { "h-5 px-1 text-[0.6875rem]" } else { "h-6 px-1.5 text-xs" }, class]))
            .attr("title", format!("Rank {}, {}", r.rank, tier_name(&r.tier)))
            .text(r.rank.to_string()),
    )
}

/// `Verified`: the brand check badge.
pub fn verified<M>(size: &str) -> El<M> {
    icon("verified", "inline-block shrink-0").attr("style", format!("width: {size}; height: {size}")).role("img").label("Verified")
}

/// `VenueMark`.
pub fn venue_mark<M>(v: &str, size: &str) -> Option<El<M>> {
    let (label, src) = venue(v)?;
    Some(
        span("inline-flex shrink-0 items-center justify-center overflow-hidden rounded-[3px]")
            .attr("style", format!("width: {size}; height: {size}"))
            .attr("title", format!("Listed on {label}"))
            .kid(img(Some(src), "", "size-full object-contain")),
    )
}

/// `Sui`: a SUI amount, value in primary, unit muted at the same size.
pub fn sui<M>(mist: Option<i128>, dp: usize, compact: bool, unit: bool, class: &str) -> El<M> {
    let none = mist.is_none();
    let mut s = span(cn(&["whitespace-nowrap tabular-nums", when(none, "text-secondary"), class])).kid(span("").text(fmt_sui(mist, dp, compact, false)));
    if unit && !none {
        s = s.kid(span("ml-1 font-normal text-secondary").text("SUI"));
    }
    s
}

/// `ItemCardView`: the square card with art, name, collection, price or status, last sale.
pub fn item_card<M>(card: &ItemCard, idx: usize, def: bool, show_collection: bool, class: &str, on: M) -> El<M> {
    let label = card.collection.as_ref().map(|c| c.name.as_str()).unwrap_or(&card.name);
    let mut art = span("relative block aspect-square w-full bg-layer-03").kid(mimg(card.image_url.as_deref(), label, 400, "absolute inset-0 size-full"));
    if let Some(b) = rarity_badge(card.rarity.as_ref(), false, "") {
        art = art.kid(span("absolute bottom-1.5 right-1.5").kid(b));
    }
    let mut info = span("flex w-full flex-col gap-y-2 p-3").kid(span("truncate text-sm font-semibold").text(short_name(card)));
    if show_collection {
        info = info.kid(span("-mt-1 truncate text-xs text-secondary").text(card.collection.as_ref().map(|c| c.name.clone()).unwrap_or_default()));
    }
    let price = match &card.listing {
        Some(l) => span("flex h-5 items-center gap-1 text-sm").kid(sui(Some(l.all_in_mist.unwrap_or(l.price_mist)), 3, false, true, "font-semibold")).kid_opt(venue_mark(&l.venue, "0.875rem")),
        None => span("flex h-5 items-center gap-1 text-sm").kid(span("text-secondary").text(if card.staked.is_some() { "Staked" } else { "Not listed" })),
    };
    info = info.kid(price).kid(
        span("flex h-6 items-center gap-1 text-sm text-secondary")
            .kid(icon("history", "").attr("size", "0.875rem").attr("aria-hidden", "true"))
            .kid(span("tabular-nums").text(match card.last_sale_mist {
                // a "0" string is truthy in TS: only a missing sale is a dash
                Some(m) => fmt_sui(Some(m), 3, false, true),
                None => DASH.into(),
            })),
    );
    button(
        cn(&["tile group relative flex flex-col overflow-hidden rounded-lg border border-transparent bg-layer-01 text-left transition-colors focus-visible:border-border-interactive-hover focus-visible:bg-layer-02", class]),
        Some(on),
    )
    .def(def)
    .attr("data-idx", idx.to_string())
    .key(&card.id)
    .kid(art)
    .kid(info)
}

/// rare-market's item card as the screens' (MIST strings to integers).
pub fn card_from(c: &rare_market::api::ItemCard) -> ItemCard {
    use super::types::{CollectionRef, Listing, Staked};
    let mist = |s: &Option<String>| s.as_deref().and_then(|v| v.trim().parse::<i128>().ok());
    ItemCard {
        id: c.id.clone(),
        name: c.name.clone(),
        type_: c.kind.clone(),
        image_url: c.image_url.clone(),
        collection: c.collection.as_ref().map(|r| CollectionRef { slug: r.slug.clone(), name: r.name.clone(), image_url: r.image_url.clone(), verified: r.verified }),
        rarity: c.rarity.as_ref().map(|r| Rarity { rank: r.rank as u32, tier: r.tier.clone() }),
        listing: c.listing.as_ref().map(|l| Listing { venue: l.venue.clone(), price_mist: l.price_mist.trim().parse().unwrap_or(0), all_in_mist: mist(&l.all_in_mist) }),
        last_sale_mist: mist(&c.last_sale_mist),
        kiosk_id: c.kiosk_id.clone(),
        staked: c.staked.as_ref().map(|s| Staked { protocol: s.protocol.clone(), name: s.name.clone(), via: s.via.clone() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::types::{CollectionRef, Listing, Staked};

    fn card() -> ItemCard {
        ItemCard {
            id: "0x77".into(),
            name: "Popkins #8475".into(),
            collection: Some(CollectionRef { slug: "popkins".into(), name: "Popkins".into(), image_url: None, verified: true }),
            rarity: Some(Rarity { rank: 412, tier: "rare".into() }),
            last_sale_mist: Some(12_500_000_000),
            ..Default::default()
        }
    }

    #[test]
    fn cards() {
        let c: El<()> = item_card(&card(), 0, false, true, "", ());
        assert_eq!(c.texts(), ["412", "#8475", "Popkins", "Not listed", "12.5"]);
        let mut l = card();
        l.listing = Some(Listing { venue: "tradeport".into(), price_mist: 69_100_000_000, all_in_mist: Some(70_000_000_000) });
        let c: El<()> = item_card(&l, 0, false, false, "", ());
        assert_eq!(c.texts(), ["412", "#8475", "70.000", "SUI", "12.5"]);
        let mut s = card();
        s.staked = Some(Staked { protocol: Some("ACT".into()), name: None, via: "staked".into() });
        s.last_sale_mist = None;
        let c: El<()> = item_card(&s, 0, false, false, "", ());
        assert!(c.texts().contains(&"Staked".to_string()));
        assert!(c.texts().contains(&"--".to_string()));
    }
}
