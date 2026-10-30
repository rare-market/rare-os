//! The Rare profile as an overlay (`Overlay::Other("profile")`, the Rare menu's Profile): the
//! current Rare Wallet account, its played games from the library, its Sui collectibles (the
//! inventory), the SUI price from the shell's live numbers.

use super::catalog::{seed, SuiGame};
use super::profile::{self, Msg, Profile};
use crate::act::Act;
use crate::kit::Ui;
use crate::page::{hash, Acts, Fx, Page};
use crate::screens::wallet::render::{build, Built};
use crate::screens::wallet::types::ItemCard;
use crate::state::{State, Tab};
use rare_render::view::El;

pub struct ProfilePage {
    prof: Profile,
    games: Vec<SuiGame>,
    items: Option<Vec<ItemCard>>,
    coins_usd: Option<f64>,
    /// The app-level wallet data's version last read (`crate::data::wallet`); None in fixtures.
    data_ver: Option<u64>,
    built: Built<Msg>,
    ver: u64,
    picking: bool,
    /// opened straight into the picker (the profile chip): closing it closes the profile too
    chip: bool,
}

/// Set before opening the profile to land in Change picture (the top bar's profile chip).
pub static OPEN_PICKER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Default for ProfilePage {
    fn default() -> Self {
        let fixture = std::env::var("RR_FIXTURE").is_ok();
        let (accounts, selected) = if fixture { (vec![crate::screens::wallet::home::fixtures::account()], None) } else { crate::screens::wallet::page::load_accounts() };
        let cur = selected.as_ref().and_then(|s| accounts.iter().find(|a| &a.address == s)).or(accounts.first()).cloned();
        let mut prof = Profile { address: cur.as_ref().map(|a| a.address.clone()), label: cur.as_ref().map(|a| a.label.clone()), ..Default::default() };
        if fixture {
            // RR_BANNER=<url> shows that banner (shots of uploads), else the account's default
            prof.banner = std::env::var("RR_BANNER").ok().or_else(|| prof.address.as_deref().map(|a| crate::banners::url(crate::banners::starter(a).0, false)));
        }
        let (items, data_ver) = match (&prof.address, fixture) {
            (Some(_), true) => (Some(crate::screens::inventory::fixtures::inventory().groups.iter().flat_map(|g| g.items.iter()).map(crate::screens::wallet::market::card_from).collect()), None),
            (Some(_), false) => (None, Some(0)),
            _ => (None, None),
        };
        crate::features::trophies::refresh();
        crate::features::perks::check(prof.address.clone());
        let chip = OPEN_PICKER.swap(false, std::sync::atomic::Ordering::Relaxed);
        let mut p = ProfilePage { prof, games: seed(), items, coins_usd: None, data_ver, built: Built::default(), ver: 0, picking: chip, chip };
        p.pull();
        p
    }
}

/// Nothing in `e` takes focus or pad buttons.
fn quiet<M>(e: &mut crate::screens::wallet::markup::El<M>) {
    e.nav = None;
    e.pads.clear();
    for k in &mut e.kids {
        quiet(k);
    }
}

impl ProfilePage {
    /// The picker shuts; opened from the profile chip, the profile goes with it.
    fn close_picker(&mut self, st: &mut State) {
        self.picking = false;
        self.ver += 1;
        if std::mem::take(&mut self.chip) {
            st.close();
        }
    }

    /// The account's coins total and items from the app-level wallet data (the last values at
    /// once, refreshed in place).
    fn pull(&mut self) {
        let (Some(seen), Some(a)) = (self.data_ver, self.prof.address.clone()) else { return };
        let v = crate::data::wallet::version();
        if v == seen {
            return;
        }
        self.data_ver = Some(v);
        let h = crate::data::wallet::get(&a);
        if let Some(inv) = &h.inv {
            self.items = Some(inv.groups.iter().flat_map(|g| g.items.iter()).map(crate::screens::wallet::market::card_from).collect());
        } else if h.inv_err {
            self.items = Some(Vec::new());
        }
        if let Some(p) = &h.pf {
            self.coins_usd = p.total_usd;
            self.prof.sui_mist = p.tokens.iter().find(|t| t.coin_type.ends_with("::sui::SUI")).map(|t| t.raw);
        }
        self.ver += 1;
    }
}

impl Page for ProfilePage {
    fn stamp(&self, st: &State) -> u64 {
        hash((self.ver, crate::data::live::PIC_VER.load(std::sync::atomic::Ordering::Relaxed), st.ver.live, st.library.as_ref().map(|l| l.games.len()), crate::features::trophies::shelf_ver(), crate::features::perks::ver()))
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let lib = st.library.as_ref().map(|l| l.games.as_slice()).unwrap_or(&[]);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        // The picked picture (kept per address on this device), else rare.market's; the live name.
        if let Some(a) = self.prof.address.clone() {
            self.prof.avatar = crate::data::live::picked(&a).map(|p| ItemCard { id: p.id, name: p.name, image_url: Some(p.image_url), ..Default::default() });
            if let Some(pl) = st.live.profile.as_ref().filter(|p| p.address.eq_ignore_ascii_case(&a)) {
                self.prof.market_avatar = if self.prof.avatar.is_some() { None } else { pl.avatar.clone() };
                if pl.banner.is_some() {
                    self.prof.banner = pl.banner.clone();
                }
                if !pl.name.is_empty() && !pl.name.starts_with("0x") {
                    self.prof.username = Some(pl.name.clone());
                }
            }
        }
        let pr = profile::Props {
            prof: &self.prof,
            games: &self.games,
            owned: &[],
            library: lib,
            items: self.items.as_deref(),
            coins_usd: self.coins_usd,
            live_sui_usd: st.live.sui_usd,
            change: None,
            now,
            tz_min: 0,
            perks: crate::features::perks::section(&self.games, 0).map(|e| e.map(&Msg::Perk)),
            trophies: crate::features::trophies::shelf(st, &Msg::Trophies),
            rareling_badge: crate::features::perks::badge().map(|e| e.map(&Msg::Perk)),
        };
        // In the overlay host the page fills its wrapper (a fixed root would be hoisted to a layer).
        let mut m = profile::view(&pr);
        m.class = m.class.replace("fixed inset-0 z-30", "h-full w-full");
        if self.picking {
            // The sheet owns focus and the pad: the profile behind keeps its looks but nothing in it
            // can be focused or pressed (before, focus stayed on the hidden buttons, so A pressed
            // Change picture or Change username behind the sheet and no picture was ever picked).
            quiet(&mut m);
            let current = self.prof.avatar.as_ref().map(|a| a.id.clone()).or_else(|| self.prof.market_avatar.as_deref().and_then(crate::avatars::id_of_src).map(String::from)).or_else(|| self.prof.address.as_deref().map(|a| crate::avatars::starter(a).to_string())).unwrap_or_default();
            // In the page, not a portal: closing the profile (the chip's way) takes the sheet with it.
            let mut sheet = profile::avatar_picker(self.items.as_deref(), &current);
            sheet.class = sheet.class.replace("fixed inset-x-0", "absolute inset-x-0");
            m.class.push_str(" relative");
            m = m.kid(sheet);
        }
        let (el, built) = build(ui, acts, &m);
        self.built = built;
        el
    }

    fn tick(&mut self, _st: &mut State, _fx: &mut Fx, now: f64) -> Option<f64> {
        let a = self.prof.address.clone().filter(|_| self.data_ver.is_some())?;
        crate::data::wallet::tick(&a, now);
        self.pull();
        Some(now + 0.5)
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        let Some(m) = self.built.msg(code) else { return };
        match m {
            Msg::Wallet => {
                st.close();
                fx.run(Act::Tab(Tab::Wallet));
            }
            Msg::Friends => {
                st.close();
                fx.run(Act::Tab(Tab::Friends));
            }
            Msg::Username => fx.run(Act::Legacy("accounts:pick-username".into())),
            Msg::Settings => {
                st.close();
                fx.run(Act::Tab(Tab::Settings));
            }
            Msg::OpenGame(id) | Msg::OpenGameTile(id) => fx.run(Act::OpenGame(id)),
            Msg::Trophies(a) => fx.run(Act::Trophies(a)),
            Msg::Perk(p) => match p {
                crate::screens::perks::Msg::OpenWeb(u) => fx.run(Act::OpenWeb(u)),
                crate::screens::perks::Msg::OpenGame(id) => fx.run(Act::OpenGame(id)),
                crate::screens::perks::Msg::Theme(t) => {
                    crate::features::perks::set_theme(&t);
                    self.ver += 1;
                }
            },
            Msg::Pick => {
                self.picking = true;
                self.ver += 1;
            }
            Msg::ClosePicker => self.close_picker(st),
            Msg::UseAvatar(id) => {
                let now = rare_render::clock::secs();
                // X (None): the account's own default, as a pick like any other.
                let id = id.or_else(|| self.prof.address.as_deref().map(|a| crate::avatars::starter(a).to_string()));
                if let Some(d) = id.as_deref().and_then(crate::avatars::get) {
                    crate::data::live::pick_default(d.id);
                    st.toast(&format!("{} is your picture", d.name), crate::state::Tone::Info, now);
                } else if let Some(it) = id.and_then(|id| self.items.as_ref()?.iter().find(|i| i.id == id).cloned()) {
                    crate::data::live::pick_avatar(&it.id, &it.name, it.image_url.as_deref().unwrap_or_default());
                    st.toast(&format!("{} is your picture", it.name), crate::state::Tone::Info, now);
                }
                self.close_picker(st);
            }
            Msg::Saves => st.open(Some(crate::state::Overlay::Other("saves"))),
            Msg::ViewItem(id) => {
                st.close();
                fx.run(Act::Market(format!("item:{id}")));
            }
        }
    }
}
