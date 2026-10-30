//! Import a recovery phrase or a private key (launcher/src/wallet/ui/pages/Import.tsx). The phrase
//! goes in a word at a time on the on-screen keyboard: type the first letters, pick the BIP39
//! word, and the next box opens; "Paste all words" takes the whole phrase. The key field shows
//! dots only. Nothing is kept after leaving (`State` goes with the page).

use super::flow::{h, plain_error, Failure, Gate};
use super::pin_setup::{self, PinOptions};
use super::word_grid::{self, Grid};
use crate::screens::wallet::home::lib::Hint;
use crate::screens::wallet::home::stack::ImportWhat;
use crate::screens::wallet::kit::{btn, btn_full, field, page, segmented, sheet, FieldSpec, SheetSpec, Size, Variant};
use crate::screens::wallet::markup::*;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub pin_step: bool,
    pub count: usize,
    pub words: Vec<String>,
    pub key: String,
    /// the word picker for box `index`
    pub pick: Option<(usize, Vec<String>)>,
    pub flash: String,
    pub busy: bool,
    pub error: String,
    pub pin: pin_setup::State,
}

impl Default for State {
    fn default() -> Self {
        State { pin_step: false, count: 12, words: vec![String::new(); 12], key: String::new(), pick: None, flash: String::new(), busy: false, error: String::new(), pin: pin_setup::State::default() }
    }
}

/// What rare-wallet says about what is typed (`isValidMnemonic`, `keypairFromSecret`).
pub struct Check {
    pub phrase_ok: bool,
    pub key_ok: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Count(usize),
    /// A on box i: keyboard "Word i+1" ("First letters")
    Word(usize),
    /// the keyboard's answer for box i
    Typed(usize, String),
    PickWord(usize, String),
    ClosePick,
    PasteAll,
    Pasted(String),
    /// A on the key field: keyboard "Private key" (secret)
    TypeKey,
    Key(String),
    Import,
    Pin(pin_setup::Msg),
    Back,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    AskWord(usize),
    AskPaste,
    AskKey,
    AskPass { confirm: bool },
    /// open the next empty box's keyboard after 120 ms
    NextWord(usize),
    Gate,
    AddPhrase { words: String, pin: Option<PinOptions> },
    AddKey { key: String, pin: Option<PinOptions> },
}

/// What restoring a PIN account does, in one line.
pub const RESTORE_SUB: &str = "Type the 12 recovery words of your PIN account. Your phone or security key signs in once, then this SuiPlay signs with a new PIN. The address stays the same.";

pub fn view(what: ImportWhat, st: &State, chk: &Check) -> El<Msg> {
    let is_key = what == ImportWhat::Key;
    let filled = st.words.iter().all(|w| !w.is_empty());
    let phrase_error = if filled && !chk.phrase_ok { "That phrase is not valid. Check each word and the order." } else { "" };
    let key_error = if !st.key.trim().is_empty() && !chk.key_ok { "That is not a valid private key. It starts with suiprivkey1." } else { "" };
    let ready = if is_key { chk.key_ok } else { chk.phrase_ok };
    let off = !ready || st.busy;
    let body = if st.pin_step {
        div("pt-4").kid(pin_setup::view(&st.pin, "Set a PIN", "It unlocks your keys on this device. It never leaves it.", st.busy).map(&Msg::Pin))
    } else if is_key {
        let err = if !key_error.is_empty() { Some(key_error) } else { (!st.error.is_empty()).then_some(st.error.as_str()) };
        div("flex flex-col gap-5")
            .kid(field(FieldSpec { label: Some("Private key"), value: &st.key, secret: true, def: true, placeholder: Some("suiprivkey1..."), error: err, ..Default::default() }, Msg::TypeKey))
            .kid(btn(if st.busy { "Adding" } else { "Import" }, Variant::Primary, &format!("w-full {}", if off { "pointer-events-none opacity-40" } else { "" }), Some(Msg::Import)))
    } else {
        let bad: BTreeSet<usize> = if phrase_error.is_empty() { BTreeSet::new() } else { (0..st.words.len()).collect() };
        let first_empty = st.words.iter().position(|w| w.is_empty()).unwrap_or(0);
        let mut d = div("flex flex-col gap-5")
            .kid(if what == ImportWhat::Restore {
                // a PIN account's words are always 12
                div("flex items-center justify-end gap-4").kid(btn_full("Paste all words", Variant::Secondary, Size::Md, false, "", Some(Msg::PasteAll)))
            } else {
                div("flex items-center justify-between gap-4")
                    .kid(segmented(&[("12", "12 words"), ("24", "24 words")], &st.count.to_string(), |v| Msg::Count(if v == "24" { 24 } else { 12 }), ""))
                    .kid(btn_full("Paste all words", Variant::Secondary, Size::Md, false, "", Some(Msg::PasteAll)))
            })
            .kid(word_grid::view(&Grid { words: &st.words, cols: None, pick: true, active: None, bad: &bad, def_index: Some(first_empty) }, Msg::Word));
        let msg = if !st.flash.is_empty() {
            st.flash.as_str()
        } else if !phrase_error.is_empty() {
            phrase_error
        } else {
            st.error.as_str()
        };
        if !msg.is_empty() {
            d = d.kid(p("text-sm text-negative").text(msg));
        }
        let label = if what == ImportWhat::Restore { "Continue" } else if st.busy { "Adding" } else { "Import" };
        d.kid(btn(label, Variant::Primary, &format!("w-full {}", if off { "pointer-events-none opacity-40" } else { "" }), Some(Msg::Import)))
    };
    let mut kids = vec![body];
    if let Some((i, options)) = &st.pick {
        kids.push(sheet(
            SheetSpec { title: format!("Word {}", i + 1), subtitle: Some("Pick the word".into()), icon: None, footer: None, close: Some(Msg::ClosePick), width: "34rem" },
            vec![div("grid grid-cols-2 gap-2").kids(options.iter().enumerate().map(|(n, w)| btn(w, Variant::Secondary, "w-full font-mono", Some(Msg::PickWord(*i, w.clone()))).def(n == 0).key(w)))],
        ));
    }
    let wide = !is_key && st.count == 24;
    let (title, sub) = if st.pin_step {
        (None, None)
    } else if is_key {
        (Some("Import a private key"), Some("Paste a key that starts with suiprivkey1. It is stored encrypted on this device."))
    } else if what == ImportWhat::Restore {
        (Some("Restore your PIN account"), Some(RESTORE_SUB))
    } else {
        (Some("Import a recovery phrase"), Some("Type the first letters of each word and pick it from the list."))
    };
    let mut pg = page(title, sub.map(text), None, is_key || st.count == 12, if wide { "mx-auto max-w-[64rem]" } else { "" }, kids);
    if st.pin_step {
        pg = pg.on_pad("b", Msg::Back);
    }
    pg
}

pub fn hints(st: &State) -> Vec<Hint> {
    if st.pin_step {
        vec![h::select(), h::del(), h::ok(), h::back()]
    } else {
        vec![h::type_(), h::back()]
    }
}

fn fill_all(st: &mut State, list: Vec<String>) {
    if list.len() != 12 && list.len() != 24 {
        st.flash = format!("That is {} words. A recovery phrase has 12 or 24.", list.len());
        return;
    }
    st.count = list.len();
    st.words = list.into_iter().map(|w| w.to_lowercase()).collect();
}

/// `wordlist` is BIP39 English (rare_wallet::keys::wordlist()).
pub fn update(what: ImportWhat, st: &mut State, msg: Msg, wordlist: &[&str]) -> Option<Effect> {
    match msg {
        Msg::Count(n) => {
            st.count = n;
            st.words.resize(n, String::new());
        }
        Msg::Word(i) => {
            st.flash.clear();
            return Some(Effect::AskWord(i));
        }
        Msg::Typed(i, raw) => {
            let t = raw.trim().to_lowercase();
            if t.is_empty() {
                return None;
            }
            let many: Vec<String> = t.split_whitespace().map(str::to_string).collect();
            if many.len() > 1 {
                fill_all(st, many);
                return None;
            }
            let options: Vec<String> = if wordlist.contains(&t.as_str()) { vec![t.clone()] } else { wordlist.iter().filter(|w| w.starts_with(&t)).map(|w| w.to_string()).collect() };
            match options.len() {
                0 => st.flash = format!("\"{t}\" is not a recovery word. Check the spelling."),
                1 => return accept(st, i, options[0].clone()),
                _ => st.pick = Some((i, options.into_iter().take(12).collect())),
            }
        }
        Msg::PickWord(i, w) => return accept(st, i, w),
        Msg::ClosePick => st.pick = None,
        Msg::PasteAll => return Some(Effect::AskPaste),
        Msg::Pasted(v) => {
            if !v.is_empty() {
                fill_all(st, v.split_whitespace().map(str::to_string).collect());
            }
        }
        Msg::TypeKey => return Some(Effect::AskKey),
        Msg::Key(k) => st.key = k,
        Msg::Import => {
            st.error.clear();
            return Some(Effect::Gate);
        }
        Msg::Pin(m) => match pin_setup::update(&mut st.pin, m, st.busy)? {
            pin_setup::Out::Done(pin) => return Some(add(what, st, Some(pin))),
            pin_setup::Out::AskPass { confirm } => return Some(Effect::AskPass { confirm }),
        },
        Msg::Back => st.pin_step = false,
    }
    None
}

fn accept(st: &mut State, i: usize, w: String) -> Option<Effect> {
    // the next empty box is found in the words before this one is set (as on the web)
    let next = st.words.iter().enumerate().position(|(j, x)| j > i && x.is_empty());
    if let Some(slot) = st.words.get_mut(i) {
        *slot = w;
    }
    st.pick = None;
    next.map(Effect::NextWord)
}

fn add(what: ImportWhat, st: &mut State, pin: Option<PinOptions>) -> Effect {
    st.busy = true;
    st.error.clear();
    match what {
        ImportWhat::Key => Effect::AddKey { key: st.key.trim().into(), pin },
        ImportWhat::Phrase | ImportWhat::Restore => Effect::AddPhrase { words: st.words.join(" "), pin },
    }
}

pub fn gate(what: ImportWhat, st: &mut State, g: Gate) -> Option<Effect> {
    match g {
        Gate::Pin => {
            st.pin_step = true;
            None
        }
        Gate::Ready => Some(add(what, st, None)),
        Gate::Cancelled => None,
    }
}

pub fn added(st: &mut State, res: Result<(), Failure>) {
    match res {
        Ok(()) => {
            st.words.iter_mut().for_each(String::clear);
            st.key.clear();
        }
        Err(e) => {
            st.error = plain_error(&e, "Could not add the wallet");
            st.busy = false;
            st.pin_step = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: [&str; 6] = ["abandon", "ability", "able", "about", "above", "absent"];

    #[test]
    fn phrase_entry() {
        let mut st = State::default();
        let chk = Check { phrase_ok: false, key_ok: false };
        let v = view(ImportWhat::Phrase, &st, &chk);
        assert_eq!(v.texts()[..5], ["Import a recovery phrase", "Type the first letters of each word and pick it from the list.", "12 words", "24 words", "Paste all words"]);
        assert_eq!(v.landing().as_deref(), Some("1word"));
        assert_eq!(update(ImportWhat::Phrase, &mut st, Msg::Typed(0, "abou".into()), &LIST), Some(Effect::NextWord(1)));
        assert_eq!(st.words[0], "about");
        assert_eq!(update(ImportWhat::Phrase, &mut st, Msg::Typed(1, "ab".into()), &LIST), None);
        assert!(view(ImportWhat::Phrase, &st, &chk).contains_text("Pick the word"));
        assert_eq!(update(ImportWhat::Phrase, &mut st, Msg::PickWord(1, "able".into()), &LIST), Some(Effect::NextWord(2)));
        update(ImportWhat::Phrase, &mut st, Msg::Typed(2, "xyz".into()), &LIST);
        assert!(view(ImportWhat::Phrase, &st, &chk).contains_text("\"xyz\" is not a recovery word. Check the spelling."));
        update(ImportWhat::Phrase, &mut st, Msg::Pasted("a b c".into()), &LIST);
        assert!(view(ImportWhat::Phrase, &st, &chk).contains_text("That is 3 words. A recovery phrase has 12 or 24."));
        let all = ["Abandon"; 12].join(" ");
        update(ImportWhat::Phrase, &mut st, Msg::Pasted(all), &LIST);
        // as on the web, the last flash stays until a box opens again
        assert!(view(ImportWhat::Phrase, &st, &chk).contains_text("That is 3 words. A recovery phrase has 12 or 24."));
        assert_eq!(update(ImportWhat::Phrase, &mut st, Msg::Word(0), &LIST), Some(Effect::AskWord(0)));
        let v = view(ImportWhat::Phrase, &st, &chk);
        assert!(v.contains_text("That phrase is not valid. Check each word and the order."));
        assert!(st.words.iter().all(|w| w == "abandon"));
        let ok = Check { phrase_ok: true, key_ok: false };
        assert_eq!(update(ImportWhat::Phrase, &mut st, Msg::Import, &LIST), Some(Effect::Gate));
        assert_eq!(gate(ImportWhat::Phrase, &mut st, Gate::Pin), None);
        assert!(view(ImportWhat::Phrase, &st, &ok).contains_text("Set a PIN"));
    }

    #[test]
    fn key_entry() {
        let mut st = State::default();
        update(ImportWhat::Key, &mut st, Msg::Key("suiprivkey1qqsecret".into()), &LIST);
        let v = view(ImportWhat::Key, &st, &Check { phrase_ok: false, key_ok: false });
        assert!(!v.content().contains("secret"));
        assert!(v.contains_text("That is not a valid private key. It starts with suiprivkey1."));
        let v = view(ImportWhat::Key, &st, &Check { phrase_ok: false, key_ok: true });
        assert!(!v.contains_text("That is not a valid private key. It starts with suiprivkey1."));
        assert_eq!(gate(ImportWhat::Key, &mut st, Gate::Ready), Some(Effect::AddKey { key: "suiprivkey1qqsecret".into(), pin: None }));
        added(&mut st, Ok(()));
        assert!(st.key.is_empty());
    }
}
