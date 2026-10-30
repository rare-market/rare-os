//! Time helpers the ported code needs from JavaScript: `Date.parse` for the ISO timestamps the
//! APIs send, and the current time in ms.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // Howard Hinnant's algorithm: days since 1970-01-01.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date (y, m, d) of a day count since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn num(s: &str) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `Date.parse` for ISO 8601 strings: `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM[:SS[.sss]]` with `Z`, an
/// offset, or none. Date-only forms are UTC; date-time forms without an offset are read as UTC too
/// (the APIs always send `Z`; the launcher runs in UTC on the device). None when unparsable.
pub fn parse_iso_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, time) = match s.find(['T', 't', ' ']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let mut dp = date.splitn(3, '-');
    let y = num(dp.next()?)?;
    let m = dp.next().map(num).unwrap_or(Some(1))?;
    let d = dp.next().map(num).unwrap_or(Some(1))?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut ms = days_from_civil(y, m, d) * 86_400_000;
    let Some(t) = time else { return Some(ms) };
    let (clock, offset_min) = if let Some(c) = t.strip_suffix(['Z', 'z']) {
        (c, 0)
    } else if let Some(i) = t.rfind(['+', '-']) {
        let (c, off) = t.split_at(i);
        let sign = if off.starts_with('-') { -1 } else { 1 };
        let off = off[1..].replace(':', "");
        if off.len() != 4 {
            return None;
        }
        (c, sign * (num(&off[..2])? * 60 + num(&off[2..])?))
    } else {
        (t, 0)
    };
    let mut cp = clock.splitn(3, ':');
    let h = num(cp.next()?)?;
    let mi = num(cp.next()?)?;
    let (sec, frac) = match cp.next() {
        Some(sx) => match sx.split_once('.') {
            Some((a, b)) => (num(a)?, {
                let digits: String = b.chars().take(3).collect();
                let v = num(&digits)?;
                v * 10_i64.pow(3 - digits.len() as u32)
            }),
            None => (num(sx)?, 0),
        },
        None => (0, 0),
    };
    if h > 24 || mi > 59 || sec > 59 {
        return None;
    }
    ms += ((h * 60 + mi) * 60 + sec) * 1000 + frac - offset_min * 60_000;
    Some(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_like_date_parse() {
        assert_eq!(parse_iso_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso_ms("2026-10-25T22:55:39.131Z"), Some(1_792_968_939_131));
        assert_eq!(parse_iso_ms("2026-10-25T22:55:39.131+02:00"), Some(1_792_968_939_131 - 7_200_000));
        assert_eq!(parse_iso_ms("2026-10-25"), Some(1_792_886_400_000));
        assert_eq!(parse_iso_ms("nope"), None);
        assert_eq!(civil_from_days(20_751), (2026, 10, 25));
    }
}
