//! What A does on a notice: open the item or collection in Marketplace, the position or coin in
//! Trade, the transaction on SuiVision, and so on. The label is here; the screen to open is
//! returned as an [`ActionTarget`] for the UI to navigate to, since navigation belongs to it.

use crate::types::{Notice, NoticeAction, NoticeKind};

/// An offer on something the person holds: A accepts it (through the inventory's item sheet).
pub fn is_offer_notice(n: Option<&Notice>) -> bool {
    n.is_some_and(|n| n.kind == NoticeKind::OfferReceived && n.item.as_ref().is_some_and(|i| !i.id.is_empty()))
}

pub fn action_label(a: Option<&NoticeAction>, n: Option<&Notice>) -> &'static str {
    if is_offer_notice(n) {
        return "Accept offer";
    }
    match a {
        Some(NoticeAction::Item { .. }) => "Open item",
        Some(NoticeAction::Collection { .. }) => "Open collection",
        Some(NoticeAction::Position { .. }) => "Open position",
        Some(NoticeAction::Coin { .. }) => "Open coin",
        Some(NoticeAction::Tx { .. }) => "View transaction",
        Some(NoticeAction::Trader { .. }) => "Open trader",
        Some(NoticeAction::Friends { .. }) => "Open friends",
        Some(NoticeAction::Party { .. }) => "Open party",
        Some(NoticeAction::OsUpdate) => "Restart now",
        Some(NoticeAction::Downloads) => "Open library",
        Some(NoticeAction::Emulators) => "Open emulators",
        Some(NoticeAction::Trophies) => "Open trophies",
        Some(NoticeAction::Battery) => "Open power",
        Some(NoticeAction::Unknown) | None => "Open",
    }
}

pub fn tx_url(digest: &str) -> String {
    format!("https://suivision.xyz/txblock/{digest}")
}

/// Where A on a notice goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionTarget {
    /// Accept the offer on this item (the inventory's accept flow).
    AcceptOffer { item_id: String },
    MarketItem { id: String },
    MarketCollection { slug: String },
    TradePerp { market_id: String },
    TradeToken { coin_type: String },
    Trader { address: String },
    /// Friends, on one friend when an address is given.
    Friends { address: Option<String> },
    Party { id: String },
    /// Open a page in the browser (the external-open bridge).
    Web { url: String },
    /// The OS restart sheet.
    OsRestart,
    /// The Library tab.
    Library,
    /// Settings > Emulators.
    EmulatorSettings,
    /// Quick settings (power).
    QuickSettings,
    /// The trophy case, at a game when the notice names one.
    Trophies { appid: Option<u32> },
    Nothing,
}

pub fn action_target(a: Option<&NoticeAction>, digest: Option<&str>, n: Option<&Notice>) -> ActionTarget {
    use ActionTarget as T;
    if is_offer_notice(n) {
        let id = n.and_then(|n| n.item.as_ref()).map(|i| i.id.clone()).unwrap_or_default();
        return T::AcceptOffer { item_id: id };
    }
    match a {
        Some(NoticeAction::Item { item_id, .. }) => T::MarketItem { id: item_id.clone() },
        Some(NoticeAction::Collection { slug }) => T::MarketCollection { slug: slug.clone() },
        Some(NoticeAction::Position { market_id, .. }) => T::TradePerp { market_id: market_id.clone() },
        Some(NoticeAction::Coin { coin_type }) => T::TradeToken { coin_type: coin_type.clone() },
        Some(NoticeAction::Tx { digest }) => T::Web { url: tx_url(digest) },
        Some(NoticeAction::Trader { address }) => T::Trader { address: address.clone() },
        Some(NoticeAction::Friends { address }) => T::Friends { address: address.clone().filter(|a| !a.is_empty()) },
        Some(NoticeAction::Party { party_id }) => T::Party { id: party_id.clone() },
        Some(NoticeAction::OsUpdate) => T::OsRestart,
        Some(NoticeAction::Downloads) => T::Library,
        Some(NoticeAction::Emulators) => T::EmulatorSettings,
        Some(NoticeAction::Trophies) => T::Trophies { appid: n.and_then(trophy_appid) },
        Some(NoticeAction::Battery) => T::QuickSettings,
        Some(NoticeAction::Unknown) | None => match digest.filter(|d| !d.is_empty()) {
            Some(d) => T::Web { url: tx_url(d) },
            None => T::Nothing,
        },
    }
}

/// The Steam app a trophy notice is about: its item id as `steam:<appid>` or a bare number.
pub fn trophy_appid(n: &Notice) -> Option<u32> {
    let id = n.item.as_ref()?.id.as_str();
    id.strip_prefix("steam:").unwrap_or(id).parse().ok()
}

/// [`action_target`] for a notice.
pub fn notice_target(n: &Notice) -> ActionTarget {
    action_target(Some(&n.action), n.digest.as_deref(), Some(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::NoticeItem;

    #[test]
    fn offers_accept_and_others_open() {
        let offer = Notice {
            kind: NoticeKind::OfferReceived,
            item: Some(NoticeItem { id: "0xi".into(), ..Default::default() }),
            action: NoticeAction::Item { item_id: "0xi".into(), slug: None },
            ..Default::default()
        };
        assert_eq!(action_label(Some(&offer.action), Some(&offer)), "Accept offer");
        assert_eq!(notice_target(&offer), ActionTarget::AcceptOffer { item_id: "0xi".into() });
        assert_eq!(action_label(Some(&NoticeAction::Battery), None), "Open power");
        assert_eq!(action_target(Some(&NoticeAction::Tx { digest: "D".into() }), None, None), ActionTarget::Web { url: "https://suivision.xyz/txblock/D".into() });
        assert_eq!(action_target(None, Some("D"), None), ActionTarget::Web { url: tx_url("D") });
        assert_eq!(action_target(None, None, None), ActionTarget::Nothing);
        assert_eq!(action_label(None, None), "Open");
        let t = Notice { kind: NoticeKind::Trophy, item: Some(NoticeItem { id: "steam:620".into(), ..Default::default() }), action: NoticeAction::Trophies, ..Default::default() };
        assert_eq!(notice_target(&t), ActionTarget::Trophies { appid: Some(620) });
        assert_eq!(action_target(Some(&NoticeAction::Trophies), None, None), ActionTarget::Trophies { appid: None });
    }
}
