//! The 10-band equalizer, and how it maps onto mpg123's.
//!
//! mpg123's equalizer scales the decoder's 32 polyphase subbands — equal
//! slices of the spectrum, each `rate / 64` Hz wide (689 Hz at 44.1 kHz).
//! The classic 10 bands are spaced by octaves instead, so the mapping is
//! coarse at the bottom: 60, 170, 310 and 600 Hz all fall in the first
//! subband, which gets their average. Above that, each subband takes the
//! band whose center it contains, or is interpolated between the
//! neighboring bands on a log-frequency scale.

/// The band centers in Hz.
pub const FREQUENCIES: [f64; 10] = [60.0, 170.0, 310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0];

/// Gain range of a band and the preamp, ±dB.
pub const RANGE_DB: f64 = 12.0;

/// Equalizer settings, in dB.
#[derive(Debug, Clone, PartialEq)]
pub struct Equalizer {
    /// Applied at all (the EQ window's ON button).
    pub on: bool,
    /// Preamp gain.
    pub preamp: f64,
    /// Band gains, low to high.
    pub bands: [f64; 10],
}

impl Default for Equalizer {
    fn default() -> Equalizer {
        Equalizer { on: false, preamp: 0.0, bands: [0.0; 10] }
    }
}

fn db_to_factor(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

impl Equalizer {
    /// The gain in dB for each of mpg123's 32 subbands at `rate` Hz
    /// (band gains only; the preamp is separate).
    pub fn subband_db(&self, rate: u32) -> [f64; 32] {
        let width = f64::from(rate.max(1)) / 64.0;
        let mut out = [0.0; 32];
        for (i, slot) in out.iter_mut().enumerate() {
            let (lo, hi) = (i as f64 * width, (i + 1) as f64 * width);
            let inside: Vec<f64> = FREQUENCIES.iter().zip(self.bands).filter(|(f, _)| **f >= lo && **f < hi).map(|(_, g)| g).collect();
            *slot = if inside.is_empty() { self.interpolate((lo + hi) / 2.0) } else { inside.iter().sum::<f64>() / inside.len() as f64 };
        }
        out
    }

    fn interpolate(&self, f: f64) -> f64 {
        if f <= FREQUENCIES[0] {
            return self.bands[0];
        }
        for i in 1..10 {
            if f <= FREQUENCIES[i] {
                let t = (f.ln() - FREQUENCIES[i - 1].ln()) / (FREQUENCIES[i].ln() - FREQUENCIES[i - 1].ln());
                return self.bands[i - 1] + t * (self.bands[i] - self.bands[i - 1]);
            }
        }
        self.bands[9]
    }

    /// The linear factor for each channel (left, right) and subband: the
    /// equalizer (when on) times the balance. `balance` runs from -100
    /// (left only) to 100 (right only); the louder side stays at unity.
    pub fn factors(&self, balance: i32, rate: u32) -> [[f64; 32]; 2] {
        let b = f64::from(balance.clamp(-100, 100)) / 100.0;
        let side = [(1.0 - b).min(1.0), (1.0 + b).min(1.0)];
        let gains = if self.on { self.subband_db(rate).map(|g| db_to_factor(g + self.preamp)) } else { [1.0; 32] };
        side.map(|s| gains.map(|g| g * s))
    }
}

/// A named equalizer curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    /// Shown in the presets menu.
    pub name: &'static str,
    /// Preamp, dB.
    pub preamp: f64,
    /// Bands, dB.
    pub bands: [f64; 10],
}

/// Built-in curves.
pub const PRESETS: &[Preset] = &[
    Preset { name: "Flat", preamp: 0.0, bands: [0.0; 10] },
    Preset { name: "Bass boost", preamp: -4.0, bands: [7.0, 6.0, 4.5, 2.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0] },
    Preset { name: "Treble boost", preamp: -4.0, bands: [0.0, 0.0, 0.0, 0.0, 0.0, 1.5, 3.5, 6.0, 7.0, 7.0] },
    Preset { name: "Loudness", preamp: -4.0, bands: [6.0, 4.5, 2.0, 0.0, -1.0, 0.0, 1.5, 4.0, 5.0, 5.0] },
    Preset { name: "Voice", preamp: -2.0, bands: [-4.0, -3.0, -1.0, 1.5, 3.5, 3.5, 2.0, 0.0, -1.5, -3.0] },
    Preset { name: "Warm", preamp: -2.0, bands: [3.0, 3.0, 2.0, 1.0, 0.0, -1.0, -2.0, -3.0, -3.5, -4.0] },
    Preset { name: "Bright", preamp: -2.0, bands: [-3.0, -2.0, -1.0, 0.0, 0.5, 2.0, 3.5, 4.0, 4.0, 4.0] },
    Preset { name: "Small speakers", preamp: -3.0, bands: [-6.0, -3.0, 1.0, 3.0, 2.0, 1.0, 1.5, 2.5, 2.0, 1.0] },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn eq(bands: [f64; 10]) -> Equalizer {
        Equalizer { on: true, preamp: 0.0, bands }
    }

    #[test]
    fn low_bands_share_the_first_subband() {
        let e = eq([4.0, 8.0, 0.0, -4.0, 6.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let s = e.subband_db(44100);
        assert_eq!(s[0], 2.0, "average of 60/170/310/600");
        assert_eq!(s[1], 6.0, "1 kHz sits in 689..1378 Hz");
    }

    #[test]
    fn bands_land_in_their_subbands_and_interpolate_between() {
        let mut b = [0.0; 10];
        b[5] = 12.0; // 3 kHz
        let s = eq(b).subband_db(44100);
        assert_eq!(s[4], 12.0, "3 kHz is in 2756..3445 Hz");
        // 2411 Hz (subband 3) is between 1 kHz and 3 kHz, close to 3 kHz.
        let t = (2411.71875f64.ln() - 1000f64.ln()) / (3000f64.ln() - 1000f64.ln());
        assert!((s[3] - 12.0 * t).abs() < 1e-9);
        assert!(s[3] > 8.0 && s[3] < 12.0);
        assert!(s[2] < s[3], "rising toward the band");
        assert!(s[8] == 0.0, "6 kHz band is flat");
        // Beyond 16 kHz everything follows the top band.
        let mut b = [0.0; 10];
        b[9] = -6.0;
        let s = eq(b).subband_db(44100);
        assert!(s[24..].iter().all(|&g| g == -6.0));
        assert_eq!(s[23], -6.0, "16 kHz is in 15848..16537 Hz");
    }

    #[test]
    fn mapping_follows_the_rate() {
        let mut b = [0.0; 10];
        b[4] = 5.0; // 1 kHz
                    // At 32 kHz subbands are 500 Hz wide: 1 kHz starts subband 2.
        assert_eq!(eq(b).subband_db(32000)[2], 5.0);
        assert_eq!(eq(b).subband_db(44100)[1], 5.0);
    }

    #[test]
    fn factors_combine_preamp_bands_and_balance() {
        let mut e = eq([6.0; 10]);
        e.preamp = -6.0;
        let f = e.factors(0, 44100);
        assert!(f.iter().flatten().all(|&v| (v - 1.0).abs() < 1e-12));
        e.preamp = 0.0;
        let f = e.factors(0, 44100);
        assert!((f[0][5] - 1.9952623).abs() < 1e-6, "+6 dB");
        let f = e.factors(50, 44100);
        assert!((f[0][5] - 1.9952623 * 0.5).abs() < 1e-6, "right of center turns the left down");
        assert!((f[1][5] - 1.9952623).abs() < 1e-6);
        let f = Equalizer::default().factors(-100, 44100);
        assert_eq!((f[0][0], f[1][31]), (1.0, 0.0), "off: balance only");
        let f = Equalizer::default().factors(250, 44100);
        assert_eq!((f[0][0], f[1][0]), (0.0, 1.0), "clamped");
    }

    #[test]
    fn presets_stay_in_range() {
        for p in PRESETS {
            assert!(p.preamp.abs() <= RANGE_DB && p.bands.iter().all(|b| b.abs() <= RANGE_DB), "{}", p.name);
        }
        assert_eq!(PRESETS[0].bands, [0.0; 10]);
    }
}
