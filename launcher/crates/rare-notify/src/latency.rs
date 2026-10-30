//! Delivery latency per live notice, measured against the notice's checkpoint time (cpTs):
//!   server: sentAt - cpTs   (rare.market's indexer and stream, by the server clock)
//!   recv:   now - cpTs      when the frame reaches the launcher
//!   paint:  now - cpTs      in the frame after the toast first drew (none when no toast showed)
//! The last 50 are kept, oldest first; each record gives a log line with the running p50 and p95
//! for the host to print.

use crate::logic::{percentile, push_sample};
use crate::types::Notice;

#[derive(Clone, Debug, PartialEq)]
pub struct LatencySample {
    pub id: String,
    pub kind: String,
    pub cp_ts: i64,
    pub server_ms: Option<i64>,
    pub recv_ms: i64,
    pub paint_ms: Option<i64>,
    /// Server clock minus ours at hello, ms (positive: the server is ahead).
    pub skew_ms: Option<i64>,
}

const CAP: usize = 50;

#[derive(Clone, Debug, Default)]
pub struct Latency {
    samples: Vec<LatencySample>,
    skew: Option<i64>,
}

impl Latency {
    pub fn set_skew(&mut self, server_now: i64, now: i64) {
        self.skew = Some(server_now - now);
    }

    pub fn samples(&self) -> &[LatencySample] {
        &self.samples
    }

    /// A live notice arrived; the log line, or none when it has no checkpoint time or was seen.
    pub fn record_receipt(&mut self, n: &Notice, now: i64) -> Option<String> {
        let cp = n.cp_ts.filter(|t| *t != 0)?;
        if self.samples.iter().any(|s| s.id == n.id) {
            return None;
        }
        let s = LatencySample {
            id: n.id.clone(),
            kind: n.kind.as_str().to_string(),
            cp_ts: cp,
            server_ms: (n.sent_at != 0).then(|| n.sent_at - cp),
            recv_ms: now - cp,
            paint_ms: None,
            skew_ms: self.skew,
        };
        push_sample(&mut self.samples, s, CAP);
        Some(self.log(self.samples.len() - 1, "recv"))
    }

    /// Its toast drew; the log line, once per notice.
    pub fn record_paint(&mut self, id: &str, now: i64) -> Option<String> {
        let i = self.samples.iter().position(|s| s.id == id)?;
        if self.samples[i].paint_ms.is_some() {
            return None;
        }
        self.samples[i].paint_ms = Some(now - self.samples[i].cp_ts);
        Some(self.log(i, "paint"))
    }

    fn log(&self, i: usize, what: &str) -> String {
        let s = &self.samples[i];
        let dash = |v: Option<i64>| v.map_or("-".to_string(), |x| x.to_string());
        let recv: Vec<f64> = self.samples.iter().map(|x| x.recv_ms as f64).collect();
        let paint: Vec<f64> = self.samples.iter().filter_map(|x| x.paint_ms.map(|v| v as f64)).collect();
        let p = |v: &[f64]| {
            let q = |pc| percentile(v, pc).map_or("-".to_string(), |x| (x as i64).to_string());
            format!("p50 {} p95 {}", q(50.0), q(95.0))
        };
        let value = if what == "recv" { Some(s.recv_ms) } else { s.paint_ms };
        format!(
            "[notify] {} {} {what} {} ms (server {} ms, skew {} ms) | recv {} | paint {} | n={}",
            s.id,
            s.kind,
            dash(value),
            dash(s.server_ms),
            dash(self.skew),
            p(&recv),
            p(&paint),
            self.samples.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_receipt_and_paint_once() {
        let mut l = Latency::default();
        l.set_skew(1_000_050, 1_000_000);
        let n = Notice { id: "5".into(), cp_ts: Some(1000), sent_at: 1300, ..Default::default() };
        let line = l.record_receipt(&n, 1500).unwrap();
        assert!(line.starts_with("[notify] 5  recv 500 ms (server 300 ms, skew 50 ms) | recv p50 500 p95 500"), "{line}");
        assert!(l.record_receipt(&n, 1600).is_none());
        assert!(l.record_paint("5", 1700).unwrap().contains("paint 700 ms"));
        assert!(l.record_paint("5", 1800).is_none());
        assert!(l.record_receipt(&Notice { id: "6".into(), ..Default::default() }, 1).is_none());
    }
}
