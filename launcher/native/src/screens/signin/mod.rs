//! First-run sign in (launcher/src/screens/SignIn.tsx). A passkey is the account; the choices open
//! Rare Wallet on the matching page (the same screens the wallet's own welcome uses), so there is
//! one set of flows to keep right. Phase 3, workstream 14.

use super::wallet::home::stack::{PasskeyMode, WalletView};
use super::wallet::markup::*;
use super::wallet::ui::{button_el, ButtonSpec, BVariant};

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    /// open the wallet tab on this page (wallet/signin.ts `openWalletPage`)
    Open(WalletView),
    /// Not now: mark onboarded and close
    NotNow,
}

/// `BrandMark`: the Rare mark at a height, width auto.
pub fn brand_mark<M>(size: &str) -> El<M> {
    img(Some("/brand/mark.webp"), "", "").attr("style", format!("height: {size}; width: auto"))
}

pub fn view() -> El<Msg> {
    div("fade-in fixed inset-0 z-40 flex items-center justify-center bg-bg").attr("data-nav-scope", "").kid(
        div("flex w-[26rem] flex-col items-center gap-[0.8rem]")
            .kid(brand_mark("4.2rem"))
            .kid(h1("mt-[0.6rem] text-[1.9rem] font-bold tracking-[-0.03em]").text("Sign in to Rare"))
            .kid(p("mb-[0.8rem] text-center text-[0.92rem] leading-relaxed text-muted").text("One account for Rare Wallet and Rare Marketplace. Your games work without signing in."))
            .kid(button_el(ButtonSpec::new("Sign in with a passkey").v(BVariant::Primary).def(true).class("w-full"), Some(Msg::Open(WalletView::Passkey { mode: PasskeyMode::Signin }))))
            .kid(button_el(ButtonSpec::new("Create account").class("w-full"), Some(Msg::Open(WalletView::Passkey { mode: PasskeyMode::Create }))))
            .kid(button_el(ButtonSpec::new("Other ways to sign in").class("w-full"), Some(Msg::Open(WalletView::Add))))
            .kid(button_el(ButtonSpec::new("Not now").v(BVariant::Ghost).class("mt-[0.4rem]"), Some(Msg::NotNow))),
    )
}

/// The sign-in overlay as an app page (`Overlay::Other("signin")`): a choice closes it and opens the
/// Wallet tab on that page; Not now closes it.
#[derive(Default)]
pub struct SignInPage {
    built: crate::screens::wallet::render::Built<Msg>,
}

impl crate::page::Page for SignInPage {
    fn stamp(&self, _st: &crate::state::State) -> u64 {
        0
    }

    fn view(&mut self, ui: &mut crate::kit::Ui, _st: &crate::state::State, acts: &mut crate::page::Acts) -> rare_render::view::El {
        // In the overlay host the page fills its wrapper; a fixed root would be hoisted to a layer
        // and closing it trips the engine's rounding pass (see app.rs `wrapper`).
        let mut m = view();
        m.class = m.class.replace("fixed inset-0 z-40", "h-full w-full");
        let (el, built) = crate::screens::wallet::render::build(ui, acts, &m);
        self.built = built;
        el
    }

    fn act(&mut self, st: &mut crate::state::State, fx: &mut crate::page::Fx, code: u32) {
        match self.built.msg(code) {
            Some(Msg::Open(v)) => {
                crate::screens::wallet::page::open_on(v);
                st.close();
                fx.run(crate::act::Act::Tab(crate::state::Tab::Wallet));
            }
            Some(Msg::NotNow) => {
                // SignIn.tsx close(): setup is done, Rare doesn't ask again at start.
                crate::features::config::set(&[("onboarded", serde_json::Value::Bool(true))]);
                st.close();
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in() {
        let v = view();
        assert_eq!(v.texts(), ["Sign in to Rare", "One account for Rare Wallet and Rare Marketplace. Your games work without signing in.", "Sign in with a passkey", "Create account", "Other ways to sign in", "Not now"]);
        assert_eq!(v.landing().as_deref(), Some("Sign in with a passkey"));
        assert_eq!(v.find_nav("Other ways to sign in").unwrap().on, Some(Msg::Open(WalletView::Add)));
    }
}
