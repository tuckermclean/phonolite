//! Direct sound output through the OSS interface (`/dev/dsp`): on Linux,
//! ALSA's OSS emulation (`CONFIG_SND_PCM_OSS`) or a real OSS driver. For a
//! machine with no sound server, no alsa-lib and no out123 — the 486
//! distribution.
//!
//! **The only `unsafe` code in phonolite is here**, in `ioctl_int` and
//! `ioctl_none`: libc's `ioctl` (std links libc already), called with
//! the five OSS requests below, each of which passes at most one `int`.
//! Setting the format needs an ioctl; there is no safe route. A test keeps
//! `unsafe` out of every other file.
//!
//! The device is exclusive: no mixing with other programs (dmix lives in
//! alsa-lib, which `/dev/dsp` bypasses). Where a sound server runs,
//! phonolite uses out123 instead.

#![allow(unsafe_code)]

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::raw::{c_int, c_ulong};
use std::path::Path;

/// `_IOWR('P', nr, int)` on Linux: read/write, 4-byte argument, type 'P'.
/// The encoding below is x86's (also ARM's); other architectures differ.
const fn iowr(nr: c_ulong) -> c_ulong {
    (3 << 30) | (4 << 16) | ((b'P' as c_ulong) << 8) | nr
}

/// `SNDCTL_DSP_RESET`: drop what is queued, at once.
pub const SNDCTL_DSP_RESET: c_ulong = (b'P' as c_ulong) << 8;
/// `SNDCTL_DSP_SPEED`: sample rate.
pub const SNDCTL_DSP_SPEED: c_ulong = iowr(2);
/// `SNDCTL_DSP_SETFMT`: sample format.
pub const SNDCTL_DSP_SETFMT: c_ulong = iowr(5);
/// `SNDCTL_DSP_CHANNELS`: channel count.
pub const SNDCTL_DSP_CHANNELS: c_ulong = iowr(6);
/// `SNDCTL_DSP_SETFRAGMENT`: buffer fragments, `count << 16 | log2(size)`.
pub const SNDCTL_DSP_SETFRAGMENT: c_ulong = iowr(10);
/// `AFMT_S16_LE`.
pub const AFMT_S16_LE: c_int = 0x10;

extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

/// An ioctl whose argument is one `int`, read and written by the driver.
fn ioctl_int(file: &File, request: c_ulong, value: c_int) -> io::Result<c_int> {
    let mut v = value;
    // SAFETY: `file` keeps the descriptor open for the call. `request` is
    // one of the SNDCTL_DSP_* requests above, whose argument is a pointer
    // to a single int; `v` is that int, alive and exclusively borrowed for
    // the duration of the call. On a descriptor that is not a sound device
    // the kernel rejects the request (ENOTTY) without touching `v`.
    let r = unsafe { ioctl(file.as_raw_fd(), request, &mut v as *mut c_int) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(v)
    }
}

/// An ioctl with no argument.
fn ioctl_none(file: &File, request: c_ulong) -> io::Result<()> {
    // SAFETY: as above; this request takes no argument and reads none
    // (the variadic slot is left empty).
    let r = unsafe { ioctl(file.as_raw_fd(), request) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// The settings a device is asked for, through whatever sets them (the
/// real ioctls, or a fake in tests).
pub trait Dsp {
    /// Request with an int argument; the driver's answer.
    fn set(&mut self, request: c_ulong, value: c_int) -> io::Result<c_int>;
}

impl Dsp for File {
    fn set(&mut self, request: c_ulong, value: c_int) -> io::Result<c_int> {
        ioctl_int(self, request, value)
    }
}

/// Fragments: 8 × 4 KiB, about 0.19 s at 44.1 kHz stereo — small enough
/// that pacing, not the device, decides the latency.
pub const FRAGMENTS: c_int = (8 << 16) | 12;

/// Ask for 16-bit little-endian stereo at `rate`. A device that offers
/// something else is refused with a message saying what it offered.
pub fn configure(dev: &mut impl Dsp, rate: u32) -> io::Result<()> {
    let bad = |what: String| io::Error::new(io::ErrorKind::Unsupported, what);
    // Fragment size first: drivers take it only before the format is set.
    // Failing is harmless (the driver's default is just more latency).
    let _ = dev.set(SNDCTL_DSP_SETFRAGMENT, FRAGMENTS);
    let fmt = dev.set(SNDCTL_DSP_SETFMT, AFMT_S16_LE)?;
    if fmt != AFMT_S16_LE {
        return Err(bad(format!("the sound device does not take 16-bit samples (offered format {fmt:#x})")));
    }
    let ch = dev.set(SNDCTL_DSP_CHANNELS, 2)?;
    if ch != 2 {
        return Err(bad(format!("the sound device does not take stereo (offered {ch} channels)")));
    }
    let got = dev.set(SNDCTL_DSP_SPEED, rate as c_int)?;
    // Drivers may round a little (a card's clock dividers); 1% is inaudible.
    if (i64::from(got) - i64::from(rate)).abs() * 100 > i64::from(rate) {
        return Err(bad(format!("the sound device plays {got} Hz, not {rate} Hz (try --rate {got})")));
    }
    Ok(())
}

/// An open, configured device: the writer goes to the audio pump; the
/// [`Control`] stays with the player to cut playback off.
pub fn open(path: &Path, rate: u32) -> io::Result<(Control, File)> {
    let mut file = OpenOptions::new().write(true).open(path).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
    configure(&mut file, rate).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
    let control = file.try_clone()?;
    Ok((Control { file: Some(control) }, file))
}

/// The player's handle on an open device.
pub struct Control {
    file: Option<File>,
}

impl Control {
    /// Let it play out what it holds (the end of the music).
    pub fn finish(mut self) {
        self.file = None;
    }
}

impl Drop for Control {
    /// Cut it off: drop what the device has queued.
    fn drop(&mut self) {
        if let Some(f) = self.file.take() {
            let _ = ioctl_none(&f, SNDCTL_DSP_RESET);
        }
    }
}

/// Whether `path` names a device that could be OSS output.
pub fn exists(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path).is_ok_and(|m| m.file_type().is_char_device())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A device that answers from a table (or echoes the request).
    #[derive(Default)]
    struct Fake {
        answers: HashMap<c_ulong, c_int>,
        asked: Vec<(c_ulong, c_int)>,
    }

    impl Dsp for Fake {
        fn set(&mut self, request: c_ulong, value: c_int) -> io::Result<c_int> {
            self.asked.push((request, value));
            Ok(*self.answers.get(&request).unwrap_or(&value))
        }
    }

    #[test]
    fn request_numbers_are_linux_x86s() {
        // From <linux/soundcard.h> as compiled on x86.
        assert_eq!(SNDCTL_DSP_RESET, 0x0000_5000);
        assert_eq!(SNDCTL_DSP_SPEED, 0xC004_5002);
        assert_eq!(SNDCTL_DSP_SETFMT, 0xC004_5005);
        assert_eq!(SNDCTL_DSP_CHANNELS, 0xC004_5006);
        assert_eq!(SNDCTL_DSP_SETFRAGMENT, 0xC004_500A);
    }

    #[test]
    fn asks_for_fragments_first_then_the_format() {
        let mut d = Fake::default();
        configure(&mut d, 44100).unwrap();
        assert_eq!(d.asked, [(SNDCTL_DSP_SETFRAGMENT, FRAGMENTS), (SNDCTL_DSP_SETFMT, AFMT_S16_LE), (SNDCTL_DSP_CHANNELS, 2), (SNDCTL_DSP_SPEED, 44100)]);
    }

    #[test]
    fn refuses_what_it_cannot_play() {
        let mut d = Fake::default();
        d.answers.insert(SNDCTL_DSP_SETFMT, 0x08); // AFMT_U8
        assert!(configure(&mut d, 44100).unwrap_err().to_string().contains("16-bit"));
        let mut d = Fake::default();
        d.answers.insert(SNDCTL_DSP_CHANNELS, 1);
        assert!(configure(&mut d, 44100).unwrap_err().to_string().contains("stereo"));
        let mut d = Fake::default();
        d.answers.insert(SNDCTL_DSP_SPEED, 48000);
        let e = configure(&mut d, 44100).unwrap_err().to_string();
        assert!(e.contains("48000") && e.contains("--rate 48000"), "{e}");
        let mut d = Fake::default();
        d.answers.insert(SNDCTL_DSP_SPEED, 44110);
        assert!(configure(&mut d, 44100).is_ok(), "a clock divider's rounding is fine");
    }

    #[test]
    fn the_real_ioctl_fails_cleanly_on_a_file() {
        // No sound device here; a plain file proves the call itself: the
        // kernel says "not a typewriter" and nothing is written.
        let p = std::env::temp_dir().join(format!("phonolite-oss-{}", std::process::id()));
        std::fs::write(&p, b"").unwrap();
        let f = OpenOptions::new().write(true).open(&p).unwrap();
        let e = ioctl_int(&f, SNDCTL_DSP_SPEED, 44100).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(25), "ENOTTY: {e}");
        assert!(ioctl_none(&f, SNDCTL_DSP_RESET).is_err());
        let e = open(&p, 44100).err().unwrap();
        assert!(e.to_string().contains(p.to_str().unwrap()), "{e}");
        assert!(!exists(&p), "a plain file is not a device");
        assert!(exists(Path::new("/dev/null")), "a character device is");
        std::fs::remove_file(&p).unwrap();
    }

    /// `unsafe` stays here: no other source file may use it.
    #[test]
    fn no_unsafe_anywhere_else() {
        fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        walk(&root.join("src"), &mut files);
        walk(&root.join("tests"), &mut files);
        assert!(files.len() > 20);
        for f in files {
            if f.ends_with("src/oss.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&f).unwrap();
            let hits: Vec<&str> = text.lines().filter(|l| l.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| w == "unsafe")).collect();
            // The crate root's lint settings name it; nothing else may.
            let allowed = |l: &str| l.contains("unsafe_code");
            assert!(hits.iter().all(|l| allowed(l)), "{}: {hits:?}", f.display());
        }
    }
}
