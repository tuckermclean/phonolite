use super::*;

fn have_mpg123() -> bool {
    let ok = std::process::Command::new("mpg123")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok || std::env::var_os("PHONOLITE_REQUIRE_MPG123").is_none(), "PHONOLITE_REQUIRE_MPG123 is set but mpg123 cannot be run");
    ok
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../tests/data/mpeg/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

const FIXTURES: [&str; 12] = [
    "l3-128-stereo.mp3",
    "l3-128-joint.mp3",
    "l3-128-notag.mp3",
    "l3-320-noise.mp3",
    "l3-112-clicks.mp3",
    "l3-vbr-noise.mp3",
    "l3-64-mono.mp3",
    "l3-mpeg2-22k.mp3",
    "l3-mpeg25-8k.mp3",
    "l2-192-stereo.mp2",
    "l2-64-mono.mp2",
    "l2-mpeg2-22k.mp2",
];

/// Our decode against mpg123's (an independent decoder) of the same file:
/// after aligning (mpg123 skips a LAME Info frame a plain decoder plays as
/// silence), the two agree to within a few least significant bits.
#[test]
fn matches_mpg123_on_every_fixture() {
    if !have_mpg123() {
        return;
    }
    for name in FIXTURES {
        let path = format!("{}/../tests/data/mpeg/{name}", env!("CARGO_MANIFEST_DIR"));
        let reference: Vec<i16> = std::process::Command::new("mpg123")
            .args(["-q", "-s", "-e", "s16", "--no-gapless"])
            .arg(&path)
            .output()
            .unwrap()
            .stdout
            .chunks(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        let (ours, info) = decode_all(&fixture(name));
        assert!(reference.len() > 1000 && ours.len() > 1000, "{name}: {} vs {}", ours.len(), reference.len());
        let ch = info.channels;
        let frame = 1152 * ch;
        // Our output may lead by whole frames of silence (the Info frame).
        let best = (0..=2)
            .map(|f| f * frame)
            .filter(|&skip| skip < ours.len())
            .map(|skip| {
                let n = (ours.len() - skip).min(reference.len());
                let maxdiff = (0..n).map(|i| (i32::from(ours[skip + i]) - i32::from(reference[i])).abs()).max().unwrap_or(i32::MAX);
                (maxdiff, skip, n)
            })
            .min()
            .unwrap();
        let (maxdiff, skip, n) = best;
        let peak = reference.iter().map(|v| i32::from(*v).abs()).max().unwrap();
        assert!(peak > 1000, "{name}: the reference is not silent");
        assert!(n + 2 * frame >= reference.len(), "{name}: covered {n} of {}", reference.len());
        assert!(maxdiff <= 1, "{name}: max difference {maxdiff} (skip {skip}, {n} samples, peak {peak})");
    }
}

#[test]
fn frame_facts() {
    let (pcm, info) = decode_all(&fixture("l3-mpeg25-8k.mp3"));
    assert_eq!((info.layer, info.hz, info.channels, info.bitrate_kbps), (3, 8000, 1, 16));
    assert!(pcm.len() >= 4000);
    let (_, info) = decode_all(&fixture("l2-192-stereo.mp2"));
    assert_eq!((info.layer, info.hz, info.channels, info.bitrate_kbps), (2, 44100, 2, 192));
    let data = crate::testsignal::tone(2, 0.8, 0.1);
    let (pcm, info) = decode_all(&data);
    assert_eq!((info.layer, info.hz, info.channels), (1, 48000, 2));
    assert_eq!(pcm.len(), 384 * 2 * 13);
    assert!(header_facts(&data).is_some_and(|(len, samples, hz, kbps)| (len, samples, hz, kbps) == (384, 384, 48000, 384)));
}

#[test]
fn the_equalizer_scales_subbands() {
    // Layer I tone in subband 2: gain on that subband scales the output.
    let data = crate::testsignal::tone(2, 0.8, 0.2);
    let (flat, _) = decode_all(&data);
    let mut d = Decoder::new();
    let mut gains = [[1.0f32; 32]; 2];
    gains[0][2] = 0.5;
    gains[1][2] = 0.0;
    d.eq = Some(gains);
    let mut pcm = vec![0i16; MAX_SAMPLES_PER_FRAME];
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let mut info = FrameInfo::default();
        let n = d.decode_frame(&data[at..], Some(&mut pcm), &mut info);
        if info.frame_bytes == 0 {
            break;
        }
        out.extend_from_slice(&pcm[..n * 2]);
        at += info.frame_bytes;
    }
    let peak = |v: &[i16], ch: usize| v.iter().skip(ch).step_by(2).map(|s| i32::from(*s).abs()).max().unwrap();
    assert!((f64::from(peak(&out, 0)) / f64::from(peak(&flat, 0)) - 0.5).abs() < 0.01);
    assert_eq!(peak(&out, 1), 0);
}

#[test]
fn damage_never_panics() {
    for name in FIXTURES {
        let good = fixture(name);
        let mut x = 0x1234_5678u32;
        for round in 0..40 {
            let mut bad = good.clone();
            for _ in 0..(1 + round % 8) {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let i = (x >> 8) as usize % bad.len();
                bad[i] ^= (x >> 3) as u8 | 1;
            }
            let _ = decode_all(&bad);
            let cut = (x as usize >> 4) % bad.len();
            let _ = decode_all(&bad[..cut]);
        }
    }
    let _ = decode_all(&[]);
    let _ = decode_all(&[0xff, 0xfb, 0x90]);
    let _ = decode_all(&[0xff; 5000]);
}
