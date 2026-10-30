//! "Other ways to sign in" (launcher/src/wallet/ui/onboard/OtherWays.tsx): the same list on
//! Welcome, the Add account page and the launcher's sign-in overlay.

use crate::screens::wallet::home::stack::{ImportWhat, WalletView};
use crate::screens::wallet::kit::{list_button, panel};
use crate::screens::wallet::markup::*;

pub struct Way {
    pub view: WalletView,
    pub title: &'static str,
    pub body: &'static str,
    pub icon: &'static str,
}

pub fn other_ways_list() -> Vec<Way> {
    vec![
        Way { view: WalletView::Import { what: ImportWhat::Phrase }, title: "Recovery phrase", body: "Use the 12 or 24 words from another wallet", icon: "file-text" },
        Way { view: WalletView::Import { what: ImportWhat::Key }, title: "Private key", body: "Paste a suiprivkey... key", icon: "key-round" },
        Way { view: WalletView::Ledger, title: "Ledger", body: "Nano S Plus, Nano X, Stax and Flex over USB or Bluetooth", icon: "usb" },
        Way { view: WalletView::Keystone, title: "Keystone", body: "Keystone 3 Pro over a USB-C cable", icon: "shield-check" },
        Way { view: WalletView::Watch, title: "Watch an address", body: "Follow any wallet. You cannot send from it", icon: "eye" },
    ]
}

/// The panel of ways; A sends `on(view)`.
pub fn other_ways<M>(on: impl Fn(WalletView) -> M, first_default: bool) -> El<M> {
    panel(
        None,
        None,
        "divide-y divide-border-primary p-1",
        other_ways_list()
            .into_iter()
            .enumerate()
            .map(|(i, w)| list_button(Some(icon(w.icon, "").attr("size", "1.4rem")), w.title, Some(text(w.body)), None, on(w.view), first_default && i == 0, "").key(w.title))
            .collect(),
    )
}
