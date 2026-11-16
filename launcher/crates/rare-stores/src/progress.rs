//! Progress from legendary's and gogdl's logs.
//!
//! legendary (downloader/mp/manager.py):
//!   `[DLManager] INFO: = Progress: 12.34% (123/456), Running for 00:00:03, ETA: 00:10:25`
//!   `[DLManager] INFO:  - Downloaded: 6.54 MiB, Written: 6.42 MiB`
//!   `[DLManager] INFO:  + Download	- 2.05 MiB/s (raw) / 4.13 MiB/s (decompressed)`
//!   `[cli] INFO: Download size: 1234.56 MiB (Compression savings: 40.1%)`
//!   `[cli] INFO: Verification progress: 12/345 (3.5%) [12.3 MiB/s]`
//! gogdl (dl/progressbar.py), sizes in bytes:
//!   `[PROGRESS] INFO: = Progress: 12.34 123456/789012, Running for: 00:00:05, ETA: 00:01:00`
//!   `[PROGRESS] INFO:  + Download	- 2.05 MiB/s (raw) / 4.13 MiB/s (decompressed)`

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Update {
    /// 0..100
    pub pct: Option<f32>,
    pub done: Option<u64>,
    pub total: Option<u64>,
    /// Bytes a second.
    pub speed: Option<f64>,
    pub eta: Option<u64>,
    /// Checking files (repair) rather than downloading.
    pub verifying: bool,
}

const MIB: f64 = 1024.0 * 1024.0;

fn num(s: &str) -> Option<f64> {
    s.trim().trim_end_matches(['%', ',']).parse::<f64>().ok()
}

fn hms(s: &str) -> Option<u64> {
    let p: Vec<u64> = s.trim().trim_end_matches(',').split(':').filter_map(|x| x.parse().ok()).collect();
    (p.len() == 3).then(|| p[0] * 3600 + p[1] * 60 + p[2])
}

/// An Epic download carried on after a pause or a restart. legendary skips what it already
/// fetched and counts only the rest: "Download size" is what is left, and its percent runs over
/// that, so the bar started again at 0 % for a game that was half done. `Carry` puts the tool's
/// numbers back onto the whole download, with what the earlier runs fetched in front.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Carry {
    /// The whole download, as the job knew it before this run (0: nothing to carry).
    whole: u64,
    /// Fetched by the earlier runs.
    before: u64,
    /// What this run fetches (the tool's own total).
    rest: u64,
}

impl Carry {
    /// For a job that knew its size (`whole`) and had fetched some of it (`done`).
    pub fn new(whole: u64, done: u64) -> Carry {
        Carry { whole: if done > 0 { whole } else { 0 }, before: 0, rest: 0 }
    }

    pub fn map(&mut self, mut u: Update) -> Update {
        if self.whole == 0 || u.verifying {
            return u;
        }
        if let Some(t) = u.total {
            if t == 0 || t >= self.whole {
                // Not the rest of the same download (a new version, a bigger size): as it says.
                self.whole = 0;
                return u;
            }
            self.rest = t;
            self.before = self.whole - t;
            u.total = Some(self.whole);
        }
        if self.rest == 0 {
            return u;
        }
        if let Some(d) = u.done {
            u.done = Some(self.before + d.min(self.rest));
        }
        if let Some(p) = u.pct {
            let part = self.rest as f64 * p.clamp(0.0, 100.0) as f64 / 100.0;
            u.pct = Some(((self.before as f64 + part) / self.whole as f64 * 100.0) as f32);
        }
        u
    }
}

/// What one log line says, if anything.
pub fn parse(line: &str) -> Option<Update> {
    let l = line.trim();
    if let Some(i) = l.find("= Progress:") {
        let rest = &l[i + "= Progress:".len()..];
        let mut it = rest.split_whitespace();
        let pct = it.next().and_then(num).map(|v| v as f32);
        let mut u = Update { pct, ..Default::default() };
        if let Some(frac) = it.next() {
            let frac = frac.trim_matches(|c| c == '(' || c == ')' || c == ',');
            // gogdl: bytes written/total; legendary: chunks (not bytes).
            if !rest.contains('%') {
                if let Some((a, b)) = frac.split_once('/') {
                    u.done = a.parse().ok();
                    u.total = b.parse().ok();
                }
            }
        }
        if let Some(j) = rest.find("ETA:") {
            u.eta = hms(&rest[j + 4..]);
        }
        return Some(u);
    }
    if let Some(i) = l.find("- Downloaded:") {
        let v = l[i + "- Downloaded:".len()..].split_whitespace().next().and_then(num)?;
        return Some(Update { done: Some((v * MIB) as u64), ..Default::default() });
    }
    if let Some(i) = l.find("+ Download") {
        let rest = &l[i + "+ Download".len()..];
        let v = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '-').split_whitespace().next().and_then(num)?;
        return Some(Update { speed: Some(v * MIB), ..Default::default() });
    }
    if let Some(i) = l.find("Download size:") {
        let v = l[i + "Download size:".len()..].split_whitespace().next().and_then(num)?;
        return Some(Update { total: Some((v * MIB) as u64), ..Default::default() });
    }
    if let Some(i) = l.find("Verification progress:") {
        let rest = &l[i + "Verification progress:".len()..];
        let pct = rest.split('(').nth(1).and_then(|p| p.split('%').next()).and_then(num).map(|v| v as f32);
        return Some(Update { pct, verifying: true, ..Default::default() });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legendary_lines() {
        let u = parse("[DLManager] INFO: = Progress: 12.34% (123/456), Running for 00:00:03, ETA: 00:10:25").unwrap();
        assert_eq!((u.pct, u.done, u.eta), (Some(12.34), None, Some(625)));
        let u = parse("[DLManager] INFO:  - Downloaded: 2.00 MiB, Written: 1.00 MiB").unwrap();
        assert_eq!(u.done, Some(2 * 1024 * 1024));
        let u = parse("[DLManager] INFO:  + Download\t- 1.00 MiB/s (raw) / 2.00 MiB/s (decompressed)").unwrap();
        assert_eq!(u.speed, Some(MIB));
        let u = parse("[cli] INFO: Download size: 10.00 MiB (Compression savings: 40.1%)").unwrap();
        assert_eq!(u.total, Some(10 * 1024 * 1024));
        let u = parse("[cli] INFO: Verification progress: 12/345 (3.5%) [12.3 MiB/s]").unwrap();
        assert_eq!((u.pct, u.verifying), (Some(3.5), true));
        assert!(parse("[cli] INFO: Install size: 10.00 MiB").is_none());
    }

    #[test]
    fn gogdl_lines() {
        let u = parse("[PROGRESS] INFO: = Progress: 50.00 500/1000, Running for: 00:00:05, ETA: 00:01:00").unwrap();
        assert_eq!((u.pct, u.done, u.total, u.eta), (Some(50.0), Some(500), Some(1000), Some(60)));
    }

    #[test]
    fn a_resumed_download_carries_on_from_where_it_was() {
        // 1000 MiB in all, 600 fetched before the restart: legendary says 400 are left.
        let mut c = Carry::new(1000, 600);
        let t = c.map(Update { total: Some(400), ..Default::default() });
        assert_eq!(t.total, Some(1000));
        // Its own 50 % of the rest is 80 % of the whole.
        let p = c.map(Update { pct: Some(50.0), ..Default::default() });
        assert_eq!(p.pct, Some(80.0));
        let d = c.map(Update { done: Some(100), ..Default::default() });
        assert_eq!(d.done, Some(700));
        // A first run, or a size that is not the rest of it, is left as the tool says.
        let mut fresh = Carry::new(1000, 0);
        assert_eq!(fresh.map(Update { total: Some(400), ..Default::default() }).total, Some(400));
        let mut bigger = Carry::new(1000, 600);
        assert_eq!(bigger.map(Update { total: Some(1200), ..Default::default() }).total, Some(1200));
        assert_eq!(bigger.map(Update { pct: Some(10.0), ..Default::default() }).pct, Some(10.0));
        // Checking files keeps its own percent.
        let mut v = Carry::new(1000, 600);
        v.map(Update { total: Some(400), ..Default::default() });
        assert_eq!(v.map(Update { pct: Some(30.0), verifying: true, ..Default::default() }).pct, Some(30.0));
    }
}
