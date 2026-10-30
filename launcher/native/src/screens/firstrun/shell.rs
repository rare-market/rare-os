//! First run (launcher/src/screens/FirstRun.tsx): one decision per step: Welcome (with the
//! language), Wi-Fi, time zone, sign in, your picture (with an account), Steam, Epic Games and GOG,
//! emulators, the controller check. Steps with nothing to do are passed over going forward. The brand top
//! left, a 6 px dot per step top right (current white, done 40 %, next 15 %); B goes back a step,
//! Y skips setup. The step on screen is saved in the config, so a restart resumes there.

use crate::screens::signin::brand_mark;
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::{button_el, hint_el, load_bar, BSize, ButtonSpec, BVariant};

pub const STEPS: [&str; 9] = ["welcome", "wifi", "time", "passkey", "picture", "steam", "stores", "emulators", "controller"];

/// lib/locales.ts: English only for now.
pub const LANGUAGES: [(&str, &str); 1] = [("en", "English")];

pub fn language_name(code: Option<&str>) -> &'static str {
    LANGUAGES.iter().find(|(c, _)| Some(*c) == code).map(|(_, n)| *n).unwrap_or(LANGUAGES[0].1)
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub i: usize,
    /// the steps that were shown, for B
    pub hist: Vec<usize>,
    pub finishing: bool,
    pub lang: String,
    pub picking_lang: bool,
}

impl Default for State {
    fn default() -> Self {
        State { i: 0, hist: Vec::new(), finishing: false, lang: "en".into(), picking_lang: false }
    }
}

impl State {
    /// `?step=` or the saved `onboardingStep` (a later step resumes there).
    pub fn resume(saved: Option<&str>) -> State {
        let i = saved.and_then(|s| STEPS.iter().position(|x| *x == s)).filter(|k| *k > 0).unwrap_or(0);
        State { i, ..State::default() }
    }
    pub fn step(&self) -> &'static str {
        STEPS[self.i]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Next,
    Back,
    Skip,
    PickLanguage,
    Language(String),
    /// LB, RB, Menu and Guide do nothing during setup (the shell's tabs and menus stay shut)
    Swallow,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// ask whether this step has nothing to do (Wi-Fi when online): answer with `skip_answer`
    ShouldSkip(usize),
    /// save onboardingStep (config) when the step changes
    SaveStep(&'static str),
    /// onboarded, setup done, scan and read the wallet (at most 4 s of the bar), close, toast
    Finish,
    SetLanguage(String),
    /// feedback sound
    Sound(&'static str),
}

/// `main` is the registered step's body for wifi, passkey and steam (or None for the welcome).
pub fn view(st: &State, main: Option<El<Msg>>, act_live: bool) -> El<Msg> {
    view_with(st, main, act_live, &|m| m)
}

/// `view` over the app's own message type: `f` wraps the shell's messages, `main` is already in `M`.
pub fn view_with<M: Clone>(st: &State, main: Option<El<M>>, act_live: bool, f: &impl Fn(Msg) -> M) -> El<M> {
    let welcome = st.i == 0;
    let mut root = div("fade fixed inset-0 z-40 flex flex-col overflow-hidden bg-bg").attr("data-nav-scope", "");
    if welcome && act_live {
        root = root.kid(
            div("absolute inset-y-0 left-[34%] right-0 overflow-hidden")
                .attr("aria-hidden", "true")
                // Native cover crops about the centre only: the art keeps its aspect at full height, right of centre.
                .kid(img(Some("/games/act/hero.jpg"), "", "fade absolute -right-[26%] top-[6%] h-[88%] object-cover").attr("data-width", "1920").attr("data-height", "620"))
                .kid(div("absolute inset-y-0 left-0 w-[45%] bg-gradient-to-r from-bg to-transparent"))
                .kid(div("absolute inset-x-0 bottom-0 h-[25%] bg-gradient-to-t from-bg to-transparent")),
        );
    }
    let dots = span("ml-auto flex items-center gap-[0.5rem]")
        .role("progressbar")
        .label(&format!("Step {} of {}", st.i + 1, STEPS.len()))
        .kids(STEPS.iter().enumerate().map(|(k, s)| span(cn(&["h-[0.375rem] w-[0.375rem] rounded-full transition-colors duration-200", if k == st.i { "bg-fg" } else if k < st.i { "bg-fg/40" } else { "bg-fg/15" }])).key(*s)));
    root = root.kid(
        el(Tag::Div, "relative flex h-[4.5rem] shrink-0 items-center gap-[1.5rem] px-[4rem]")
            .attr("data-tag", "header")
            .kid(span("flex items-center gap-[0.5rem]").kid(brand_mark("1.5rem")).kid(span("text-[1.25rem] font-semibold tracking-[-0.03em]").text("rare")))
            .kid(dots),
    );
    let body = if st.finishing {
        div("flex h-full items-center text-row text-muted").kid(text("Getting your games ready")).kid(load_bar(true, "top-[4.5rem]"))
    } else if let Some(m) = main {
        m
    } else {
        welcome_view(st).map(f)
    };
    root = root.kid(el(Tag::Div, "rise relative min-h-0 flex-1 px-[4rem]").attr("data-tag", "main").key(st.step()).kid(body));
    let mut foot = el(Tag::Div, cn(&["relative flex h-[3rem] shrink-0 items-center justify-end gap-[1.5rem] px-[4rem] text-small text-fg/80", when(!welcome, "border-t border-hair")])).attr("data-tag", "footer").kid(hint_el("a", "Select"));
    if st.i > 0 {
        foot = foot.kid(hint_el("b", "Back"));
    }
    // The controller check lights Y instead (it is the last step: Continue finishes the same way).
    let skip = st.step() != "controller";
    if skip {
        foot = foot.kid(hint_el("y", "Skip setup"));
    }
    root = root.kid(foot);
    // A registered step handles B itself; the welcome's B does nothing.
    let mut root = root.on_pad("y", f(Msg::Skip)).on_pad("lb", f(Msg::Swallow)).on_pad("rb", f(Msg::Swallow)).on_pad("start", f(Msg::Swallow)).on_pad("guide", f(Msg::Swallow));
    if main_is_welcome(st) {
        root = root.on_pad("b", f(Msg::Back));
    }
    root
}

fn main_is_welcome(st: &State) -> bool {
    st.i == 0
}

fn welcome_view(st: &State) -> El<Msg> {
    let mut d = div("flex h-full max-w-[40rem] flex-col justify-center pb-[2rem]")
        .kid(h1("text-[3.75rem] font-semibold leading-[1.02] tracking-[-0.04em]").text("Welcome to Rare OS"))
        .kid(
            div("mt-[2.5rem] flex flex-wrap gap-[0.75rem]")
                .attr("data-nav-group", "firstrun-actions")
                .attr("data-nav-remember", "")
                .kid(button_el(ButtonSpec::new("Get started").v(BVariant::Primary).size(BSize::Lg).def(true).class("min-w-[11rem]"), Some(Msg::Next)))
                .kid(button_el(ButtonSpec::new(language_name(Some(&st.lang))).size(BSize::Lg).v(BVariant::Ghost).hint("Language"), Some(Msg::PickLanguage))),
        );
    if st.picking_lang {
        d = d.kid(div("fade mt-[1rem] flex w-[22rem] flex-col gap-[0.4rem]").attr("data-nav-group", "firstrun-language").role("radiogroup").kids(LANGUAGES.iter().map(|(code, name)| {
            let on = *code == st.lang;
            button(cn(&["flex items-center justify-between rounded-card border px-[1.2rem] py-[0.8rem] text-left text-body", if on { "border-line-2 bg-l2" } else { "border-hair bg-l1" }]), Some(Msg::Language(code.to_string())))
                .def(on)
                .role("radio")
                .attr("aria-checked", on.to_string())
                .key(*code)
                .kid(text(*name))
                .kid_if(on, || icon("check", "").attr("size", "1rem").attr("stroke-width", "2.5"))
        })));
    }
    d
}

/// Applies a message. Going forward asks `ShouldSkip` for each next step until one has work.
pub fn update(st: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Next => {
            let k = st.i + 1;
            if k >= STEPS.len() {
                return finish(st);
            }
            Some(Effect::ShouldSkip(k))
        }
        Msg::Back => {
            if st.i == 0 {
                return None;
            }
            st.i = st.hist.pop().unwrap_or(st.i.saturating_sub(1));
            Some(Effect::SaveStep(st.step()))
        }
        Msg::Skip => finish(st),
        Msg::PickLanguage => {
            st.picking_lang = !st.picking_lang;
            None
        }
        Msg::Language(c) => {
            st.lang = c.clone();
            st.picking_lang = false;
            Some(Effect::SetLanguage(c))
        }
        Msg::Swallow => None,
    }
}

/// The answer to `ShouldSkip(k)`: skip moves on to the next, else the step shows.
pub fn skip_answer(st: &mut State, k: usize, skip: bool) -> Option<Effect> {
    if skip {
        if k + 1 >= STEPS.len() {
            return finish(st);
        }
        return Some(Effect::ShouldSkip(k + 1));
    }
    st.hist.push(st.i);
    st.i = k;
    Some(Effect::SaveStep(st.step()))
}

fn finish(st: &mut State) -> Option<Effect> {
    if st.finishing {
        return None;
    }
    st.finishing = true;
    Some(Effect::Finish)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome() {
        let st = State::default();
        let v = view(&st, None, true);
        assert_eq!(v.texts(), ["rare", "Welcome to Rare OS", "Get started", "English", "Select", "Skip setup"]);
        assert_eq!(v.landing().as_deref(), Some("Get started"));
        assert_eq!(v.pad("y"), Some(&Msg::Skip));
        assert!(v.find_by_attr("role", "progressbar").unwrap().get_attr("aria-label") == Some("Step 1 of 9"));
        // ACT not live: the page is typographic
        let v = view(&st, None, false);
        assert!(!v.dump().contains("hero.jpg"));
    }

    #[test]
    fn steps_skip_and_back() {
        let mut st = State::default();
        assert_eq!(update(&mut st, Msg::Next), Some(Effect::ShouldSkip(1)));
        // online: Wi-Fi is passed over
        assert_eq!(skip_answer(&mut st, 1, true), Some(Effect::ShouldSkip(2)));
        assert_eq!(skip_answer(&mut st, 2, false), Some(Effect::SaveStep("time")));
        assert_eq!(st.i, 2);
        let v = view(&st, Some(div("").text("body")), true);
        assert!(v.contains_text("Back"));
        // B walks back through the steps that were shown: to the welcome, not Wi-Fi
        assert_eq!(update(&mut st, Msg::Back), Some(Effect::SaveStep("welcome")));
        assert_eq!(st.i, 0);
        st.i = STEPS.len() - 1;
        assert_eq!(update(&mut st, Msg::Next), Some(Effect::Finish));
        assert!(view(&st, None, true).contains_text("Getting your games ready"));
        assert_eq!(update(&mut st, Msg::Skip), None);
        assert_eq!(State::resume(Some("steam")).i, 5);
        assert_eq!(State::resume(Some("picture")).i, 4);
        assert_eq!(State::resume(Some("welcome")).i, 0);
    }

    #[test]
    fn language() {
        let mut st = State::default();
        update(&mut st, Msg::PickLanguage);
        let v = view(&st, None, true);
        assert!(v.find_by_attr("data-nav-group", "firstrun-language").is_some());
        assert_eq!(update(&mut st, Msg::Language("en".into())), Some(Effect::SetLanguage("en".into())));
        assert!(!st.picking_lang);
    }
}
