//! Small numeric helpers that keep big library code out of the 486 build:
//! decimal text in and out without std's general float formatting and
//! parsing (~24 KB), and `ln`/`exp`/`sin`/`cos` without musl's libm
//! (~17 KB). Each covers only what phonolite needs, accurately: plain
//! decimals (no exponents, at most 18 digits), and the elementary
//! functions on the modest ranges the equalizer and visualizer use.

use std::f64::consts::{FRAC_PI_2, LN_2};

/// Parse a plain decimal: optional sign, digits, optional point and
/// digits ("-3.5", "2.02", "50.000000", "7."). Digits past the 18th are
/// ignored. Correctly rounded: an integer below 2^53 divided by an exact
/// power of ten.
pub fn parse_decimal(s: &str) -> Option<f64> {
    let s = s.trim();
    let (neg, body) = match s.as_bytes().first()? {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    if int.is_empty() && frac.is_empty() || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut mantissa: u64 = 0;
    let mut digits = 0;
    let mut scale = 0;
    for (b, is_frac) in int.bytes().map(|b| (b, false)).chain(frac.bytes().map(|b| (b, true))) {
        if digits == 18 {
            if is_frac {
                break;
            }
            return None; // an integer part this long is not a decimal we write or read
        }
        if mantissa > 0 || b != b'0' {
            digits += 1;
        }
        mantissa = mantissa * 10 + u64::from(b - b'0');
        if is_frac {
            scale += 1;
        }
    }
    let v = mantissa as f64 / POW10[scale];
    Some(if neg { -v } else { v })
}

const POW10: [f64; 19] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18];

/// `v` with exactly `decimals` digits after the point (rounded half away
/// from zero), like `format!("{v:.N}")` for the values phonolite writes.
pub fn fixed(v: f64, decimals: usize) -> String {
    let decimals = decimals.min(9);
    let scale = POW10[decimals] as u64;
    let n = (v.abs() * scale as f64 + 0.5) as u64;
    let sign = if v < 0.0 && n != 0 { "-" } else { "" };
    if decimals == 0 {
        return format!("{sign}{n}");
    }
    format!("{sign}{}.{:0width$}", n / scale, n % scale, width = decimals)
}

/// Natural logarithm of `x > 0` (NaN otherwise).
pub fn ln(x: f64) -> f64 {
    if x.is_nan() || x <= 0.0 {
        return f64::NAN;
    }
    if x.is_infinite() {
        return x;
    }
    // x = m · 2^e with m in [√½, √2); then ln m = 2 atanh((m-1)/(m+1)),
    // whose series converges fast for |s| < 0.172.
    let (mut m, mut e) = frexp(x);
    if m > std::f64::consts::SQRT_2 {
        m /= 2.0;
        e += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let mut term = s;
    let mut sum = 0.0;
    for k in 0..12 {
        sum += term / f64::from(2 * k + 1);
        term *= s2;
    }
    f64::from(e) * LN_2 + 2.0 * sum
}

/// `x = m · 2^e`, `m` in [1, 2).
fn frexp(x: f64) -> (f64, i32) {
    let mut bits = x.to_bits();
    let mut e = ((bits >> 52) & 0x7ff) as i32;
    if e == 0 {
        // Subnormal: normalize first.
        let y = x * (1u64 << 54) as f64;
        bits = y.to_bits();
        e = ((bits >> 52) & 0x7ff) as i32 - 54;
    }
    let m = f64::from_bits((bits & !(0x7ff << 52)) | (1023 << 52));
    (m, e - 1023)
}

/// `e^x`.
pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x > 709.0 {
        return f64::INFINITY;
    }
    if x < -745.0 {
        return 0.0;
    }
    // x = k ln 2 + r, |r| ≤ ½ ln 2; e^r by its series, then scale by 2^k.
    let k = (x / LN_2 + if x < 0.0 { -0.5 } else { 0.5 }) as i32;
    let r = x - f64::from(k) * LN_2;
    let mut term = 1.0;
    let mut sum = 1.0;
    for n in 1..18 {
        term *= r / f64::from(n);
        sum += term;
    }
    // Two steps so 2^k stays representable at the extremes.
    let half = k / 2;
    sum * pow2(half) * pow2(k - half)
}

fn pow2(k: i32) -> f64 {
    f64::from_bits(((k + 1023).clamp(1, 2046) as u64) << 52)
}

/// `base^e` for `base > 0`.
pub fn pow(base: f64, e: f64) -> f64 {
    exp(e * ln(base))
}

/// Base-10 logarithm.
pub fn log10(x: f64) -> f64 {
    ln(x) / std::f64::consts::LN_10
}

/// Sine and cosine of `x`, for |x| up to a few turns (range reduction is
/// plain, not exact for huge arguments — nothing here needs those).
pub fn sin_cos(x: f64) -> (f64, f64) {
    let q = (x / FRAC_PI_2 + if x < 0.0 { -0.5 } else { 0.5 }) as i64;
    let r = x - q as f64 * FRAC_PI_2; // |r| ≤ π/4
    let r2 = r * r;
    let (mut s, mut c) = (0.0, 0.0);
    let (mut ts, mut tc) = (r, 1.0);
    for n in 0..11 {
        s += ts;
        c += tc;
        ts *= -r2 / f64::from((2 * n + 2) * (2 * n + 3));
        tc *= -r2 / f64::from((2 * n + 1) * (2 * n + 2));
    }
    match q.rem_euclid(4) {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_parse_exactly() {
        for (s, v) in
            [("2.02", 2.02), ("-3.5", -3.5), ("50.000000", 50.0), ("0", 0.0), ("7.", 7.0), (".25", 0.25), ("+1", 1.0), (" 12.346 ", 12.346), ("26.12", 26.12)]
        {
            assert_eq!(parse_decimal(s), Some(v), "{s}");
        }
        // Same answer as std's correctly rounded parser, digit for digit.
        for s in ["0.1", "123.456789", "0.000001", "99999.99999", "1.0000000000000002", "3.14159265358979"] {
            assert_eq!(parse_decimal(s), s.parse::<f64>().ok(), "{s}");
        }
        for s in ["", "-", ".", "1e5", "1.2.3", "abc", "1,5", "--1", "nan", "inf", "1234567890123456789012"] {
            assert_eq!(parse_decimal(s), None, "{s:?}");
        }
    }

    #[test]
    fn fixed_decimals() {
        for (v, d, want) in [
            (12.3456, 3, "12.346"),
            (55.56, 1, "55.6"),
            (1.5, 4, "1.5000"),
            (0.0, 3, "0.000"),
            (-3.5, 2, "-3.50"),
            (100.0, 1, "100.0"),
            (0.0004, 3, "0.000"),
            (-0.0004, 3, "0.000"),
            (2.0, 0, "2"),
            (1.9952623149688795, 4, "1.9953"),
            // Ties round away from zero (std's format rounds to even here).
            (0.125, 2, "0.13"),
        ] {
            assert_eq!(fixed(v, d), want, "{v} to {d}");
        }
    }

    #[test]
    fn elementary_functions_match_std() {
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * b.abs().max(1.0);
        for x in [1e-300, 1e-9, 0.001, 0.5, std::f64::consts::FRAC_1_SQRT_2, 1.0, std::f64::consts::SQRT_2, 1.5, 2.0, 10.0, 689.0625, 16000.0, 1e9, 1e300] {
            assert!(close(ln(x), x.ln()), "ln {x}: {} vs {}", ln(x), x.ln());
            assert!(close(log10(x), x.log10()), "log10 {x}");
        }
        assert!(ln(0.0).is_nan() && ln(-1.0).is_nan());
        for x in [-700.0, -20.0, -1.0, -0.3, 0.0, 0.3, 1.0, std::f64::consts::LN_10, 20.0, 700.0] {
            assert!(close(exp(x), x.exp()), "exp {x}: {} vs {}", exp(x), x.exp());
        }
        assert_eq!(exp(800.0), f64::INFINITY);
        assert!(close(pow(10.0, 6.0 / 20.0), 1.9952623149688795));
        assert!(close(pow(400.0, 0.5), 20.0));
        for i in -40..=40 {
            let x = f64::from(i) * 0.17;
            let (s, c) = sin_cos(x);
            assert!((s - x.sin()).abs() < 1e-14 && (c - x.cos()).abs() < 1e-14, "{x}");
        }
    }
}
