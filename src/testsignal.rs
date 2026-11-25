//! Generated MPEG audio for tests: a minimal MPEG-1 Layer I encoder that
//! puts a pure tone in one subband per channel, plus ID3 tags.
//!
//! A constant-magnitude subband sample sequence modulated by `cos(πn/2)`
//! comes out of the decoder's synthesis filterbank as a sine at the
//! subband's center, `(2k + 1) · 48000 / 128` Hz — so subband 2 is
//! 1875 Hz. Frames are 48 kHz, 384 kbit/s, 384 bytes, 384 samples (8 ms).

/// One channel's tone: subband `0..32` and amplitude `0.0..=1.0` (1.0 is
/// about half of full scale after decoding). Amplitude 0 is silence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tone {
    /// Subband.
    pub subband: u8,
    /// Amplitude.
    pub amplitude: f64,
}

impl Tone {
    /// The frequency this tone decodes to, in Hz.
    pub fn frequency(&self) -> f64 {
        f64::from(2 * u32::from(self.subband) + 1) * 48000.0 / 128.0
    }
}

/// Sample rate of the generated stream.
pub const RATE: u32 = 48000;
/// Frames per second of the generated stream.
pub const FRAMES_PER_SECOND: u32 = 125;

struct Bits {
    bytes: Vec<u8>,
    bit: u32,
}

impl Bits {
    fn put(&mut self, value: u32, n: u32) {
        for i in (0..n).rev() {
            if self.bit % 8 == 0 {
                self.bytes.push(0);
            }
            if (value >> i) & 1 == 1 {
                *self.bytes.last_mut().unwrap() |= 0x80 >> (self.bit % 8);
            }
            self.bit += 1;
        }
    }
}

fn frame(tones: [Tone; 2], first_sample: u32) -> Vec<u8> {
    const NB: u32 = 15; // bits per sample (allocation code 14)
    let mut b = Bits { bytes: Vec::with_capacity(384), bit: 0 };
    // Sync, MPEG-1, Layer I, no CRC; 384 kbit/s, 48 kHz, no padding; stereo.
    b.put(0xFFFF_C400, 32);
    let on = |sb: u32, t: &Tone| t.amplitude > 0.0 && u32::from(t.subband) == sb;
    for sb in 0..32 {
        for t in &tones {
            b.put(if on(sb, t) { NB - 1 } else { 0 }, 4);
        }
    }
    for sb in 0..32 {
        for t in &tones {
            if on(sb, t) {
                b.put(0, 6); // scalefactor index 0: 2.0
            }
        }
    }
    for s in 0..12 {
        for sb in 0..32 {
            for t in &tones {
                if on(sb, t) {
                    let n = first_sample + s;
                    let v = t.amplitude.min(1.0) * (std::f64::consts::FRAC_PI_2 * f64::from(n)).cos() / 2.0;
                    // Layer I requantization inverted: s = 2^nb/(2^nb-1)·(f + 2^(1-nb)).
                    let full = f64::from(1u32 << NB);
                    let f = v * (full - 1.0) / full - 2.0 / full;
                    let half = 1i64 << (NB - 1);
                    let q = ((f * half as f64).round() as i64).clamp(-half, half - 1);
                    b.put(((q as u32) & ((1 << NB) - 1)) ^ (1 << (NB - 1)), NB);
                }
            }
        }
    }
    b.bytes.resize(384, 0);
    b.bytes
}

/// `seconds` of a tone per channel (left, right), as Layer I frames.
pub fn tones(left: Tone, right: Tone, seconds: f64) -> Vec<u8> {
    let frames = (seconds * f64::from(FRAMES_PER_SECOND)).round() as u32;
    (0..frames).flat_map(|i| frame([left, right], i * 12)).collect()
}

/// `seconds` of the same tone in both channels.
pub fn tone(subband: u8, amplitude: f64, seconds: f64) -> Vec<u8> {
    let t = Tone { subband, amplitude };
    tones(t, t, seconds)
}

/// An ID3v2.4 tag with UTF-8 text frames, e.g. `[("TIT2", "Song")]`, to
/// put in front of the audio.
pub fn id3v2(frames: &[(&str, &str)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (id, text) in frames {
        let len = text.len() as u32 + 1;
        body.extend(id.as_bytes());
        // ID3v2.4 sizes are "synchsafe": 7 bits per byte.
        body.extend(synchsafe(len));
        body.extend([0, 0, 3]); // flags, then encoding 3 = UTF-8
        body.extend(text.as_bytes());
    }
    let mut tag = b"ID3\x04\x00\x00".to_vec();
    tag.extend(synchsafe(body.len() as u32));
    tag.extend(body);
    tag
}

fn synchsafe(n: u32) -> [u8; 4] {
    [(n >> 21) as u8 & 127, (n >> 14) as u8 & 127, (n >> 7) as u8 & 127, n as u8 & 127]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_384_bytes_with_a_layer_one_header() {
        let d = tone(2, 1.0, 0.1);
        assert_eq!(d.len(), 384 * 13);
        for f in d.chunks(384) {
            assert_eq!(&f[..4], &[0xFF, 0xFF, 0xC4, 0x00]);
        }
        assert_eq!(Tone { subband: 2, amplitude: 1.0 }.frequency(), 1875.0);
        assert_eq!(Tone { subband: 0, amplitude: 1.0 }.frequency(), 375.0);
    }

    #[test]
    fn silence_allocates_nothing() {
        let d = tone(5, 0.0, 0.008);
        assert!(d[4..].iter().all(|&b| b == 0));
    }

    #[test]
    fn id3v2_layout() {
        let t = id3v2(&[("TIT2", "Hé")]);
        assert_eq!(&t[..10], b"ID3\x04\x00\x00\x00\x00\x00\x0e");
        assert_eq!(&t[10..], b"TIT2\x00\x00\x00\x04\x00\x00\x03H\xc3\xa9");
        assert_eq!(synchsafe(200), [0, 0, 1, 72]);
    }
}
