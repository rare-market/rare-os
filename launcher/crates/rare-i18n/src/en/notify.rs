strings! {
    /// Relative time, short: n seconds ago ("12s")
    time_s: "{n}s",
    /// Relative time, short: n minutes ago ("3m")
    time_m: "{n}m",
    /// Relative time, short: n hours ago ("2h")
    time_h: "{n}h",
    /// Relative time, short: n days ago ("4d")
    time_d: "{n}d",
    /// Relative time, short: n months ago ("1mo")
    time_mo: "{n}mo",
    /// Relative time, short: n years ago ("1y")
    time_y: "{n}y",
    /// Price alert direction in a sentence: "SUI goes above $1.10"
    above: "above",
    /// Price alert direction in a sentence: "SUI goes below $0.90"
    below: "below",
    /// Notification centre download row status
    paused: "Paused",
    /// Unread count; n is a number ("3 new")
    n_new: "{n} new",
    /// Notification centre button and hint (short)
    mark_all_read: "Mark all read",
    /// Notification centre button (short)
    clear: "Clear",
    /// Hint on the Clear button
    clear_all: "Clear all",
    /// Hint on a download row in the notification centre
    open_library: "Open library",
    /// Notification centre empty state
    none_yet: "No notifications yet.",
    /// Price alert sheet: change from the current price; sign is + or a minus sign, pct a number
    pct_from_now: "{sign}{pct}% from now",
    /// Price alert line: symbol is a coin like SUI, dir is above or below, price a dollar amount
    goes_dir: "{symbol} goes {dir} {price}",
    /// Price alert sheet title; symbol is a coin like SUI
    alert_title: "Price alert for {symbol}",
    /// Price alert sheet subtitle
    alert_sub: "One notice when the price crosses your target.",
    /// Controller hint on the minus button of the target price stepper
    less_price: "Less price",
    /// Controller hint on the plus button of the target price stepper
    more_price: "More price",
    /// Price alert sheet row label
    price_now: "Price now",
    /// Price alert sheet row label
    alert_when: "Alert when",
    /// Price alert sheet error
    alert_failed: "Could not set the alert. Try again in a moment.",
    /// Price alert sheet button and hint: save the alert
    set_alert: "Set alert",
    /// Price alert sheet button and hint: open the keyboard to type a price
    type_price: "Type a price",
    /// Controller hint: lower the target (short)
    lower: "Lower",
    /// Controller hint: raise the target (short)
    higher: "Higher",
    /// Toast after setting an alert; dir is above or below, price a dollar amount
    alert_set_toast: "Price alert set: {symbol} {dir} {price}",
    /// Notification settings: explanation under a group of notices that pop up over a game
    note_over_game: "Pops up over a game. Waits in the list with Do not disturb.",
    /// Notification settings: explanation under the trophies group
    note_trophies: "A small toast over a game. Waits in the list with Do not disturb.",
    /// Notification settings: explanation under the group that pops up in Rare
    note_in_rare: "Pops up in Rare. Waits in the list over a game and with Do not disturb.",
    /// Notification settings: explanation under the battery group
    note_battery: "Always pops up, even over a game and with Do not disturb.",
    /// Notification settings: price alerts row when there are none
    alerts_none: "No price alerts. Set one from a coin's page in Trade.",
    /// Notification settings: price alerts row when loading failed
    alerts_load_failed: "Could not load price alerts.",
    /// Notification settings: price alerts row explanation
    alerts_about: "One notice when the price crosses your target, then the alert stops.",
    /// Notification settings switch (Do not disturb mode)
    dnd: "Do not disturb",
    /// Notification settings switch description
    dnd_body: "Only battery warnings pop up. Everything else waits in the list.",
    /// Notification settings switch
    game_toasts: "Show toasts over a game",
    /// Notification settings switch description
    game_toasts_body: "Money, invites, messages and trophies pop up over a game. Off: they wait in the list until it ends.",
    /// Notification settings switch
    desktop: "Desktop notifications",
    /// Notification settings switch description
    desktop_body: "When Rare is in the background, notices also show on the desktop.",
    /// Notification settings row title
    your_alerts: "Your price alerts",
    /// A price alert row; symbol is a coin like SUI, dir is above or below
    alert_line: "{symbol} {dir} {price}",
    /// Price alert row status: still watching
    watching: "Watching",
    /// Price alert row status: the price crossed the target
    hit: "Hit",
    /// Hint on a price alert row
    delete_alert: "Delete alert",
    // ---- the switch rows per kind of notice (rare-notify keeps the English) and their groups
    /// Notification settings switch: a kind of notice, and its description
    kind_sold: "Sold",
    kind_sold_body: "One of your items sold, or someone took your listing.",
    /// Notification settings switch: a kind of notice, and its description
    kind_bought: "Bought",
    kind_bought_body: "You bought an item, or your offer was accepted.",
    /// Notification settings switch: a kind of notice, and its description
    kind_offer_received: "Offers",
    kind_offer_received_body: "A new offer on an item or collection you hold.",
    /// Notification settings switch: a kind of notice, and its description
    kind_coin_received: "Coins received",
    kind_coin_received_body: "Someone sent you coins, above $1.",
    /// Notification settings switch: a kind of notice, and its description
    kind_nft_received: "NFTs received",
    kind_nft_received_body: "Someone sent you an item from a collection on rare.market.",
    /// Notification settings switch: a kind of notice, and its description
    kind_airdrop: "Airdrops",
    kind_airdrop_body: "An item minted straight to you.",
    /// Notification settings switch: a kind of notice, and its description
    kind_perp_liquidation: "Perps liquidation",
    kind_perp_liquidation_body: "A perps position is near its liquidation price.",
    /// Notification settings switch: a kind of notice, and its description
    kind_price_alert: "Price alerts",
    kind_price_alert_body: "A price alert you set was hit.",
    /// Notification settings switch: a kind of notice, and its description
    kind_followed_trade: "Followed traders",
    kind_followed_trade_body: "A trader you follow made a trade.",
    /// Notification settings switch: a kind of notice, and its description
    kind_friend_request: "Friend requests",
    kind_friend_request_body: "Someone wants to be your friend.",
    /// Notification settings switch: a kind of notice, and its description
    kind_friend_online: "Friends online",
    kind_friend_online_body: "A friend came online or started a game.",
    /// Notification settings switch: a kind of notice, and its description
    kind_party_invite: "Party invites",
    kind_party_invite_body: "A friend invited you to their party.",
    /// Notification settings switch: a kind of notice, and its description
    kind_direct_message: "Messages",
    kind_direct_message_body: "A friend sent you a message.",
    /// Notification settings switch: a kind of notice, and its description
    kind_party_message: "Party messages",
    kind_party_message_body: "A new message in your party chat.",
    /// Notification settings switch: a kind of notice, and its description
    kind_os_update: "Rare OS updates",
    kind_os_update_body: "An update is ready and installs when you restart.",
    /// Notification settings switch: a kind of notice, and its description
    kind_download_done: "Downloads",
    kind_download_done_body: "A Steam download finished.",
    /// Notification settings switch: a kind of notice, and its description
    kind_emu_update: "Emulator updates",
    kind_emu_update_body: "A newer version of an emulator is available.",
    /// Notification settings switch: a kind of notice, and its description
    kind_trophy: "Trophies",
    kind_trophy_body: "A trophy was earned.",
    /// Notification settings: group headings
    class_money: "Money",
    class_invites: "Invites",
    class_messages: "Messages",
    class_trophies: "Trophies",
    class_social: "Social",
    class_system: "System",
    class_battery: "Battery",
    /// Battery warnings while discharging, at 15 % and at 5 %
    battery_15: "Battery at 15%.",
    battery_5: "Battery at 5%. Plug in soon.",
}
