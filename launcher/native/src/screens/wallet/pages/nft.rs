//! One collectible (launcher/src/wallet/ui/pages/Nft.tsx, after rare.market's item view): art, name,
//! collection, traits and where it is held, with Send and View in Marketplace.

use crate::screens::wallet::fmt::fmt_sui;
use crate::screens::wallet::home::lib::{copy_value, hint, load_bar, short_digest, Hint};
use crate::screens::wallet::home::stack::WalletView;
use crate::screens::wallet::kit::{btn, btn_icon, empty, fmt_pct, mimg, page, panel, row, Size, Variant};
use crate::screens::wallet::market::{rarity_badge, verified};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::types::{NftDetail, OwnerKind};

/// `held`: where the item is, in plain words.
pub fn held(d: &NftDetail) -> &'static str {
    match d.owner {
        OwnerKind::Address if d.in_kiosk => "In your kiosk",
        OwnerKind::Address => "In your wallet",
        OwnerKind::Object if d.in_kiosk => "In a kiosk",
        OwnerKind::Object => "Held inside another object",
        OwnerKind::Shared => "Shared object",
        OwnerKind::Immutable => "Immutable",
        OwnerKind::Unknown => "Unknown",
    }
}

/// `useLoad(nftDetail)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Load {
    pub data: Option<NftDetail>,
    pub loading: bool,
    pub error: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Push(WalletView),
    /// View in Marketplace (opens the Market tab on the item)
    OpenInMarket(String),
    Copy(String),
    /// "Can't send this one" does nothing
    Nothing,
}

/// `copied`: which value shows its check (kiosk or object id).
pub fn view(q: &Load, can_sign: bool, copied: Option<&str>) -> El<Msg> {
    let Some(d) = &q.data else {
        let mut body = vec![load_bar(q.loading)];
        if q.error {
            body.push(empty("Could not load this item", None, None, false));
        }
        return page(None, None, None, false, "", body).attr("data-ready", q.error.to_string());
    };
    let mut meta = div("mt-2 flex flex-wrap items-center gap-x-3 gap-y-2 text-base");
    if let Some(c) = &d.collection {
        let mut s = span("flex items-center gap-1.5").kid(text(format!("{} ", c.name)));
        if c.verified {
            s = s.kid(verified("1rem"));
        }
        meta = meta.kid(s);
    }
    meta = meta.kid_opt(rarity_badge(d.rarity.as_ref(), false, ""));
    if let Some(l) = &d.listing {
        meta = meta.kid(span("text-secondary").text(format!("Listed for {} SUI", fmt_sui(Some(l.all_in_mist.unwrap_or(l.price_mist)), 3, false, false))));
    }
    let mut right = div("min-w-0").kid(h1("text-3xl font-bold leading-tight").text(&d.name)).kid(meta);
    if !d.description.is_empty() {
        right = right.kid(p("mt-3 line-clamp-3 text-sm leading-relaxed text-secondary").text(&d.description));
    }
    let main = if !can_sign {
        btn_icon("user-plus", "Add a signing account", Variant::Primary, Size::Lg, "", Some(Msg::Push(WalletView::Add))).def(true)
    } else if d.can_send {
        btn_icon("arrow-up-right", "Send", Variant::Primary, Size::Lg, "", Some(Msg::Push(WalletView::SendNfts { ids: vec![d.id.clone()], to: None }))).def(true)
    } else {
        btn("Can't send this one", Variant::Dimmed, "", Some(Msg::Nothing)).def(true)
    };
    let mut market = btn_icon("external-link", "View in Marketplace", Variant::Secondary, Size::Lg, "", Some(Msg::OpenInMarket(d.id.clone())));
    market.kids[0] = icon("external-link", "").attr("size", "1.05rem");
    right = right.kid(div("mt-5 flex flex-wrap gap-2").kid(main).kid(market));
    if let Some(why) = d.cant_send.as_ref().filter(|_| can_sign && !d.can_send) {
        right = right.kid(p("mt-2 text-sm text-secondary").text(format!("{why}.")));
    }
    if !d.attributes.is_empty() {
        right = right.kid(h2("mb-2 mt-6 text-lg font-bold").text("Traits")).kid(div("grid grid-cols-3 gap-2").kids(d.attributes.iter().take(9).map(|a| {
            div("min-w-0 rounded-lg border border-border-primary bg-layer-01 px-3 py-2")
                .key(format!("{}:{}", a.type_, a.value))
                .kid(div("truncate text-xs text-secondary").text(&a.type_))
                .kid(div("truncate text-sm font-semibold").text(&a.value))
                .kid_opt(a.pct.map(|p_| div("text-xs tabular-nums text-secondary").text(format!("{} have this", fmt_pct(p_, 1)))))
        })));
    }
    let mut rows = vec![row("Held", vec![text(held(d))], "", true)];
    if let Some(k) = &d.kiosk_id {
        rows.push(row("Kiosk", vec![copy_value(&short_digest(k), "Copy kiosk id", copied == Some(k.as_str()), Msg::Copy(k.clone()))], "", true));
    }
    rows.push(row("Object", vec![copy_value(&short_digest(&d.id), "Copy object id", copied == Some(d.id.as_str()), Msg::Copy(d.id.clone()))], "", true));
    right = right.kid(panel(None, None, "mt-6 flex flex-col gap-2 px-5 py-4", rows));
    page(
        None,
        None,
        None,
        false,
        "",
        vec![div("grid grid-cols-[minmax(0,26rem)_minmax(0,1fr)] gap-10")
            .kid(div("self-start").kid(div("relative aspect-square overflow-hidden rounded-xl bg-layer-01").kid(mimg(d.image_url.as_deref(), d.collection.as_ref().map(|c| c.name.as_str()).unwrap_or(&d.name), 1024, "absolute inset-0 size-full bg-layer-02").attr("data-fit", "inside").attr("data-contain", "true"))))
            .kid(right)],
    )
    .attr("data-ready", d.id.clone())
}

pub fn hints() -> Vec<Hint> {
    vec![hint("a", "Select"), hint("b", "Back")]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::types::{Attribute, CollectionRef, Rarity};

    fn detail() -> NftDetail {
        NftDetail {
            id: "0x7d20dcdb2bca4f508ea9613994683eb4e76e9c4ed371169677c1be02aaf0b58e".into(),
            name: "Popkins #8475".into(),
            image_url: Some("https://img.rare.market/popkins/8475.png".into()),
            description: "A Popkin.".into(),
            collection: Some(CollectionRef { slug: "popkins".into(), name: "Popkins".into(), image_url: None, verified: true }),
            attributes: vec![Attribute { type_: "Hat".into(), value: "Crown".into(), pct: Some(2.4) }],
            rarity: Some(Rarity { rank: 412, tier: "rare".into() }),
            listing: None,
            kiosk_id: Some("0xkiosk00000000000000000000000000000001".into()),
            owner: OwnerKind::Object,
            in_kiosk: true,
            can_send: true,
            cant_send: None,
        }
    }

    #[test]
    fn nft() {
        let v = view(&Load { data: Some(detail()), ..Default::default() }, true, None);
        assert_eq!(
            v.texts(),
            ["Popkins #8475", "Popkins ", "412", "A Popkin.", "Send", "View in Marketplace", "Traits", "Hat", "Crown", "2.4% have this", "Held", "In a kiosk", "Kiosk", "0xkios...0001", "Object", "0x7d20...b58e"]
        );
        assert_eq!(v.landing().as_deref(), Some("Send"));
        let mut d = detail();
        d.can_send = false;
        d.cant_send = Some("It is staked in ACT".into());
        let v = view(&Load { data: Some(d), ..Default::default() }, true, None);
        assert!(v.contains_text("It is staked in ACT."));
        assert_eq!(v.landing().as_deref(), Some("Can't send this one"));
        let v = view(&Load { data: None, loading: false, error: true }, true, None);
        assert!(v.contains_text("Could not load this item"));
    }
}
