//! Live Sui for the Home strip: SUI price and TPS (rare.market status), reference gas (gRPC), the
//! player's portfolio and its change, and what is minting on rare.market's launchpad. Ports
//! `src/sui/live.ts`.
//!
//! Polled on a calm schedule ([`ACTIVE`], [`IDLE`], nothing while away) and paused while a game
//! has the screen. The host runs [`SuiLive::tick`] from its worker pool; the UI reads
//! [`SuiLive::snapshot`] when [`SuiLive::version`] moves.
//!
//! Portfolio change: rare.market values collectibles but keeps no per-wallet history, so the
//! launcher keeps a small series of USD snapshots per address on this device and compares against
//! the one nearest 24 hours ago (or the oldest it has, labelled with its time).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::grpc::ChainRead;
use crate::market::{self, Launch, MarketApi};
use crate::profile::{Current, Persist, ProfileStore};

/// Every minute in use, every 5 minutes idle, none away.
pub const ACTIVE: Duration = Duration::from_secs(60);
pub const IDLE: Duration = Duration::from_secs(5 * 60);
const DAY: f64 = 86_400_000.0;
const MINTS_EVERY: f64 = 5.0 * 60_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub t: f64,
    pub usd: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub usd: f64,
    pub pct: Option<f64>,
    pub since: f64,
}

/// Adds a snapshot (at most one per 10 minutes) and drops those older than two days.
pub fn add_snapshot(list: &[Snapshot], s: Snapshot) -> Vec<Snapshot> {
    let mut kept: Vec<Snapshot> = list.iter().copied().filter(|x| s.t - x.t < 2.0 * DAY).collect();
    if kept.last().is_some_and(|l| s.t - l.t < 10.0 * 60_000.0) {
        return kept;
    }
    kept.push(s);
    kept
}

/// Change against the snapshot nearest 24 h ago (or the oldest when none is that old). Needs 30
/// minutes of history.
pub fn change_since(list: &[Snapshot], now: Snapshot) -> Option<Change> {
    let target = now.t - DAY;
    let base = list.iter().filter(|x| now.t - x.t >= 30.0 * 60_000.0).fold(None::<&Snapshot>, |best, x| match best {
        Some(b) if (x.t - target).abs() >= (b.t - target).abs() => Some(b),
        _ => Some(x),
    })?;
    let usd = now.usd - base.usd;
    Some(Change { usd, pct: (base.usd > 0.0).then(|| usd / base.usd * 100.0), since: base.t })
}

fn snap_key(a: &str) -> String {
    format!("rare.sui.snaps.{a}")
}

fn read_snaps(p: &dyn Persist, a: &str) -> Vec<Snapshot> {
    p.get(&snap_key(a)).and_then(|v| serde_json::from_str::<Vec<serde_json::Value>>(&v).ok()).map(|v| v.into_iter().filter_map(|x| serde_json::from_value(x).ok()).collect()).unwrap_or_default()
}

/// Total value in USD: coins (the shell's the Trade API portfolio when it has this account, else SUI
/// alone) plus collectibles at rare.market's value.
pub fn total_usd(sui_mist: Option<u64>, nft_mist: Option<u128>, sui_usd: Option<f64>, coins_usd: Option<f64>) -> Option<f64> {
    let price = sui_usd.filter(|p| *p != 0.0)?;
    // Collectibles alone are not a total: before the coins are read there is no value, never a
    // $0.00 that changes a moment later.
    if sui_mist.is_none() && coins_usd.is_none() {
        return None;
    }
    let coins = coins_usd.unwrap_or_else(|| sui_mist.unwrap_or(0) as f64 / 1e9 * price);
    Some(coins + nft_mist.unwrap_or(0) as f64 / 1e9 * price)
}

/// One tile of the "minting now" row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mint {
    pub key: String,
    pub name: String,
    pub image_url: Option<String>,
    pub sub: String,
    pub href: String,
}

/// `toLocaleString("en-US")` for a whole count.
pub fn thousands(n: f64) -> String {
    let n = n.round() as i64;
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

pub fn launch_mint(l: &Launch) -> Mint {
    Mint {
        key: format!("launch:{}", l.slug),
        name: l.name.clone(),
        image_url: l.image_url.clone(),
        sub: match l.supply.filter(|s| *s != 0.0) {
            Some(s) => format!("{} of {} minted", thousands(l.minted.unwrap_or(0.0)), thousands(s)),
            None => "Minting now".into(),
        },
        href: format!("https://rare.market/launchpad/{}", l.slug),
    }
}

/// Live launches first, then up to six recent mints, eight tiles at most.
pub fn merge_mints(launches: &[Launch], drops: &[Launch], events: &[market::MintEvent], now_ms: f64) -> Vec<Mint> {
    let all: Vec<Launch> = launches.iter().chain(drops).cloned().collect();
    let mut out: Vec<Mint> = market::live_launches(&all, now_ms).iter().map(launch_mint).collect();
    out.extend(events.iter().filter_map(|e| e.item.as_ref().map(|i| (e, i))).take(6).map(|(e, i)| Mint {
        key: format!("mint:{}", e.id),
        name: i.name.clone(),
        image_url: i.image_url.clone(),
        sub: e.collection.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| "New mint".into()),
        href: format!("https://rare.market/item/{}", i.id),
    }));
    out.truncate(8);
    out
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveState {
    pub sui_usd: Option<f64>,
    pub tps: Option<f64>,
    pub gas_mist: Option<u64>,
    pub degraded: bool,
    pub portfolio_usd: Option<f64>,
    pub change: Option<Change>,
    pub mints: Option<Vec<Mint>>,
}

/// What a tick reads with.
pub struct LiveDeps<'a> {
    pub api: &'a dyn MarketApi,
    pub chain: &'a dyn ChainRead,
    pub persist: &'a dyn Persist,
    pub profile: &'a ProfileStore,
    /// The wallet's current account (None: signed out).
    pub current: Option<Current>,
    /// The shell's the Trade API portfolio total for this account, when it has one (`coinsUsdFor`).
    pub coins_usd: Option<f64>,
    /// A game has the screen: the launcher must not consume then.
    pub game_running: bool,
}

/// The Home strip's live store (`useSuiLive`).
#[derive(Default)]
pub struct SuiLive {
    state: Mutex<LiveState>,
    last_mints: Mutex<f64>,
    version: AtomicU64,
}

impl SuiLive {
    pub fn snapshot(&self) -> LiveState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Bumps on every change: the UI rebuilds when it moves.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    fn set(&self, f: impl FnOnce(&mut LiveState)) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let before = s.clone();
        f(&mut s);
        if *s != before {
            self.version.fetch_add(1, Ordering::AcqRel);
        }
    }

    async fn status(&self, d: &LiveDeps<'_>) {
        if let Ok(s) = market::status(d.api).await {
            self.set(|st| {
                st.sui_usd = Some(s.sui_usd);
                st.tps = Some(s.tps);
                st.degraded = s.degraded;
            });
        }
    }

    async fn gas(&self, d: &LiveDeps<'_>) {
        if let Ok(g) = d.chain.reference_gas_price().await {
            self.set(|st| st.gas_mist = Some(g));
        }
    }

    async fn portfolio(&self, d: &LiveDeps<'_>, now_ms: f64) {
        d.profile.refresh(d.chain, d.api, d.persist, d.current.clone(), false, now_ms).await;
        let p = d.profile.snapshot();
        let price = self.snapshot().sui_usd.or(p.sui_usd);
        let (Some(address), Some(usd)) = (p.address.clone(), total_usd(p.sui_mist, p.nft_mist, price, d.coins_usd)) else {
            self.set(|st| {
                st.portfolio_usd = None;
                st.change = None;
            });
            return;
        };
        let now = Snapshot { t: now_ms, usd };
        let list = read_snaps(d.persist, &address);
        let change = change_since(&list, now);
        d.persist.put(&snap_key(&address), serde_json::to_string(&add_snapshot(&list, now)).unwrap_or_default());
        self.set(|st| {
            st.portfolio_usd = Some(usd);
            st.change = change;
        });
    }

    async fn mints(&self, d: &LiveDeps<'_>, now_ms: f64) {
        let (launches, drops, events) = futures::join!(market::launches(d.api), market::drops(d.api), market::mints(d.api));
        let m = merge_mints(&launches.unwrap_or_default(), &drops.map(|p| p.data).unwrap_or_default(), &events.map(|p| p.data).unwrap_or_default(), now_ms);
        self.set(|st| st.mints = Some(m));
    }

    /// One poll: status (TPS), gas and the portfolio; every 5 minutes (or `all`) the mints.
    /// Nothing while a game has the screen.
    pub async fn tick(&self, d: &LiveDeps<'_>, all: bool, now_ms: f64) {
        if d.game_running {
            return;
        }
        let want_mints = {
            let mut last = self.last_mints.lock().unwrap_or_else(|e| e.into_inner());
            let due = all || now_ms - *last >= MINTS_EVERY - 2_000.0;
            if due {
                *last = now_ms;
            }
            due
        };
        let mints = async {
            if want_mints {
                self.mints(d, now_ms).await;
            }
        };
        futures::join!(self.status(d), self.gas(d), self.portfolio(d, now_ms), mints);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::CoinBalance;
    use crate::profile::MemPersist;
    use crate::BoxFut;
    use serde_json::{json, Value};

    const H: f64 = 3_600_000.0;
    fn s(t: f64, usd: f64) -> Snapshot {
        Snapshot { t, usd }
    }

    // live.test.ts "portfolio snapshots"
    #[test]
    fn keeps_one_snapshot_per_10_minutes_and_two_days_at_most() {
        let mut list = add_snapshot(&[], s(0.0, 1.0));
        list = add_snapshot(&list, s(5.0 * 60_000.0, 2.0));
        assert_eq!(list.len(), 1);
        list = add_snapshot(&list, s(11.0 * 60_000.0, 3.0));
        assert_eq!(list.len(), 2);
        list = add_snapshot(&list, s(49.0 * H, 4.0));
        assert_eq!(list.iter().map(|x| x.usd).collect::<Vec<_>>(), [4.0]);
    }

    #[test]
    fn compares_against_the_snapshot_nearest_24h_ago() {
        let list = [s(0.0, 100.0), s(23.0 * H, 80.0), s(40.0 * H, 90.0)];
        let c = change_since(&list, s(47.0 * H, 120.0)).unwrap();
        assert_eq!(c.since, 23.0 * H);
        assert_eq!(c.usd, 40.0);
        assert_eq!(c.pct, Some(50.0));
    }

    #[test]
    fn needs_half_an_hour_of_history() {
        assert_eq!(change_since(&[s(0.0, 1.0)], s(10.0 * 60_000.0, 2.0)), None);
    }

    #[test]
    fn totals_and_tiles() {
        assert_eq!(total_usd(Some(2_000_000_000), Some(1_000_000_000), Some(2.0), None), Some(6.0));
        assert_eq!(total_usd(Some(2_000_000_000), Some(1_000_000_000), Some(2.0), Some(10.0)), Some(12.0));
        assert_eq!(total_usd(None, None, Some(2.0), None), None);
        // collectibles read, coins not yet: no value rather than $0.00
        assert_eq!(total_usd(None, Some(0), Some(2.0), None), None);
        assert_eq!(total_usd(Some(1), None, None, None), None);
        assert_eq!(thousands(1234567.0), "1,234,567");
        assert_eq!(thousands(12.0), "12");
        let l = Launch { slug: "x".into(), name: "X".into(), minted: Some(1200.0), supply: Some(5000.0), ..Default::default() };
        assert_eq!(launch_mint(&l).sub, "1,200 of 5,000 minted");
        assert_eq!(launch_mint(&Launch { slug: "y".into(), ..Default::default() }).sub, "Minting now");
    }

    struct Api;
    impl MarketApi for Api {
        fn get(&self, p: &str) -> BoxFut<'_, Value> {
            let now = 1_800_000_000_000f64;
            let v = match p {
                "/v1/status" => json!({ "suiUsd": 2.0, "tps": 321, "checkpoint": 9, "degraded": false }),
                "/launchpad/launches.json" => json!([{ "slug": "a", "name": "A", "phases": [{ "startsAt": now - 1.0, "endsAt": now + 1.0 }] }]),
                "/v1/drops" => json!({ "data": [], "nextCursor": null }),
                "/v1/events?kind=mint&limit=12" => json!({ "data": [{ "id": "e1", "item": { "id": "0xi", "name": "I", "imageUrl": null }, "collection": null, "at": "" }, { "id": "e2", "item": null, "at": "" }] }),
                p if p.ends_with("/portfolio?limit=1") => json!({ "valueMist": "1000000000" }),
                _ => json!({ "address": "0x1" }),
            };
            Box::pin(async move { Ok(v) })
        }
    }
    struct Chain;
    impl ChainRead for Chain {
        fn reference_gas_price(&self) -> BoxFut<'_, u64> {
            Box::pin(async { Ok(750) })
        }
        fn balance(&self, _o: &str, t: &str) -> BoxFut<'_, CoinBalance> {
            let t = t.to_string();
            Box::pin(async move { Ok(CoinBalance { coin_type: t, balance: 2_000_000_000, ..Default::default() }) })
        }
        fn default_name(&self, _a: &str) -> BoxFut<'_, Option<String>> {
            Box::pin(async { Ok(None) })
        }
    }

    #[tokio::test]
    async fn tick_fills_the_strip_and_keeps_snapshots() {
        let live = SuiLive::default();
        let (persist, profile) = (MemPersist::default(), ProfileStore::default());
        let mut d = LiveDeps { api: &Api, chain: &Chain, persist: &persist, profile: &profile, current: Some(Current { address: "0x1".into(), label: None }), coins_usd: None, game_running: true };
        live.tick(&d, true, 1_800_000_000_000.0).await;
        assert_eq!(live.version(), 0, "nothing while a game has the screen");
        d.game_running = false;
        live.tick(&d, true, 1_800_000_000_000.0).await;
        let st = live.snapshot();
        assert_eq!((st.sui_usd, st.tps, st.gas_mist), (Some(2.0), Some(321.0), Some(750)));
        let mints = st.mints.unwrap();
        assert_eq!(mints.iter().map(|m| m.key.as_str()).collect::<Vec<_>>(), ["launch:a", "mint:e1"]);
        assert_eq!(mints[1].sub, "New mint");
        // 2 SUI + 1 SUI of collectibles at $2
        assert_eq!(st.portfolio_usd, Some(6.0));
        assert_eq!(st.change, None);
        assert_eq!(read_snaps(&persist, "0x1").len(), 1);
        assert!(live.version() > 0);
    }
}
