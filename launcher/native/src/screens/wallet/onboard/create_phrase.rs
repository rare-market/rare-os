//! A new recovery phrase (launcher/src/wallet/ui/pages/CreatePhrase.tsx): choose 12 or 24 words,
//! write them down, prove it by picking three words (no typing, a controller is enough), then
//! protect it with a PIN if there is no vault yet. The words live only in this page's `State`
//! and go with it.

use super::flow::{h, plain_error, Failure, Gate};
use super::pin_setup::{self, PinOptions};
use super::word_grid::{self, Grid};
use crate::screens::wallet::home::lib::Hint;
use crate::screens::wallet::kit::{btn, page, panel, segmented, Variant};
use crate::screens::wallet::markup::*;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Length,
    Show,
    Verify,
    Pin,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Question {
    pub index: usize,
    pub choices: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub step: Step,
    pub count: usize,
    pub words: Vec<String>,
    pub quiz: Vec<Question>,
    pub q: usize,
    pub wrong: bool,
    pub busy: bool,
    pub error: String,
    pub pin: pin_setup::State,
}

impl Default for State {
    fn default() -> Self {
        State { step: Step::Length, count: 12, words: Vec::new(), quiz: Vec::new(), q: 0, wrong: false, busy: false, error: String::new(), pin: pin_setup::State::default() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Count(usize),
    ShowWords,
    WroteIt,
    Answer(String),
    Pin(pin_setup::Msg),
    Back,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// `newMnemonic(128 | 256)`: answer with `words`
    NewPhrase { bits: u32 },
    /// make the three questions (needs randomness and the word list): answer with `quiz`
    Quiz,
    /// `vaultGate("Enter your PIN to add this phrase")`: answer with `gate`
    Gate,
    /// `keystore.addPhrase(words, pin)`, then finishAdd
    Add { words: String, pin: Option<PinOptions> },
    AskPass { confirm: bool },
}

/// `shuffle` with the given randomness (`rnd(n)` in 0..n).
pub fn shuffle<T: Clone>(xs: &[T], rnd: &mut impl FnMut(usize) -> usize) -> Vec<T> {
    let mut a = xs.to_vec();
    for i in (1..a.len()).rev() {
        let j = rnd(i + 1);
        a.swap(i, j);
    }
    a
}

/// `makeQuiz`: three distinct positions, each with the right word among five others.
pub fn make_quiz(words: &[String], wordlist: &[&str], rnd: &mut impl FnMut(usize) -> usize) -> Vec<Question> {
    let idx: Vec<usize> = (0..words.len()).collect();
    let mut pos: Vec<usize> = shuffle(&idx, rnd).into_iter().take(3).collect();
    pos.sort_unstable();
    pos.into_iter()
        .map(|i| {
            let pool: Vec<String> = wordlist.iter().filter(|w| !words.iter().any(|x| x == *w)).map(|w| w.to_string()).collect();
            let others: Vec<String> = shuffle(&pool, rnd).into_iter().take(5).collect();
            let mut choices = vec![words[i].clone()];
            choices.extend(others);
            Question { index: i, choices: shuffle(&choices, rnd) }
        })
        .collect()
}

pub fn view(st: &State) -> El<Msg> {
    let wide = st.step == Step::Show && st.words.len() > 12;
    let title = match st.step {
        // From setup, whose page was already "Create your account".
        Step::Length if crate::features::firstrun::wallet_from_setup() => Some("Your recovery words"),
        Step::Length => Some("Create your account"),
        Step::Show => Some("Back up your recovery phrase"),
        Step::Verify => Some("Check your backup"),
        Step::Pin => None,
    };
    let body = match st.step {
        Step::Length => div("flex flex-col gap-6")
            .kid(p("text-secondary").text("It is kept on this SuiPlay and signs with A, no PIN. You get a list of secret words to write down once. They are the only way to get your account back if you lose this device."))
            .kid(div("").kid(div("mb-1.5 text-sm text-secondary").text("Length")).kid(segmented(&[("12", "12 words"), ("24", "24 words")], &st.count.to_string(), |v| Msg::Count(if v == "24" { 24 } else { 12 }), "")))
            .kid(btn("Show my words", Variant::Primary, "w-full", Some(Msg::ShowWords)).def(true)),
        Step::Show => div("flex flex-col gap-5")
            .kid(panel(
                None,
                None,
                "flex items-start gap-3 px-5 py-4",
                vec![
                    icon("triangle-alert", "mt-0.5 shrink-0 text-secondary").attr("size", "1.25rem"),
                    p("min-w-0 flex-1 text-sm").text("Write these words down in order and keep them private. They never leave this SuiPlay. Anyone with them can take your money, and Rare cannot recover them for you."),
                ],
            ))
            .kid(word_grid::view(&Grid { words: &st.words, cols: None, pick: false, active: None, bad: &BTreeSet::new(), def_index: None }, |_| Msg::WroteIt))
            .kid(btn("I wrote it down", Variant::Primary, "w-full", Some(Msg::WroteIt)).def(true)),
        Step::Verify => {
            let Some(qn) = st.quiz.get(st.q) else { return page(title, None, None, true, "", vec![]) };
            let mut d = div("flex flex-col gap-5")
                .kid(
                    p("text-secondary")
                        .kid(text("Pick word "))
                        .kid(span("font-semibold text-primary").text(format!("#{}", qn.index + 1)))
                        .kid(text(format!(" of your phrase. Question {} of {}.", st.q + 1, st.quiz.len()))),
                )
                .kid(div("grid grid-cols-3 gap-2").kids(qn.choices.iter().enumerate().map(|(i, w)| btn(w, Variant::Secondary, "w-full font-mono", Some(Msg::Answer(w.clone()))).def(i == 0).key(w))));
            if st.wrong {
                d = d.kid(p("text-sm text-negative").text(format!("That is not word #{}. Look at your backup and try again.", qn.index + 1)));
            }
            if !st.error.is_empty() {
                d = d.kid(p("text-sm text-negative").text(&st.error));
            }
            d
        }
        Step::Pin => {
            let mut d = div("pt-4").kid(pin_setup::view(&st.pin, "Set a PIN", "It unlocks your recovery phrase on this device. It never leaves it.", st.busy).map(&Msg::Pin));
            if !st.error.is_empty() {
                d = d.kid(p("mt-4 text-center text-sm text-negative").text(&st.error));
            }
            d
        }
    };
    let mut pg = page(title, None, None, !wide, if wide { "mx-auto max-w-[64rem]" } else { "" }, vec![body]);
    if st.step != Step::Length {
        pg = pg.on_pad("b", Msg::Back);
    }
    pg
}

pub fn hints(st: &State) -> Vec<Hint> {
    if st.step == Step::Pin {
        vec![h::select(), h::del(), h::ok(), h::back()]
    } else {
        vec![h::select(), h::back()]
    }
}

pub fn update(st: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Count(n) => st.count = n,
        Msg::ShowWords => return Some(Effect::NewPhrase { bits: if st.count == 24 { 256 } else { 128 } }),
        Msg::WroteIt => return Some(Effect::Quiz),
        Msg::Answer(w) => {
            let qn = st.quiz.get(st.q)?;
            if st.words.get(qn.index) != Some(&w) {
                st.wrong = true;
                return None;
            }
            st.wrong = false;
            if st.q + 1 < st.quiz.len() {
                st.q += 1;
                return None;
            }
            return Some(Effect::Gate);
        }
        Msg::Pin(m) => match pin_setup::update(&mut st.pin, m, st.busy)? {
            pin_setup::Out::Done(pin) => return add(st, Some(pin)),
            pin_setup::Out::AskPass { confirm } => return Some(Effect::AskPass { confirm }),
        },
        Msg::Back => {
            if st.step == Step::Verify && st.q > 0 {
                st.q -= 1;
            } else {
                st.step = match st.step {
                    Step::Pin => Step::Verify,
                    Step::Verify => Step::Show,
                    _ => Step::Length,
                };
            }
            st.wrong = false;
        }
    }
    None
}

/// The new phrase arrived.
pub fn words(st: &mut State, phrase: &str) {
    st.words = phrase.split_whitespace().map(str::to_string).collect();
    st.step = Step::Show;
}

/// The questions arrived (`make_quiz`).
pub fn quiz(st: &mut State, q: Vec<Question>) {
    st.quiz = q;
    st.q = 0;
    st.wrong = false;
    st.step = Step::Verify;
}

pub fn gate(st: &mut State, g: Gate) -> Option<Effect> {
    match g {
        Gate::Pin => {
            st.step = Step::Pin;
            None
        }
        Gate::Ready => add(st, None),
        Gate::Cancelled => None,
    }
}

fn add(st: &mut State, pin: Option<PinOptions>) -> Option<Effect> {
    st.busy = true;
    st.error.clear();
    Some(Effect::Add { words: st.words.join(" "), pin })
}

/// After `Effect::Add`: success clears the words (the host finishes), failure says why.
pub fn added(st: &mut State, res: Result<(), Failure>) {
    match res {
        Ok(()) => st.words.clear(),
        Err(e) => {
            st.error = plain_error(&e, "Could not save the phrase");
            st.busy = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHRASE: &str = "abandon ability able about above absent absorb abstract absurd abuse access accident";

    #[test]
    fn create_flow() {
        let mut st = State::default();
        let v = view(&st);
        assert_eq!(v.texts(), ["Create your account", "It is kept on this SuiPlay and signs with A, no PIN. You get a list of secret words to write down once. They are the only way to get your account back if you lose this device.", "Length", "12 words", "24 words", "Show my words"]);
        assert_eq!(update(&mut st, Msg::Count(24)), None);
        assert_eq!(update(&mut st, Msg::ShowWords), Some(Effect::NewPhrase { bits: 256 }));
        words(&mut st, PHRASE);
        let v = view(&st);
        assert!(v.contains_text("Write these words down in order and keep them private. They never leave this SuiPlay. Anyone with them can take your money, and Rare cannot recover them for you."));
        assert_eq!(v.texts()[2..4], ["1", "abandon"]);
        assert_eq!(v.landing().as_deref(), Some("I wrote it down"));
        let list = ["zoo", "zone", "zero", "youth", "young", "yellow", "abandon"];
        let mut seed = 7usize;
        let mut rnd = |n: usize| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345) % 2_147_483_648;
            seed % n
        };
        let q = make_quiz(&st.words, &list, &mut rnd);
        assert_eq!(q.len(), 3);
        assert!(q.windows(2).all(|w| w[0].index < w[1].index));
        for qn in &q {
            assert_eq!(qn.choices.len(), 6);
            assert!(qn.choices.contains(&st.words[qn.index]));
            assert!(!qn.choices.contains(&"abandon".to_string()) || st.words[qn.index] == "abandon");
        }
        quiz(&mut st, q.clone());
        let v = view(&st);
        assert_eq!(v.texts()[1..4], ["Pick word ".to_string(), format!("#{}", q[0].index + 1), " of your phrase. Question 1 of 3.".to_string()]);
        update(&mut st, Msg::Answer("nope".into()));
        assert!(view(&st).contains_text(&format!("That is not word #{}. Look at your backup and try again.", q[0].index + 1)));
        for qn in &q {
            let w = st.words[qn.index].clone();
            let e = update(&mut st, Msg::Answer(w));
            if qn.index == q[2].index {
                assert_eq!(e, Some(Effect::Gate));
            }
        }
        assert_eq!(gate(&mut st, Gate::Ready), Some(Effect::Add { words: PHRASE.into(), pin: None }));
        added(&mut st, Ok(()));
        assert!(st.words.is_empty());
    }
}
