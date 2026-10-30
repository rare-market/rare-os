//! The kinds of notice that have a switch in Settings, in the order shown. Battery has none: it
//! always shows.

use crate::types::NoticeKind;

pub struct KindInfo {
    pub kind: NoticeKind,
    pub title: &'static str,
    pub body: &'static str,
}

/// The Settings rows, in order.
pub fn kinds() -> Vec<KindInfo> {
    use NoticeKind::*;
    let k = |kind, title, body| KindInfo { kind, title, body };
    vec![
        k(Sold, "Sold", "One of your items sold, or someone took your listing."),
        k(Bought, "Bought", "You bought an item, or your offer was accepted."),
        k(OfferReceived, "Offers", "A new offer on an item or collection you hold."),
        k(CoinReceived, "Coins received", "Someone sent you coins, above $1."),
        k(NftReceived, "NFTs received", "Someone sent you an item from a collection on rare.market."),
        k(Airdrop, "Airdrops", "An item minted straight to you."),
        k(PerpLiquidation, "Perps liquidation", "A perps position is near its liquidation price."),
        k(PriceAlert, "Price alerts", "A price alert you set was hit."),
        k(FollowedTrade, "Followed traders", "A trader you follow made a trade."),
        k(FriendRequest, "Friend requests", "Someone wants to be your friend."),
        k(FriendOnline, "Friends online", "A friend came online or started a game."),
        k(PartyInvite, "Party invites", "A friend invited you to their party."),
        k(DirectMessage, "Messages", "A friend sent you a message."),
        k(PartyMessage, "Party messages", "A new message in your party chat."),
        k(OsUpdate, "Rare OS updates", "An update is ready and installs when you restart."),
        k(DownloadDone, "Downloads", "A Steam download finished."),
        k(EmuUpdate, "Emulator updates", "A newer version of an emulator is available."),
        k(Trophy, "Trophies", "A trophy was earned."),
    ]
}
