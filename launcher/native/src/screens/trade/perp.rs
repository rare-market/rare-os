//! A perps market (trade/PerpPage.tsx): price and chart, the market's figures, your position with
//! Close, and the order panel. LT/RT set the leverage while the order panel has focus, X flips Long
//! and Short, the presets set the margin, A on the panel's button opens the review.

use super::chart_area::{self, ChartArea, ChartOut};
use super::fmt::*;
use super::model::*;
use super::review::{review_sheet, Review};
use super::ui::{action, chip, coin_logo, lines, side_switch, stepper, tf_chips, Action, Tone, Val};
use crate::kit::format::{num_parts, num_text, NumKind, NumOptions};
use crate::screens::market::host::*;
use rare_render::chart::{Candle, Textures};
use rare_render::input::Pad;
use rare_render::view::El;
use rare_render::{DrawList, Id, Scene, TextSys};

/// Everything the page reads (useApi / useAsync answers); None while loading.
#[derive(Clone, Debug, Default)]
pub struct PerpData {
    pub now_ms: f64,
    pub market: Option<PerpMarket>,
    pub logo: Option<String>,
    pub bars: Vec<PerpBar>,
    /// None: not loaded; Some(None): the wallet has no perps account.
    pub account: Option<Option<PerpsAccount>>,
    pub preview: Option<OrderPreview>,
    pub preview_error: Option<String>,
    pub usdc: Option<f64>,
    /// The Rare Wallet account trading (useTradeAccount), if any.
    pub wallet: Option<String>,
    pub markets_error: bool,
}

impl PerpData {
    pub fn from_fixture(f: &PerpFixture, wallet: Option<&str>) -> PerpData {
        let market = f.markets.items.iter().find(|m| m.market_id == f.market_id).cloned();
        let logo = market.as_ref().and_then(|m| perp_logo(&f.logos.items, &m.market_id, &m.symbol)).map(str::to_string);
        PerpData {
            now_ms: f.now,
            market,
            logo,
            bars: f.bars.clone(),
            account: Some(f.account.account.clone()),
            preview: f.preview.clone(),
            preview_error: None,
            usdc: f.usdc,
            wallet: wallet.map(str::to_string),
            markets_error: false,
        }
    }

    pub fn position(&self) -> Option<&PerpPosition> {
        let m = self.market.as_ref()?;
        self.account.as_ref()?.as_ref()?.positions.iter().find(|p| p.market_id == m.market_id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sheet {
    Open,
    Close,
}

/// The ticket: what the pad changes on this page.
#[derive(Clone, Debug, PartialEq)]
pub struct Ticket {
    pub side: PerpSide,
    pub margin: f64,
    pub lev: f64,
    /// Index into PERP_TIMEFRAMES.
    pub tf: usize,
    pub sheet: Option<Sheet>,
}

impl Default for Ticket {
    fn default() -> Ticket {
        Ticket { side: PerpSide::Long, margin: 10.0, lev: 5.0, tf: 1, sheet: None }
    }
}

// Focus tags.
pub const T_TF: u32 = 100;
pub const T_CHART: u32 = 150;
pub const T_SIDE: u32 = 200;
pub const T_MARGIN: u32 = 300;
pub const T_LEV_MINUS: u32 = 400;
pub const T_LEV_PLUS: u32 = 401;
pub const T_REVIEW: u32 = 500;
pub const T_CLOSE: u32 = 600;

/// The order panel's derived words and switches (PerpPage's `plan`, `blocked`, `why`, the button).
#[derive(Clone, Debug, PartialEq)]
pub struct OrderView {
    pub plan: FundingPlan,
    pub blocked: bool,
    pub why: Option<String>,
    pub button: String,
    pub disabled: bool,
}

pub fn order_view(d: &PerpData, t: &Ticket) -> OrderView {
    let avail = d.account.as_ref().map(|a| a.as_ref().map(|x| x.available_usd));
    let plan = funding_plan(avail, t.margin, d.preview.as_ref().map(|p| p.fee_usd).unwrap_or(0.0), d.usdc);
    let min_order = d.market.as_ref().and_then(|m| m.min_order_usd).unwrap_or(1.0);
    let too_small = t.margin * t.lev < min_order;
    let blocked = d.market.is_none() || d.preview.is_none() || plan.blocked || too_small;
    let why = if d.wallet.is_none() {
        Some("Add a wallet first".to_string())
    } else if plan.needs_account {
        Some("One time, before your first order".into())
    } else if plan.blocked {
        Some("Not enough USDC in your wallet".into())
    } else if too_small {
        Some(format!("Minimum order {}", perp_px(Some(min_order))))
    } else {
        d.preview_error.clone()
    };
    let button = if plan.needs_account && d.wallet.is_some() { "Set up perps".to_string() } else { format!("Review {} {}", t.side.lower(), fmt_leverage(t.lev)) };
    OrderView { plan, blocked, why, button, disabled: blocked && !plan.needs_account }
}

/// The review sheet's content for the ticket (PerpPage's three ReviewSheets).
pub fn review_for(d: &PerpData, t: &Ticket, ov: &OrderView) -> Option<Review> {
    let m = d.market.as_ref()?;
    match t.sheet? {
        Sheet::Open if ov.plan.needs_account => Some(Review {
            title: "Set up perps".into(),
            sub: Some("One time, before your first order".into()),
            action: "Create account".into(),
            tone: Tone::Primary,
            lines: vec![("What it does".into(), Val::t("Creates your perps account for USDC margin"), false), ("After that".into(), Val::t("Review your order again to open it"), false)],
        }),
        Sheet::Open => {
            let p = d.preview.as_ref()?;
            let mut lines = vec![
                ("Margin".to_string(), Val::t(format!("${}", trim_num(t.margin))), false),
                ("Size".into(), Val::t(format!("{} {}, {}", num_size(p.size_units), m.symbol, num_text(NumKind::Money, Some(p.notional_usd), NumOptions::default()))), true),
                ("Entry about".into(), Val::t(perp_px(Some(p.entry_price))), false),
                ("Liquidation".into(), Val::t(liq_text(p.liquidation_price, p.entry_price)), false),
                ("Fees".into(), Val::n(NumKind::Money, Some(p.fee_usd)), false),
            ];
            if ov.plan.short_by > 0.0 {
                lines.push(("Adds from your wallet".into(), Val::t(format!("{} USDC", perp_px(Some(ov.plan.short_by)))), false));
            }
            lines.push(("Slippage".into(), Val::t("1%"), false));
            Some(Review {
                title: format!("{} {} {}", t.side.word(), m.symbol, fmt_leverage(t.lev)),
                sub: Some("Market order".into()),
                action: format!("Open {}", t.side.lower()),
                tone: if t.side == PerpSide::Long { Tone::Buy } else { Tone::Sell },
                lines,
            })
        }
        Sheet::Close => {
            let p = d.position()?;
            Some(Review {
                title: format!("Close {} {}", m.symbol, p.side.lower()),
                sub: Some("Market order for the whole position".into()),
                action: "Close position".into(),
                tone: Tone::Primary,
                lines: vec![
                    ("Position".into(), Val::t(format!("{} {}", p.side.word(), fmt_leverage(p.leverage))), false),
                    ("Entry".into(), Val::t(perp_px(Some(p.entry_price))), false),
                    ("Mark".into(), Val::t(perp_px(p.mark_price)), false),
                    ("P/L if closed now".into(), Val::n(NumKind::Pnl, Some(p.pnl_usd)), true),
                ],
            })
        }
    }
}

pub struct PerpPage {
    pub data: PerpData,
    pub t: Ticket,
    pub chart: ChartArea,
    pub host: Host,
}

/// The perps chart's price labels: perpPx, "$0" for nothing.
fn chart_px(v: f64) -> String {
    let s = perp_px(Some(v));
    if s.is_empty() {
        "$0".into()
    } else {
        s
    }
}

impl PerpPage {
    pub fn new(cx: &mut Cx, parent: Id, data: PerpData, now: f64) -> PerpPage {
        let mut chart = ChartArea::new(Box::new(chart_px), PERP_TIMEFRAMES.len(), 1);
        chart.tz = Some(0);
        let mut p = PerpPage { data, t: Ticket::default(), chart, host: Host::new(cx, parent) };
        p.load_bars();
        p.render(cx, now);
        p
    }

    fn fit_key(&self) -> String {
        format!("{}:{}", self.data.market.as_ref().map(|m| m.market_id.as_str()).unwrap_or(""), PERP_TIMEFRAMES[self.t.tf].0)
    }

    /// New answers (live data): the ticket and chart view stay.
    pub fn set_data(&mut self, cx: &mut Cx, data: PerpData, now: f64) {
        let bars = data.bars != self.data.bars;
        self.data = data;
        if bars {
            self.load_bars();
        }
        self.render(cx, now);
    }

    fn load_bars(&mut self) {
        let rows: Vec<Candle> = self.data.bars.iter().map(|b| Candle { time: (b.time / 1000.0).floor() as i64, open: b.open, high: b.high, low: b.low, close: b.close, volume: b.volume }).collect();
        let key = self.fit_key();
        self.chart.tf = self.t.tf;
        self.chart.set_rows(&rows, &key);
    }

    pub fn render(&mut self, cx: &mut Cx, now: f64) {
        let focused_chart = self.host.tag() == Some(T_CHART);
        let (d, t, chart) = (&self.data, &self.t, &self.chart);
        self.host.render(cx, now, |ui| view(ui, d, t, chart, focused_chart));
    }

    /// Which part of the page holds focus.
    fn area(&self) -> &'static str {
        match self.host.tag() {
            Some(T_CHART) => "chart",
            Some(t) if (T_SIDE..T_CLOSE).contains(&t) => "order",
            _ => "other",
        }
    }

    /// The hint bar (PerpPage's usePageHints, the sheet's, or chart mode's).
    pub fn page_hints(&self) -> Vec<(&'static str, String)> {
        if self.t.sheet.is_some() {
            return vec![("a", "Select".into()), ("b", "Cancel".into())];
        }
        if self.chart.mode.on {
            return chart_area::HINTS.iter().map(|(b, l)| (*b, l.to_string())).collect();
        }
        let flip = ("x", if self.t.side == PerpSide::Long { "Short" } else { "Long" }.to_string());
        let sel = ("a", "Select".to_string());
        let search = ("y", "Search".to_string());
        let back = ("b", "Back".to_string());
        match self.area() {
            "order" => vec![sel, ("lt", "Less leverage".into()), ("rt", "More leverage".into()), flip, search, back],
            "chart" => vec![("a", "Control chart".into()), ("select", "Chart mode".into()), flip, search, back],
            _ => vec![sel, flip, search, back],
        }
    }

    /// A press; returns what the app should do.
    pub fn press(&mut self, cx: &mut Cx, b: Pad, repeat: bool, now: f64) -> Out {
        if self.t.sheet.is_some() {
            return match b {
                Pad::B => {
                    self.t.sheet = None;
                    self.render(cx, now);
                    Out::Moved
                }
                Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                    if self.host.step(cx, b, now) {
                        Out::Moved
                    } else {
                        Out::None
                    }
                }
                Pad::A => match self.host.tag() {
                    Some(super::review::T_CONFIRM) => Out::Act("perp:sign".into()),
                    Some(super::review::T_CANCEL) => {
                        self.t.sheet = None;
                        self.render(cx, now);
                        Out::Moved
                    }
                    _ => Out::None,
                },
                _ => Out::None,
            };
        }
        // Chart mode takes everything while it is on; Select toggles it from anywhere on the page.
        let focused = self.host.tag() == Some(T_CHART);
        match self.chart.press(b, repeat, focused) {
            ChartOut::Pass => {}
            ChartOut::Tf(i) => {
                self.t.tf = i;
                self.chart.tf = i;
                self.render(cx, now);
                return Out::Act(format!("tf:{}", PERP_TIMEFRAMES[i].0));
            }
            ChartOut::Took | ChartOut::Mode(_) => {
                self.render(cx, now);
                return Out::Moved;
            }
        }
        let stops = leverage_stops(self.data.market.as_ref().and_then(|m| m.max_leverage));
        let max = *stops.last().unwrap_or(&1.0);
        match b {
            Pad::Lt | Pad::Rt if self.area() == "order" => {
                self.t.lev = step(&stops, self.t.lev, if b == Pad::Rt { 1 } else { -1 }, max, 1.0);
                self.render(cx, now);
                Out::Moved
            }
            Pad::X => {
                self.t.side = self.t.side.flip();
                self.render(cx, now);
                Out::Moved
            }
            Pad::B => Out::Close,
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                let was = self.host.tag();
                let moved = self.host.step(cx, b, now);
                if was != self.host.tag() && (was == Some(T_CHART) || self.host.tag() == Some(T_CHART)) {
                    // "Control chart" shows only while the chart has focus.
                    self.render(cx, now);
                }
                if moved {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Pad::A => self.activate(cx, now),
            Pad::Y => Out::Act("search".into()),
            _ => Out::None,
        }
    }

    fn activate(&mut self, cx: &mut Cx, now: f64) -> Out {
        let Some(tag) = self.host.tag() else { return Out::None };
        let stops = leverage_stops(self.data.market.as_ref().and_then(|m| m.max_leverage));
        let max = *stops.last().unwrap_or(&1.0);
        match tag {
            t if (T_TF..T_TF + 10).contains(&t) => {
                self.t.tf = (t - T_TF) as usize;
                self.chart.tf = self.t.tf;
                self.render(cx, now);
                return Out::Act(format!("tf:{}", PERP_TIMEFRAMES[self.t.tf].0));
            }
            t if t == T_SIDE || t == T_SIDE + 1 => self.t.side = if t == T_SIDE { PerpSide::Long } else { PerpSide::Short },
            t if (T_MARGIN..T_MARGIN + 10).contains(&t) => self.t.margin = MARGIN_PRESETS[(t - T_MARGIN) as usize],
            T_LEV_MINUS => self.t.lev = step(&stops, self.t.lev, -1, max, 1.0),
            T_LEV_PLUS => self.t.lev = step(&stops, self.t.lev, 1, max, 1.0),
            T_REVIEW => {
                if order_view(&self.data, &self.t).disabled {
                    return Out::None;
                }
                self.t.sheet = Some(Sheet::Open);
            }
            T_CLOSE => self.t.sheet = Some(Sheet::Close),
            _ => return Out::None,
        }
        self.render(cx, now);
        Out::Moved
    }
}

/// The page (PerpPage's JSX).
pub fn view(ui: &mut Ui, d: &PerpData, t: &Ticket, chart: &ChartArea, focused_chart: bool) -> El {
    let page = ui.bx(s().fill().col().pt(1.0).pb(2.5)).scrolls_y(rare_render::view::key("tr-perp-scroll"));
    let Some(m) = d.market.as_ref() else {
        let mut page = page;
        if d.markets_error {
            page = page.child(ui.bx(s().pt(5.0).row().justify_center()).child(ui.text("This market is unavailable right now.", T::new(Ty::Small).c(tok::MUTED))));
        } else {
            page = page.child(super::ui::loading_bar(ui));
        }
        return page;
    };
    let ov = order_view(d, t);

    // ---- left column
    let logo = d.logo.as_deref().and_then(|l| logo_url(Some(l)));
    let ch = num_parts(NumKind::Change, m.change_pct, NumOptions::default());
    let ch_col = ch.as_ref().map(|p| crate::kit::num::tone_color(p.tone, tok::FG)).unwrap_or(tok::FG);
    let title = baseline_row(ui, s().gap(0.5), vec![(T::new(Ty::Title).w(600), m.symbol.clone()), (T::new(Ty::Body).w(500).c(tok::MUTED), format!("{} perp", pair_label(&m.pair)))]);
    let price = baseline_row(
        ui,
        s().gap(0.75),
        vec![(T::new(Ty::Display).w(600).tab(), perp_px(Some(m.price))), (T::new(Ty::Body).tab().c(ch_col), ch.map(|p| p.text()).unwrap_or_default()), (T::new(Ty::Small).c(tok::MUTED), "24h".into())],
    );
    let price = roll_first(ui, price, Some(m.price));
    let names = ui.bx(s().col().shrink().min0()).child(title).child(price);
    let labels: Vec<&str> = PERP_TIMEFRAMES.iter().map(|x| x.0).collect();
    let logo_el = coin_logo(ui, logo.as_deref(), &m.symbol, 3.5);
    let chips_tf = tf_chips(ui, &labels, t.tf, "tr-tf", T_TF);
    let head = ui.bx(s().row().items_center().gap(1.0)).child(logo_el).child(names).child(ui.bx(s().ml_auto()).child(chips_tf));

    let o = NumOptions::default();
    let stats = stat_line(
        ui,
        s(),
        vec![
            ("24h volume", Stat::Num(NumKind::Market, m.volume_usd, o)),
            ("Open interest", Stat::Num(NumKind::Market, m.open_interest_usd, o)),
            ("Funding 1h", Stat::Text(m.funding_rate.map(|f| funding_pct(f, 1.0)).unwrap_or_default())),
            ("Next funding", Stat::Text(m.next_funding_ms.filter(|v| *v != 0.0).map(|v| until_text(v, d.now_ms)).unwrap_or_default())),
            ("Max leverage", Stat::Text(fmt_leverage(m.max_leverage.unwrap_or(1.0)))),
            ("Fees", Stat::Text(m.taker_fee.map(|f| format!("{:.3}%", f * 100.0)).unwrap_or_default())),
        ],
        true,
        false,
    );
    let stats_panel = kit::panel(ui, s().col().mt(1.0).px(1.25).py(0.75)).maybe(stats);

    let mut pos = kit::panel(ui, s().col().mt(1.25).p(1.25)).child(ui.text("Your position", T::new(Ty::Row).w(600)));
    match d.position() {
        Some(p) => {
            let sl = stat_line(
                ui,
                s().flex(1.0),
                vec![
                    ("Side", Stat::Text(format!("{} {}", p.side.word(), fmt_leverage(p.leverage)))),
                    ("Size", Stat::Num(NumKind::Money, Some(p.notional_usd), NumOptions { compact: true, ..Default::default() })),
                    ("Entry", Stat::Text(perp_px(Some(p.entry_price)))),
                    ("Liquidation", Stat::Text(perp_px(p.liquidation_price))),
                    ("P/L", Stat::Num(NumKind::Pnl, Some(p.pnl_usd), NumOptions::default())),
                ],
                true,
                false,
            );
            let close = action(ui, s(), Action { hint: Some("Close position".into()), ..Action::new("Close", Tone::Secondary, T_CLOSE) });
            pos = pos.child(ui.bx(s().row().items_center().mt(0.75).gap(1.5)).maybe(sl).child(close));
        }
        None => {
            let msg = if d.wallet.is_some() { "No open position in this market." } else { "Add a Rare Wallet account to trade perps." };
            pos = pos.child(ui.text_s(s().mt(0.5), msg, T::new(Ty::Small).c(tok::MUTED)));
        }
    }
    let chart_el = chart.el(ui, s().mt(1.0).h(19.0), T_CHART, focused_chart);
    let left = ui.bx(s().col().min0()).child(head).child(chart_el).child(stats_panel).child(pos);

    // ---- order panel
    let stops = leverage_stops(m.max_leverage);
    let mut chips = ui.bx(s().row().gap(0.5)).group(kit::group("tr-margin", true));
    for (i, v) in MARGIN_PRESETS.iter().enumerate() {
        let c = chip(ui, &format!("${}", trim_num(*v)), (t.margin - v).abs() < 1e-9, T_MARGIN + i as u32);
        chips = chips.child(c);
    }
    let mut dots = ui.bx(s().row().items_center().gap(0.25).flex(1.0).min0());
    for st in &stops {
        dots = dots.child(ui.bx(s().h(0.375).flex(1.0).min0()).bg(if *st <= t.lev { tok::FG } else { tok::L4 }).radius(ui.r(0.1875)));
    }
    let lev_mid = ui.bx(s().row().items_center().gap(0.75).wp(1.0)).child(dots).child(right_text(ui, s().w(3.5), fmt_leverage(t.lev), T::new(Ty::Row).w(600).tab()));
    let lev = stepper(ui, "leverage", "tr-lev", T_LEV_MINUS, T_LEV_PLUS, lev_mid);
    let pv = d.preview.as_ref();
    let mut items: Vec<(&str, Val, bool)> = vec![
        ("Size", Val::t(pv.map(|p| format!("{} {}, {}", num_size(p.size_units), m.symbol, num_text(NumKind::Money, Some(p.notional_usd), NumOptions::default()))).unwrap_or_default()), false),
        ("Entry", Val::t(pv.map(|p| perp_px(Some(p.entry_price))).unwrap_or_default()), false),
        ("Liquidation", Val::t(pv.map(|p| liq_text(p.liquidation_price, p.entry_price)).unwrap_or_default()), false),
        ("Fees", pv.map(|p| Val::n(NumKind::Money, Some(p.fee_usd))).unwrap_or(Val::None), false),
    ];
    if d.wallet.is_some() {
        let label = if ov.plan.needs_account { "Perps account" } else { "From your wallet" };
        let v = if ov.plan.needs_account {
            "Set up first".to_string()
        } else if ov.plan.short_by > 0.0 {
            format!("{} USDC", perp_px(Some(ov.plan.short_by)))
        } else {
            "Nothing".into()
        };
        items.push((label, Val::t(v), false));
    }
    items.push(("Wallet USDC", d.usdc.map(|u| Val::n(NumKind::Money, Some(u))).unwrap_or(Val::None), false));
    let tone = if t.side == PerpSide::Long { Tone::Buy } else { Tone::Sell };
    let review = action(ui, s(), Action { def: true, full: true, disabled: ov.disabled, hint: Some(ov.why.clone().unwrap_or_else(|| "Review".into())), ..Action::new(ov.button.clone(), tone, T_REVIEW) });
    let switch = side_switch(ui, [("Long", Tone::Buy), ("Short", Tone::Sell)], if t.side == PerpSide::Long { 0 } else { 1 }, "tr-side", T_SIDE);
    let margin = ui.bx(s().col()).child(ui.text("Margin", T::new(Ty::Small).c(tok::MUTED))).child(center_text(ui, s().mt(0.25), format!("${}", trim_num(t.margin)), T::new(Ty::Display).w(600).tab()));
    let leverage = ui.bx(s().col()).child(ui.text("Leverage", T::new(Ty::Small).c(tok::MUTED))).child(ui.bx(s().col().mt(0.25)).child(lev));
    let figures = lines(ui, &items);
    let mut order = kit::panel(ui, s().col().gap(1.0).p(1.25).self_start()).child(switch).child(margin).child(chips).child(leverage).child(figures).child(review);
    if let Some(w) = &ov.why {
        order = order.child(center_text(ui, s().mt(-0.5), w.clone(), T::new(Ty::Label).c(tok::MUTED)));
    }

    let grid = ui.bx(s().grid(&[Track::Fr(1.0), Track::Rem(24.0)]).gap(1.5).px(2.5).items_start()).child(left).child(order);
    let mut page = page.child(grid);
    if let Some(r) = review_for(d, t, &ov) {
        page = page.child(review_sheet(ui, &r, d.wallet.is_some()));
    }
    page
}

/// 10 -> "10", 2.5 -> "2.5".
pub fn trim_num(v: f64) -> String {
    trim_zeros(&format!("{v:.4}"))
}

impl Page for PerpPage {
    fn any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn prepare_paint(&mut self, scene: &Scene, ts: &mut TextSys, tex: &mut dyn Textures) {
        if self.data.market.is_none() {
            return;
        }
        let Some(area) = self.host.id(chart_area::AREA) else { return };
        self.chart.prepare(scene, area, ts, tex);
    }
    fn paint_slot(&mut self, sp: &rare_render::scene::SlotPaint, ts: &mut TextSys, out: &mut DrawList) {
        self.chart.paint_slot(sp, ts, out);
    }
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.page_hints()
    }
}

impl Screen for PerpPage {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.host.touch_focus(cx, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        self.press(cx, pad, false, now)
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if self.host.tick(cx, now) {
            self.render(cx, now);
        }
        self.chart.tick(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> PerpData {
        PerpData::from_fixture(&perp_btc(), Some("0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e"))
    }

    #[test]
    fn order_panel_words_match_the_web() {
        let d = data();
        let ov = order_view(&d, &Ticket::default());
        assert!(ov.plan.needs_account);
        assert_eq!(ov.button, "Set up perps");
        assert_eq!(ov.why.as_deref(), Some("One time, before your first order"));
        assert!(!ov.disabled);
        let mut nw = d.clone();
        nw.wallet = None;
        let ov = order_view(&nw, &Ticket::default());
        assert_eq!(ov.button, "Review long 5x");
        assert_eq!(ov.why.as_deref(), Some("Add a wallet first"));
        let mut ready = d.clone();
        ready.account = Some(Some(PerpsAccount { available_usd: 100.0, ..Default::default() }));
        let ov = order_view(&ready, &Ticket { side: PerpSide::Short, lev: 10.0, ..Default::default() });
        assert_eq!(ov.button, "Review short 10x");
        assert_eq!(ov.why, None);
        assert!(!ov.disabled);
    }

    #[test]
    fn review_sheets_say_what_the_web_says() {
        let d = data();
        let t = Ticket { sheet: Some(Sheet::Open), ..Default::default() };
        let r = review_for(&d, &t, &order_view(&d, &t)).unwrap();
        assert_eq!(r.title, "Set up perps");
        assert_eq!(r.action, "Create account");
        let mut ready = d.clone();
        ready.account = Some(Some(PerpsAccount { available_usd: 5.0, ..Default::default() }));
        let t = Ticket { sheet: Some(Sheet::Open), side: PerpSide::Short, ..Default::default() };
        let r = review_for(&ready, &t, &order_view(&ready, &t)).unwrap();
        assert_eq!(r.title, "Short BTC 5x");
        assert_eq!(r.action, "Open short");
        assert_eq!(r.tone, Tone::Sell);
        assert!(r.lines.iter().any(|l| l.0 == "Adds from your wallet"));
        assert_eq!(r.lines.last().unwrap().0, "Slippage");
    }
}
