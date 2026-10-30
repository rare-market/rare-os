//! Send collectibles (launcher/src/wallet/ui/pages/SendNfts.tsx): the picked items (art and names
//! from rare.market), the recipient, Review. Items the chain won't let move are listed with the
//! reason and left out. Large picks go as several transactions; each gets its own review.

use crate::screens::wallet::home::lib::{hint, Hint};
use crate::screens::wallet::kit::{btn, field, mimg, page, panel, FieldSpec, Variant};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::sign::recipient::{recipient_line, recipient_picks, Recent, RecipientState};
use crate::screens::wallet::types::{ItemCard, RareAccount};
use std::collections::BTreeMap;

/// tx `nftSendability` for one item.
#[derive(Clone, Debug, PartialEq)]
pub enum Sendability {
    Ok,
    Blocked(String),
}

pub struct Props<'a> {
    pub account: Option<&'a RareAccount>,
    pub accounts: &'a [RareAccount],
    pub can_sign: bool,
    /// the ids the page was opened with
    pub ids: &'a [String],
    /// rare.market's cards for them (None while loading; failed ones are left out)
    pub cards: Option<&'a [ItemCard]>,
    /// None while checking
    pub can: Option<&'a BTreeMap<String, Sendability>>,
    pub rs: &'a RecipientState,
    pub recents: &'a [Recent],
    pub now: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub to: String,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    TypeTo,
    To(String),
    Review,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    AskTo(String),
    Resolve(String),
    /// plan, build and sign each batch (titles: "Send <name>" or "Send N collectibles (i of n)")
    Send { to: String, suins: Option<String>, ids: Vec<String> },
}

fn sendable<'a>(pr: &Props<'a>) -> Vec<&'a ItemCard> {
    let (Some(cards), Some(can)) = (pr.cards, pr.can) else { return Vec::new() };
    cards.iter().filter(|c| can.get(&c.id) == Some(&Sendability::Ok)).collect()
}

fn cta(pr: &Props, st: &State) -> (String, bool) {
    let ready = matches!(pr.rs, RecipientState::Ready { .. });
    let self_ = match (pr.rs, pr.account) {
        (RecipientState::Ready { address, .. }, Some(a)) => *address == a.address.to_lowercase(),
        _ => false,
    };
    let checking = pr.can.is_none();
    let n = sendable(pr).len();
    let cta = if checking {
        "Checking items"
    } else if n == 0 {
        "Nothing here can be sent"
    } else if !ready {
        match pr.rs {
            RecipientState::Empty => "Enter a recipient",
            RecipientState::Loading { .. } => "Looking up name",
            _ => "Check the recipient",
        }
    } else if self_ {
        "Pick another address"
    } else {
        "Review"
    };
    (cta.into(), pr.can_sign && !checking && n > 0 && ready && !self_ && !st.busy)
}

pub fn view(pr: &Props, st: &State) -> Option<El<Msg>> {
    let account = pr.account?;
    let n = pr.cards.map(|c| c.len()).unwrap_or(pr.ids.len());
    let (cta, can_review) = cta(pr, st);
    let self_ = match pr.rs {
        RecipientState::Ready { address, .. } => *address == account.address.to_lowercase(),
        _ => false,
    };
    let mut body = Vec::new();
    if !pr.can_sign {
        body.push(panel(None, None, "mx-auto mb-6 max-w-[64rem] px-5 py-4", vec![p("text-base font-semibold").text("This account is watch only"), p("mt-1 text-sm text-secondary").text("Add an account that can sign to send from this wallet.")]));
    }
    let mut left = div("").kid(div("grid grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-3").kids(pr.cards.unwrap_or(&[]).iter().map(|c| {
        let blocked = pr.can.and_then(|m| m.get(&c.id)).and_then(|s| match s {
            Sendability::Blocked(r) => Some(r.clone()),
            Sendability::Ok => None,
        });
        div("min-w-0")
            .key(&c.id)
            .kid(div("relative aspect-square overflow-hidden rounded-md bg-layer-03").kid(mimg(c.image_url.as_deref(), &c.name, 300, &cn(&["absolute inset-0 size-full", when(blocked.is_some(), "opacity-40")]))))
            .kid(p("mt-1.5 truncate text-sm font-medium").text(&c.name))
            .kid(p(cn(&["truncate text-xs", if blocked.is_some() { "text-negative" } else { "text-secondary" }])).text(blocked.unwrap_or_else(|| c.collection.as_ref().map(|x| x.name.clone()).unwrap_or_default())))
    })));
    if pr.cards.is_some_and(|c| c.is_empty()) {
        left = left.kid(p("py-10 text-center text-sm text-secondary").text("These items could not be loaded."));
    }
    let mut right = div("flex min-w-0 flex-col gap-4").kid(
        div("")
            .kid(field(FieldSpec { label: Some("To"), value: &st.to, placeholder: Some("0x address or name.sui"), def: true, ..Default::default() }, Msg::TypeTo))
            .kid_opt(recipient_line(pr.rs, self_)),
    );
    if !matches!(pr.rs, RecipientState::Ready { .. }) {
        right = right.kid_opt(recipient_picks(pr.accounts, Some(&account.address), pr.recents, &st.to, pr.now, |a| Msg::To(a.to_string())));
    }
    let mut foot = div("mt-auto");
    if let Some(e) = &st.error {
        foot = foot.kid(p("mb-2 text-sm text-negative").role("alert").text(e));
    }
    foot = foot.kid(btn(if st.busy { "Opening review" } else { &cta }, if can_review { Variant::Primary } else { Variant::Dimmed }, "w-full", can_review.then_some(Msg::Review)));
    right = right.kid(foot);
    body.push(div("mx-auto grid max-w-[64rem] grid-cols-[minmax(0,1fr)_minmax(0,22rem)] gap-10").kid(left).kid(right));
    Some(
        page(Some("Send collectibles"), Some(text(format!("{n} {} from {}", if n == 1 { "item" } else { "items" }, account.label))), None, false, "", body)
            .attr("data-ready", format!("{}{}", account.address, if pr.cards.is_some() { 1 } else { 0 })),
    )
}

pub fn hints() -> Vec<Hint> {
    vec![hint("a", "Select"), hint("b", "Back")]
}

pub fn update(pr: &Props, st: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::TypeTo => Some(Effect::AskTo(st.to.clone())),
        Msg::To(v) => {
            st.to = v.split_whitespace().collect();
            Some(Effect::Resolve(st.to.clone()))
        }
        Msg::Review => {
            let (_, ok) = cta(pr, st);
            let RecipientState::Ready { address, suins } = pr.rs else { return None };
            if !ok {
                return None;
            }
            st.busy = true;
            st.error = None;
            Some(Effect::Send { to: address.clone(), suins: suins.clone(), ids: sendable(pr).iter().map(|c| c.id.clone()).collect() })
        }
    }
}

/// `planNftSend` batch titles: one item by name, else a count and the batch number.
pub fn batch_title(names: &[String], batch: usize, batches: usize) -> String {
    if names.len() == 1 {
        return format!("Send {}", names[0]);
    }
    format!("Send {} collectibles{}", names.len(), if batches > 1 { format!(" ({} of {batches})", batch + 1) } else { String::new() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::home::fixtures::{account, NOW};
    use crate::screens::wallet::sign::fixtures::OTHER;
    use crate::screens::wallet::types::CollectionRef;

    fn cards() -> Vec<ItemCard> {
        let coll = Some(CollectionRef { slug: "popkins".into(), name: "Popkins".into(), image_url: None, verified: true });
        vec![
            ItemCard { id: "0xa".into(), name: "Popkins #18736".into(), collection: coll.clone(), ..Default::default() },
            ItemCard { id: "0xb".into(), name: "Soul Badge".into(), collection: None, ..Default::default() },
        ]
    }

    #[test]
    fn send_nfts() {
        let acct = account();
        let ids = vec!["0xa".to_string(), "0xb".to_string()];
        let cs = cards();
        let rs = RecipientState::Ready { address: OTHER.into(), suins: None };
        let mut can = BTreeMap::new();
        can.insert("0xa".to_string(), Sendability::Ok);
        can.insert("0xb".to_string(), Sendability::Blocked("Soulbound: it can't leave this wallet".into()));
        let pr = Props { account: Some(&acct), accounts: std::slice::from_ref(&acct), can_sign: true, ids: &ids, cards: Some(&cs), can: Some(&can), rs: &rs, recents: &[], now: NOW };
        let mut st = State::default();
        let v = view(&pr, &st).unwrap();
        let t = v.texts();
        assert_eq!(&t[..6], ["Send collectibles", "2 items from Main", "Popkins #18736", "Popkins", "Soul Badge", "Soulbound: it can't leave this wallet"]);
        assert_eq!(t.last().unwrap(), "Review");
        assert_eq!(v.landing().as_deref(), Some("0x address or name.sui"));
        assert_eq!(update(&pr, &mut st, Msg::Review), Some(Effect::Send { to: OTHER.into(), suins: None, ids: vec!["0xa".into()] }));
        let checking = Props { can: None, ..pr };
        assert!(view(&checking, &State::default()).unwrap().contains_text("Checking items"));
        assert_eq!(batch_title(&["Popkins #1".into()], 0, 1), "Send Popkins #1");
        assert_eq!(batch_title(&["a".into(), "b".into()], 1, 3), "Send 2 collectibles (2 of 3)");
    }
}
