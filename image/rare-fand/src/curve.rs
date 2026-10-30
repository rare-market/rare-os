//! Fan presets and curve maths. Curves map the GPU edge temperature (C) to a fan duty (%).

/// The launcher's preset points (same as Handheld Daemon's edge curve points).
pub const POINTS: [u32; 9] = [40, 45, 50, 55, 60, 65, 70, 80, 90];
pub const COOL: [(u32, u32); 9] = [(40, 30), (45, 36), (50, 46), (55, 56), (60, 66), (65, 76), (70, 86), (80, 100), (90, 100)];
pub const QUIET: [(u32, u32); 9] = [(40, 20), (45, 20), (50, 20), (55, 22), (60, 26), (65, 30), (70, 50), (80, 80), (90, 100)];

/// Limits for a custom curve.
pub const MAX_POINTS: usize = 16;
pub const MIN_TEMP: u32 = 20;
pub const MAX_TEMP: u32 = 100;

#[derive(Clone, Debug, PartialEq)]
pub enum Preset {
    /// The embedded controller's own curve.
    Default,
    Cool,
    Quiet,
    Custom(Vec<(u32, u32)>),
}

impl Preset {
    pub fn name(&self) -> &'static str {
        match self {
            Preset::Default => "default",
            Preset::Cool => "cool",
            Preset::Quiet => "quiet",
            Preset::Custom(_) => "custom",
        }
    }

    /// The named presets ("custom" needs points, see `Preset::Custom`).
    pub fn parse(name: &str) -> Option<Preset> {
        match name.trim().to_ascii_lowercase().as_str() {
            "default" => Some(Preset::Default),
            "cool" => Some(Preset::Cool),
            "quiet" => Some(Preset::Quiet),
            _ => None,
        }
    }

    /// None for Default: the EC drives the fan.
    pub fn curve(&self) -> Option<&[(u32, u32)]> {
        match self {
            Preset::Default => None,
            Preset::Cool => Some(&COOL),
            Preset::Quiet => Some(&QUIET),
            Preset::Custom(c) => Some(c),
        }
    }
}

/// A custom curve: 2 to 16 points, temperatures rising strictly within 20..=100 C, duty
/// 0..=100 % and never falling as the temperature rises.
pub fn validate(points: &[(u32, u32)]) -> Result<(), String> {
    if points.len() < 2 || points.len() > MAX_POINTS {
        return Err(format!("a curve needs 2 to {MAX_POINTS} points"));
    }
    for &(t, d) in points {
        if !(MIN_TEMP..=MAX_TEMP).contains(&t) {
            return Err(format!("temperature {t} C is outside {MIN_TEMP}..={MAX_TEMP}"));
        }
        if d > 100 {
            return Err(format!("duty {d}% is above 100"));
        }
    }
    for w in points.windows(2) {
        if w[1].0 <= w[0].0 {
            return Err("temperatures must rise from point to point".into());
        }
        if w[1].1 < w[0].1 {
            return Err("fan duty must not fall as the temperature rises".into());
        }
    }
    Ok(())
}

/// Duty for a temperature: flat below the first and above the last point, linear in between,
/// rounded to the nearest percent.
pub fn interpolate(curve: &[(u32, u32)], temp: f64) -> u32 {
    let Some(&(t0, d0)) = curve.first() else { return 100 };
    if !temp.is_finite() {
        return 100;
    }
    if temp <= t0 as f64 {
        return d0;
    }
    for w in curve.windows(2) {
        let ((ta, da), (tb, db)) = (w[0], w[1]);
        if temp <= tb as f64 {
            let f = (temp - ta as f64) / (tb as f64 - ta as f64);
            let d = da as f64 + f * (db as f64 - da as f64);
            return d.round().clamp(0.0, 100.0) as u32;
        }
    }
    curve.last().map_or(100, |p| p.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_match_the_launcher() {
        assert_eq!(COOL.map(|p| p.0), POINTS);
        assert_eq!(QUIET.map(|p| p.0), POINTS);
        validate(&COOL).unwrap();
        validate(&QUIET).unwrap();
        assert_eq!(Preset::parse("Cool"), Some(Preset::Cool));
        assert_eq!(Preset::parse("quiet"), Some(Preset::Quiet));
        assert_eq!(Preset::parse("default"), Some(Preset::Default));
        assert_eq!(Preset::parse("custom"), None);
        assert_eq!(Preset::Default.curve(), None);
    }

    #[test]
    fn interpolates() {
        // On the points.
        for (t, d) in COOL {
            assert_eq!(interpolate(&COOL, t as f64), d);
        }
        for (t, d) in QUIET {
            assert_eq!(interpolate(&QUIET, t as f64), d);
        }
        // Flat outside.
        assert_eq!(interpolate(&COOL, 20.0), 30);
        assert_eq!(interpolate(&QUIET, 105.0), 100);
        // Between points: Cool 40->45 C is 30->36 %.
        assert_eq!(interpolate(&COOL, 42.5), 33);
        assert_eq!(interpolate(&COOL, 41.0), 31); // 31.2
        // Quiet 70->80 C is 50->80 %, 75 C is 65 %.
        assert_eq!(interpolate(&QUIET, 75.0), 65);
        assert_eq!(interpolate(&QUIET, 72.4), 57); // 57.2
        // Fractional temperatures and a sensor glitch.
        assert_eq!(interpolate(&COOL, 47.5), 41);
        assert_eq!(interpolate(&COOL, f64::NAN), 100);
        // Never falls as the temperature rises.
        let mut last = 0;
        for i in 0..1000 {
            let d = interpolate(&QUIET, 30.0 + i as f64 * 0.07);
            assert!(d >= last);
            last = d;
        }
    }

    #[test]
    fn validates_custom_curves() {
        assert!(validate(&[(40, 30), (80, 100)]).is_ok());
        assert!(validate(&[(40, 30)]).is_err());
        assert!(validate(&[(40, 30), (40, 50)]).is_err());
        assert!(validate(&[(50, 30), (40, 50)]).is_err());
        assert!(validate(&[(40, 60), (50, 50)]).is_err());
        assert!(validate(&[(40, 30), (80, 101)]).is_err());
        assert!(validate(&[(10, 30), (80, 100)]).is_err());
        assert!(validate(&[(40, 30), (120, 100)]).is_err());
        let many: Vec<_> = (0..17).map(|i| (20 + i * 4, i * 5)).collect();
        assert!(validate(&many).is_err());
        assert!(validate(&many[..16]).is_ok());
    }
}
