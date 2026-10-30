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
}
