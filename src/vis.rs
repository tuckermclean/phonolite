//! The visualizer: a spectrum analyzer (19 bars with falling peaks) and an
//! oscilloscope, drawn in a skin's `viscolor.txt` colors.

use protolith::canvas::Canvas;
use protolith::Rect;

/// Samples per analysis (at 44.1 kHz: 12 ms, 86 Hz per bin). Small on
/// purpose: a 486's FPU does this 30 times a second.
pub const FFT_SIZE: usize = 512;
/// Bars across the 76-pixel area: 3 pixels and a gap each.
pub const BARS: usize = 19;
/// Lowest and highest frequency shown.
const F_LO: f64 = 40.0;
const F_HI: f64 = 16000.0;
/// Bar height in pixels.
pub const HEIGHT: u32 = 16;
/// Range shown: this many dB below full scale is an empty bar.
const RANGE_DB: f32 = 54.0;

/// What the visualizer shows; clicking it cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Bars.
    Spectrum,
    /// Waveform.
    Scope,
    /// Nothing.
    Off,
}

impl Mode {
    /// The next mode in the cycle.
    pub fn next(self) -> Mode {
        match self {
            Mode::Spectrum => Mode::Scope,
            Mode::Scope => Mode::Off,
            Mode::Off => Mode::Spectrum,
        }
    }
}

/// In-place radix-2 FFT; `re.len()` must be a power of two.
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    assert!(n.is_power_of_two() && im.len() == n);
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (s, c) = crate::num::sin_cos(ang);
        let (wr, wi) = (c as f32, s as f32);
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let next = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = next;
            }
        }
        len <<= 1;
    }
}

/// Spectrum state: bar heights that fall smoothly, and peak dots that
/// hold a moment before dropping.
#[derive(Debug, Clone)]
pub struct Spectrum {
    window: Vec<f32>,
    /// Bar heights, 0.0..=16.0.
    pub bars: [f32; BARS],
    /// Peak heights.
    pub peaks: [f32; BARS],
    hold: [u8; BARS],
    edges: [usize; BARS + 1],
}

impl Default for Spectrum {
    fn default() -> Spectrum {
        Spectrum::new()
    }
}

impl Spectrum {
    /// Empty bars, for samples at the usual 44.1 kHz.
    pub fn new() -> Spectrum {
        Spectrum::with_rate(crate::audio::RATE)
    }

    /// Empty bars, for samples at `rate` Hz.
    pub fn with_rate(rate: u32) -> Spectrum {
        let window = (0..FFT_SIZE).map(|i| (0.5 - 0.5 * crate::num::sin_cos(2.0 * std::f64::consts::PI * i as f64 / FFT_SIZE as f64).1) as f32).collect();
        let bin = f64::from(rate.max(1)) / FFT_SIZE as f64;
        let hi = F_HI.min(f64::from(rate) / 2.0 * 0.95);
        let mut edges = [0; BARS + 1];
        for (k, e) in edges.iter_mut().enumerate() {
            let f = F_LO * crate::num::pow(hi / F_LO, k as f64 / BARS as f64);
            *e = ((f / bin).round() as usize).clamp(1, FFT_SIZE / 2);
        }
        // Every bar gets at least one bin of its own.
        for k in 1..=BARS {
            if edges[k] <= edges[k - 1] {
                edges[k] = edges[k - 1] + 1;
            }
        }
        Spectrum { window, bars: [0.0; BARS], peaks: [0.0; BARS], hold: [0; BARS], edges }
    }

    /// The FFT bins `lo..hi` bar `k` covers.
    pub fn bins(&self, k: usize) -> (usize, usize) {
        (self.edges[k], self.edges[k + 1])
    }

    /// The level each bar would jump to for these samples (no smoothing),
    /// in pixels. `samples` is `FFT_SIZE` mono samples.
    pub fn levels(&self, samples: &[i16]) -> [f32; BARS] {
        let mut re: Vec<f32> = samples.iter().zip(&self.window).map(|(&s, w)| f32::from(s) / 32768.0 * w).collect();
        re.resize(FFT_SIZE, 0.0);
        let mut im = vec![0.0; FFT_SIZE];
        fft(&mut re, &mut im);
        // A full-scale sine through a Hann window peaks at N/4.
        let norm = 4.0 / FFT_SIZE as f32;
        let mut out = [0.0; BARS];
        for (k, o) in out.iter_mut().enumerate() {
            let (lo, hi) = self.bins(k);
            let mag = (lo..hi).map(|b| (re[b] * re[b] + im[b] * im[b]).sqrt() * norm).fold(0.0f32, f32::max);
            let db = 20.0 * crate::num::log10(f64::from(mag.max(1e-9))) as f32;
            *o = ((db + RANGE_DB) / RANGE_DB * HEIGHT as f32).clamp(0.0, HEIGHT as f32);
        }
        out
    }

    /// Advance one frame (call ~30 times a second): bars rise at once and
    /// fall 1.5 px a frame; peaks hold 12 frames, then fall.
    pub fn update(&mut self, samples: Option<&[i16]>) {
        let levels = samples.map_or([0.0; BARS], |s| self.levels(s));
        for (k, &level) in levels.iter().enumerate() {
            self.bars[k] = level.max(self.bars[k] - 1.5);
            if self.bars[k] >= self.peaks[k] {
                self.peaks[k] = self.bars[k];
                self.hold[k] = 12;
            } else if self.hold[k] > 0 {
                self.hold[k] -= 1;
            } else {
                self.peaks[k] = (self.peaks[k] - 0.5).max(0.0);
            }
        }
    }

    /// All bars and peaks are down: nothing left to animate.
    pub fn is_idle(&self) -> bool {
        self.bars.iter().chain(&self.peaks).all(|&v| v < 0.5)
    }
}

/// Draw the background: color 0 with color 1 dots on every other pixel of
/// every other row, as classic skins expect.
pub fn draw_background(c: &mut Canvas, area: Rect, colors: &[u32; 24]) {
    c.fill_rect(area, colors[0]);
    for y in (area.y..area.y + area.height as i32).step_by(2) {
        for x in (area.x + 1..area.x + area.width as i32).step_by(2) {
            c.put(x, y, colors[1]);
        }
    }
}

/// Draw bars: row `y` of a bar (0 = top) takes color `2 + y`; peaks are
/// color 23.
pub fn draw_spectrum(c: &mut Canvas, area: Rect, colors: &[u32; 24], s: &Spectrum) {
    draw_background(c, area, colors);
    let h = area.height.min(HEIGHT) as i32;
    for k in 0..BARS {
        let x = area.x + 4 * k as i32;
        let bar = s.bars[k].round() as i32;
        for row in (h - bar).max(0)..h {
            c.fill_rect(Rect::new(x, area.y + row, 3, 1), colors[2 + row as usize]);
        }
        let peak = s.peaks[k].round() as i32;
        if peak > 0 {
            c.fill_rect(Rect::new(x, area.y + (h - peak).max(0), 3, 1), colors[23]);
        }
    }
}

/// Draw a waveform: one column per pixel, joined vertically, colored by
/// distance from the center line (colors 18–22).
pub fn draw_scope(c: &mut Canvas, area: Rect, colors: &[u32; 24], samples: &[i16]) {
    draw_background(c, area, colors);
    if samples.is_empty() {
        return;
    }
    let h = area.height as i32;
    let y_of = |s: i16| (h / 2 - 1 - i32::from(s) * (h / 2) / 32768).clamp(0, h - 1);
    let mut prev = None;
    for x in 0..area.width as i32 {
        let i = x as usize * samples.len() / area.width as usize;
        let y = y_of(samples[i]);
        let (a, b) = match prev {
            Some(p) if p < y => (p + 1, y),
            Some(p) if p > y => (y, p - 1),
            _ => (y, y),
        };
        for yy in a..=b {
            let d = ((yy - h / 2).abs() * 5 / (h / 2 + 1)).min(4) as usize;
            c.put(area.x + x, area.y + yy, colors[18 + d]);
        }
        prev = Some(y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::RATE;

    fn sine(freq: f64, amp: f64) -> Vec<i16> {
        (0..FFT_SIZE).map(|i| (amp * 32767.0 * (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(RATE)).sin()) as i16).collect()
    }

    #[test]
    fn fft_matches_a_naive_dft() {
        let n = 64;
        let x: Vec<f32> = (0..n).map(|i| ((i * 37 % 11) as f32 - 5.0) / 7.0).collect();
        let (mut re, mut im) = (x.clone(), vec![0.0; n]);
        fft(&mut re, &mut im);
        for k in 0..n {
            let (mut sr, mut si) = (0.0f64, 0.0f64);
            for (t, &v) in x.iter().enumerate() {
                let a = -2.0 * std::f64::consts::PI * (k * t) as f64 / n as f64;
                sr += f64::from(v) * a.cos();
                si += f64::from(v) * a.sin();
            }
            assert!((f64::from(re[k]) - sr).abs() < 1e-4 && (f64::from(im[k]) - si).abs() < 1e-4, "bin {k}");
        }
    }

    #[test]
    fn a_tone_lights_its_bar_and_full_scale_reaches_the_top() {
        let s = Spectrum::new();
        for freq in [100.0, 1000.0, 1875.0, 8000.0] {
            let bin = (freq * FFT_SIZE as f64 / f64::from(RATE)).round() as usize;
            let bar = (0..BARS).find(|&k| (s.bins(k).0..s.bins(k).1).contains(&bin)).unwrap();
            let l = s.levels(&sine(freq, 1.0));
            assert!(l[bar] > 15.5, "{freq} Hz: {l:?}");
            let far = if bar > 9 { 0 } else { BARS - 1 };
            assert!(l[far] < 3.0, "{freq} Hz leaks into bar {far}: {l:?}");
        }
        // -24 dB is a bit more than halfway down a 54 dB range.
        let l = s.levels(&sine(1000.0, 10f64.powf(-24.0 / 20.0)));
        assert!(l.iter().cloned().fold(0.0, f32::max) - 16.0 * 30.0 / 54.0 < 0.5);
        assert_eq!(s.levels(&[0; FFT_SIZE]), [0.0; BARS]);
    }

    #[test]
    fn bars_fall_and_peaks_hold() {
        let mut s = Spectrum::new();
        let tone = sine(1000.0, 1.0);
        s.update(Some(&tone));
        let k = (0..BARS).max_by(|&a, &b| s.bars[a].total_cmp(&s.bars[b])).unwrap();
        let top = s.bars[k];
        s.update(None);
        assert_eq!(s.bars[k], top - 1.5);
        assert_eq!(s.peaks[k], top, "held");
        for _ in 0..12 {
            s.update(None);
        }
        assert!(s.peaks[k] < top);
        for _ in 0..100 {
            s.update(None);
        }
        assert!(s.is_idle());
        // Bins cover the range without gaps, at any rate.
        for rate in [44100, 22050, 11025] {
            let s = Spectrum::with_rate(rate);
            for k in 1..BARS {
                assert_eq!(s.bins(k).0, s.bins(k - 1).1);
            }
            assert!(s.bins(BARS - 1).1 <= FFT_SIZE / 2, "{rate}: below Nyquist");
        }
    }

    #[test]
    fn drawing_uses_the_skin_colors() {
        let colors: [u32; 24] = std::array::from_fn(|i| i as u32 + 100);
        let area = Rect::new(0, 0, 76, 16);
        let mut c = Canvas::new(76, 16);
        let mut s = Spectrum::new();
        s.bars[0] = 16.0;
        s.peaks[0] = 16.0;
        s.bars[1] = 4.0;
        draw_spectrum(&mut c, area, &colors, &s);
        assert_eq!(c.get(0, 0), Some(123), "peak on top of a full bar");
        assert_eq!(c.get(0, 15), Some(117), "bottom row: color 17");
        assert_eq!(c.get(4, 12), Some(114), "a short bar's top row has its row's color");
        assert_eq!(c.get(4, 11), Some(100), "above it: background");
        assert_eq!(c.get(3, 0), Some(101), "the gap shows the dot grid");

        let mut c = Canvas::new(76, 16);
        draw_scope(&mut c, area, &colors, &[0; 76]);
        assert_eq!(c.get(10, 7), Some(118), "silence: the center line in color 18");
        let square: Vec<i16> = (0..76).map(|i| if i < 38 { 32767 } else { -32768 }).collect();
        draw_scope(&mut c, area, &colors, &square);
        assert_eq!(c.get(10, 0), Some(122), "the top edge, far from center");
        assert_eq!(c.get(38, 8), Some(118), "the jump is joined");
        assert_eq!(Mode::Spectrum.next().next().next(), Mode::Spectrum);
    }
}
