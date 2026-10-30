//! Rare score: every unlocked trophy is worth points by its tier, and the tier comes from how many
//! players have it (the source's global unlock %). Points are the chain's (15, 30, 90, 300), so a
//! trophy pinned to chain later is worth the same. The level is a curve over the points.

/// 0 Bronze, 1 Silver, 2 Gold, 3 Platinum.
pub type Tier = u8;

pub const POINTS: [u32; 4] = [15, 30, 90, 300];

pub fn points(tier: Tier) -> u32 {
    POINTS[tier.min(3) as usize]
}

pub fn tier_name(t: Tier) -> &'static str {
    match t {
        0 => "Bronze",
        1 => "Silver",
        2 => "Gold",
        _ => "Platinum",
    }
}

/// The tier of an achievement by the share of players who have it: under 2 % Platinum, under
/// 10 % Gold, under 30 % Silver, else (or unknown) Bronze.
pub fn tier_for_rarity(pct: Option<f64>) -> Tier {
    match pct {
        Some(p) if p.is_finite() && p < 2.0 => 3,
        Some(p) if p.is_finite() && p < 10.0 => 2,
        Some(p) if p.is_finite() && p < 30.0 => 1,
        _ => 0,
    }
}

/// Points needed to reach `level` (level 1 is 0 points): 25 n (n - 1), so 50 for level 2, 150
/// for 3, 2,250 for 10, 9,500 for 20.
pub fn level_floor(level: u32) -> u64 {
    let n = level.max(1) as u64;
    25 * n * (n - 1)
}

/// The level for `points`, with the points into it and the points the level spans.
pub fn level(points: u64) -> (u32, u64, u64) {
    let mut n = ((1.0 + (1.0 + points as f64 * 4.0 / 25.0).sqrt()) / 2.0).floor().max(1.0) as u32;
    while level_floor(n + 1) <= points {
        n += 1;
    }
    while n > 1 && level_floor(n) > points {
        n -= 1;
    }
    let floor = level_floor(n);
    (n, points - floor, level_floor(n + 1) - floor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_from_rarity() {
        assert_eq!(tier_for_rarity(Some(1.2)), 3);
        assert_eq!(tier_for_rarity(Some(2.0)), 2);
        assert_eq!(tier_for_rarity(Some(9.9)), 2);
        assert_eq!(tier_for_rarity(Some(29.0)), 1);
        assert_eq!(tier_for_rarity(Some(64.0)), 0);
        assert_eq!(tier_for_rarity(None), 0);
        assert_eq!(points(3), 300);
    }

    #[test]
    fn levels() {
        assert_eq!(level(0), (1, 0, 50));
        assert_eq!(level(49), (1, 49, 50));
        assert_eq!(level(50), (2, 0, 100));
        assert_eq!(level(2250).0, 10);
        assert_eq!(level(2249).0, 9);
        assert_eq!(level(9500).0, 20);
        for p in [0u64, 1, 77, 150, 151, 9_999, 123_456, 10_000_000] {
            let (n, into, span) = level(p);
            assert!(level_floor(n) <= p && p < level_floor(n + 1), "{p}");
            assert!(into < span);
        }
    }
}
