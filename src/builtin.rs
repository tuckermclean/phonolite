//! The built-in decoder as an engine: what phonolite uses when mpg123 is
//! not installed (the 486 distribution ships without it). It reads the
//! file in small pieces on a thread, decodes with [`crate::mpeg`], and
//! speaks the same [`Message`]s as mpg123 so the player cannot tell them
//! apart: tags (ID3v2, ID3v1), the stream's format, its length (from the
//! LAME/Xing header, else the bitrate), the end. Output is 16-bit stereo
//! at the session's rate, resampled linearly if the file's differs.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::engine::Notify;
use crate::mpeg::{self, Decoder, FrameInfo, MAX_SAMPLES_PER_FRAME};
use crate::remote::{Command, Message, PlayState, Progress, StreamInfo};

/// File bytes kept ahead of the decoder (several frames, so a frame and
/// the next header are always in view).
const WINDOW: usize = 16 * 1024;

/// What the start of a file says about it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probe {
    /// Where the audio starts (after an ID3v2 tag).
    pub audio_start: u64,
    /// Where it ends (before an ID3v1 tag).
    pub audio_end: u64,
    /// Tags as `(key, value)`, keys as the player knows them.
    pub tags: Vec<(String, String)>,
    /// Frames, from a Xing/Info header.
    pub frames: Option<u32>,
    /// The Xing seek table: 100 entries, byte position in 256ths.
    pub toc: Option<[u8; 100]>,
    /// The first frame is a Xing/Info header, not audio.
    pub skip_first_frame: bool,
    /// A "Xing" header (variable bitrate), not "Info" (constant).
    pub vbr: bool,
}

fn synchsafe(b: &[u8]) -> usize {
    b.iter().take(4).fold(0, |a, &x| (a << 7) | usize::from(x & 0x7f))
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

fn utf16(b: &[u8], big_endian: bool) -> String {
    let units: Vec<u16> = b.chunks_exact(2).map(|c| if big_endian { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) }).collect();
    String::from_utf16_lossy(&units)
}

/// An ID3v2 text frame's value: encoding byte, then text (the first of
/// several null-separated strings).
fn id3_text(b: &[u8]) -> String {
    let Some((&enc, body)) = b.split_first() else { return String::new() };
    let s = match enc {
        0 => latin1(body),
        1 => match body {
            [0xff, 0xfe, rest @ ..] => utf16(rest, false),
            [0xfe, 0xff, rest @ ..] => utf16(rest, true),
            _ => utf16(body, false),
        },
        2 => utf16(body, true),
        _ => String::from_utf8_lossy(body).into_owned(),
    };
    s.split('\0').next().unwrap_or("").trim().to_string()
}

/// Parse an ID3v2 tag at the start of `data`: its total length and its
/// text fields.
pub fn parse_id3v2(data: &[u8]) -> Option<(usize, Vec<(String, String)>)> {
    if data.len() < 10 || &data[..3] != b"ID3" {
        return None;
    }
    let (major, flags) = (data[3], data[5]);
    let size = synchsafe(&data[6..10]);
    let total = 10 + size + if flags & 0x10 != 0 { 10 } else { 0 };
    let end = (10 + size).min(data.len());
    let mut at = 10;
    if flags & 0x40 != 0 && end >= at + 4 {
        // Extended header: v2.4 counts itself, v2.3 does not.
        let n = if major >= 4 { synchsafe(&data[at..at + 4]) } else { u32::from_be_bytes(data[at..at + 4].try_into().ok()?) as usize + 4 };
        at += n;
    }
    let mut tags = Vec::new();
    let (id_len, head) = if major == 2 { (3, 6) } else { (4, 10) };
    while at + head <= end {
        let id = &data[at..at + id_len];
        if id[0] == 0 {
            break; // padding
        }
        let len = match major {
            2 => (usize::from(data[at + 3]) << 16) | (usize::from(data[at + 4]) << 8) | usize::from(data[at + 5]),
            3 => u32::from_be_bytes(data[at + 4..at + 8].try_into().ok()?) as usize,
            _ => synchsafe(&data[at + 4..at + 8]),
        };
        let body = &data[(at + head).min(end)..(at + head + len).min(end)];
        let key = match id {
            b"TIT2" | b"TT2" => Some("title"),
            b"TPE1" | b"TP1" => Some("artist"),
            b"TALB" | b"TAL" => Some("album"),
            b"TYER" | b"TDRC" | b"TYE" => Some("year"),
            b"TCON" | b"TCO" => Some("genre"),
            _ => None,
        };
        if let Some(k) = key {
            let v = id3_text(body);
            if !v.is_empty() {
                tags.push((k.to_string(), v));
            }
        }
        at += head + len;
    }
    Some((total, tags))
}

/// An ID3v1 tag (the last 128 bytes), as the `id3v1` field mpg123 sends.
pub fn parse_id3v1(tail: &[u8]) -> Option<String> {
    if tail.len() != 128 || &tail[..3] != b"TAG" {
        return None;
    }
    let field = |a: usize, b: usize| latin1(&tail[a..b]).trim_end_matches(['\0', ' ']).trim().to_string();
    Some([field(3, 33), field(33, 63), field(63, 93), field(93, 97), field(97, 127), String::new()].join("\u{1f}"))
}

/// Look for a Xing/Info header in the first frame (`frame` starts at its
/// header): frame count, seek table, and whether it says variable bitrate.
pub fn parse_xing(frame: &[u8]) -> Option<(Option<u32>, Option<[u8; 100]>, bool)> {
    if frame.len() < 4 {
        return None;
    }
    let mpeg1 = frame[1] & 0x08 != 0;
    let mono = frame[3] & 0xC0 == 0xC0;
    let side = match (mpeg1, mono) {
        (true, false) => 32,
        (true, true) | (false, false) => 17,
        (false, true) => 9,
    };
    let at = 4 + side;
    let tag = frame.get(at..at + 4)?;
    if tag != b"Xing" && tag != b"Info" {
        return None;
    }
    let flags = u32::from_be_bytes(frame.get(at + 4..at + 8)?.try_into().ok()?);
    let mut p = at + 8;
    let mut frames = None;
    if flags & 1 != 0 {
        frames = Some(u32::from_be_bytes(frame.get(p..p + 4)?.try_into().ok()?));
        p += 4;
    }
    if flags & 2 != 0 {
        p += 4;
    }
    let toc = if flags & 4 != 0 { frame.get(p..p + 100).map(|t| t.try_into().expect("100 bytes")) } else { None };
    Some((frames, toc, tag == b"Xing"))
}

/// Read what the file's head and tail say.
pub fn probe(file: &mut File) -> io::Result<Probe> {
    let len = file.seek(SeekFrom::End(0))?;
    let mut p = Probe { audio_end: len, ..Probe::default() };
    if len >= 128 {
        file.seek(SeekFrom::Start(len - 128))?;
        let mut tail = [0u8; 128];
        file.read_exact(&mut tail)?;
        if let Some(v1) = parse_id3v1(&tail) {
            p.tags.push(("id3v1".into(), v1));
            p.audio_end = len - 128;
        }
    }
    file.seek(SeekFrom::Start(0))?;
    let mut head = [0u8; 10];
    let n = read_full(file, &mut head)?;
    if n == 10 && &head[..3] == b"ID3" {
        let size = 10 + synchsafe(&head[6..10]) + if head[5] & 0x10 != 0 { 10 } else { 0 };
        // Tags can be large (cover art); read only enough for the text.
        let mut tag = vec![0u8; size.min(256 * 1024)];
        file.seek(SeekFrom::Start(0))?;
        let got = read_full(file, &mut tag)?;
        tag.truncate(got);
        if let Some((total, tags)) = parse_id3v2(&tag) {
            p.audio_start = total as u64;
            p.tags.splice(0..0, tags); // v2 first: its fields win
        }
    }
    // The first frame: is it a Xing/Info header?
    file.seek(SeekFrom::Start(p.audio_start))?;
    let mut first = vec![0u8; 4096];
    let got = read_full(file, &mut first)?;
    first.truncate(got);
    let mut d = Decoder::new();
    let mut info = FrameInfo::default();
    if d.decode_frame(&first, None, &mut info) > 0 {
        let frame = &first[info.frame_offset..];
        if let Some((frames, toc, vbr)) = parse_xing(frame) {
            p.frames = frames;
            p.toc = toc;
            p.skip_first_frame = true;
            p.vbr = vbr;
        }
        p.audio_start += info.frame_offset as u64;
    }
    Ok(p)
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// Linear interpolation between sample rates (cheap; good enough for the
/// odd 48 kHz or 22.05 kHz file in a 44.1 kHz session).
struct Resampler {
    step: f64,
    t: f64,
    prev: [f32; 2],
}

impl Resampler {
    fn push(&mut self, cur: [f32; 2], out: &mut Vec<u8>) {
        while self.t < 1.0 {
            for (&p, &c) in self.prev.iter().zip(&cur) {
                let v = p + (c - p) * self.t as f32;
                out.extend((v.round().clamp(-32768.0, 32767.0) as i16).to_le_bytes());
            }
            self.t += self.step;
        }
        self.t -= 1.0;
        self.prev = cur;
    }
}

/// A pipe-like reader over the decoder thread's chunks.
pub struct PcmReader {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    at: usize,
}

impl Read for PcmReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.at == self.buf.len() {
            match self.rx.recv() {
                Ok(b) => {
                    self.buf = b;
                    self.at = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.at);
        out[..n].copy_from_slice(&self.buf[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// A built-in decoding of one file.
pub struct Engine {
    rx: Receiver<Message>,
    stop: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    eq: Arc<Mutex<[[f32; 32]; 2]>>,
}

impl Engine {
    /// Start decoding `path` from `offset` seconds, producing stereo s16 at
    /// `rate`, with equalizer gains `eq` (per channel and subband, as
    /// mpg123's `EQ` command takes them); `notify` is called for each
    /// message.
    pub fn start(path: &str, offset: f64, rate: u32, eq: [[f64; 32]; 2], notify: Notify) -> io::Result<(Engine, PcmReader)> {
        let (tx, rx) = mpsc::channel();
        let (ptx, prx) = mpsc::sync_channel(2);
        let stop = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let eq = Arc::new(Mutex::new(eq.map(|ch| ch.map(|g| g as f32))));
        let job = Job { path: path.to_string(), offset, rate, tx, pcm: ptx, stop: stop.clone(), eq: eq.clone(), notify };
        let done = exited.clone();
        thread::spawn(move || {
            if let Err(e) = job.run() {
                job.say(Message::Error(e));
            }
            done.store(true, Ordering::SeqCst);
        });
        Ok((Engine { rx, stop, exited, eq }, PcmReader { rx: prx, buf: Vec::new(), at: 0 }))
    }

    /// Commands as mpg123 would take them: equalizer values apply; the
    /// rest have no meaning here (loading and seeking happen at start).
    pub fn send(&mut self, cmd: &Command) -> io::Result<()> {
        if let Command::Eq(ch, band, v) = *cmd {
            let mut eq = self.eq.lock().unwrap();
            for c in 0..2 {
                if ch == 3 || usize::from(ch) == c + 1 {
                    if let Some(slot) = eq[c].get_mut(usize::from(band)) {
                        *slot = v as f32;
                    }
                }
            }
        }
        Ok(())
    }

    /// The next message, if any.
    pub fn try_recv(&self) -> Option<Message> {
        self.rx.try_recv().ok()
    }

    /// Stop decoding.
    pub fn kill(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Nothing to do: the thread ends at the end of the file.
    pub fn finish(&mut self) {}

    /// The decoding thread has ended.
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.kill();
    }
}

struct Job {
    path: String,
    offset: f64,
    rate: u32,
    tx: mpsc::Sender<Message>,
    pcm: SyncSender<Vec<u8>>,
    stop: Arc<AtomicBool>,
    eq: Arc<Mutex<[[f32; 32]; 2]>>,
    notify: Notify,
}

impl Job {
    fn say(&self, m: Message) {
        let quiet = matches!(m, Message::Progress(_));
        let _ = self.tx.send(m);
        if !quiet {
            (self.notify)();
        }
    }

    fn run(&self) -> Result<(), String> {
        if crate::playlist::is_url(&self.path) {
            return Err(format!("{}: streams need mpg123 (the built-in decoder plays files)", self.path));
        }
        let path = Path::new(&self.path);
        let mut file = File::open(path).map_err(|e| format!("{}: {e}", self.path))?;
        let p = probe(&mut file).map_err(|e| format!("{}: {e}", self.path))?;
        for (k, v) in &p.tags {
            self.say(Message::Tag(k.clone(), v.clone()));
        }
        // The format, from the first audio frame.
        file.seek(SeekFrom::Start(p.audio_start)).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; WINDOW];
        let got = read_full(&mut file, &mut buf).map_err(|e| e.to_string())?;
        buf.truncate(got);
        let mut dec = Decoder::new();
        let mut info = FrameInfo::default();
        if dec.decode_frame(&buf, None, &mut info) == 0 {
            return Err(format!("{}: no MPEG audio found", self.path));
        }
        let samples_per_frame = mpeg::header_facts(&buf[info.frame_offset..]).map_or(1152, |f| f.1);
        let audio_bytes = p.audio_end.saturating_sub(p.audio_start);
        let duration = match p.frames {
            Some(n) => f64::from(n) * samples_per_frame as f64 / f64::from(info.hz),
            None if info.bitrate_kbps > 0 => audio_bytes as f64 * 8.0 / (f64::from(info.bitrate_kbps) * 1000.0),
            None => 0.0,
        };
        let version = if buf[info.frame_offset + 1] & 0x08 != 0 {
            "1.0"
        } else if buf[info.frame_offset + 1] & 0x10 != 0 {
            "2.0"
        } else {
            "2.5"
        };
        let mode = match buf[info.frame_offset + 3] >> 6 {
            0 => "Stereo",
            1 => "Joint-Stereo",
            2 => "Dual-Channel",
            _ => "Single-Channel",
        };
        self.say(Message::State(PlayState::Playing));
        self.say(Message::Stream(StreamInfo {
            version: version.into(),
            layer: info.layer,
            rate: info.hz,
            mode: mode.into(),
            channels: info.channels as u8,
            bitrate: info.bitrate_kbps,
            vbr: p.vbr,
        }));
        let offset = if duration > 0.0 { self.offset.clamp(0.0, duration) } else { 0.0 };
        self.say(Message::Progress(Progress { frame: 0, frames_left: 0, seconds: offset, seconds_left: (duration - offset).max(0.0) }));

        // Where to start reading.
        let mut start = p.audio_start;
        let mut skip_first = p.skip_first_frame;
        if offset > 0.0 && duration > 0.0 {
            let frac = offset / duration;
            let pos = match p.toc {
                Some(toc) => {
                    let pct = (frac * 100.0).min(99.999);
                    let i = pct as usize;
                    let a = f64::from(toc[i]);
                    let b = if i < 99 { f64::from(toc[i + 1]) } else { 256.0 };
                    (a + (b - a) * (pct - i as f64)) / 256.0
                }
                None => frac,
            };
            start = p.audio_start + (pos * audio_bytes as f64) as u64;
            skip_first = false;
            dec.reset();
        } else {
            dec = Decoder::new();
        }
        file.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
        buf.clear();
        let mut remaining = p.audio_end.saturating_sub(start);
        let mut eof = false;
        let mut at = 0usize;
        let mut pcm = vec![0i16; MAX_SAMPLES_PER_FRAME];
        let mut rs: Option<Resampler> = None;
        let mut chunk = Vec::with_capacity(8192);
        loop {
            if self.stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            // Keep the window full.
            if !eof && buf.len() - at < WINDOW / 2 {
                buf.drain(..at);
                at = 0;
                let want = (WINDOW - buf.len()).min(remaining as usize);
                let old = buf.len();
                buf.resize(old + want, 0);
                let n = read_full(&mut file, &mut buf[old..]).map_err(|e| e.to_string())?;
                buf.truncate(old + n);
                remaining -= n as u64;
                eof = n < want || remaining == 0;
            }
            if at >= buf.len() {
                break;
            }
            dec.eq = Some(*self.eq.lock().unwrap()).filter(|g| g.iter().flatten().any(|&v| v != 1.0));
            let mut info = FrameInfo::default();
            let n = dec.decode_frame(&buf[at..], Some(&mut pcm), &mut info);
            if info.frame_bytes == 0 {
                if eof {
                    break;
                }
                // Not enough in view to find a frame: read more.
                at = buf.len().min(at);
                if buf.len() - at >= WINDOW / 2 {
                    at += 1; // garbage that never syncs: step over it
                }
                continue;
            }
            at += info.frame_bytes;
            if skip_first {
                skip_first = false;
                continue;
            }
            if n == 0 {
                continue;
            }
            let rs = rs.get_or_insert_with(|| Resampler { step: f64::from(info.hz) / f64::from(self.rate.max(1)), t: 0.0, prev: [0.0; 2] });
            let ch = info.channels;
            for i in 0..n {
                let l = pcm[i * ch];
                let r = if ch == 2 { pcm[i * ch + 1] } else { l };
                if info.hz == self.rate {
                    chunk.extend(l.to_le_bytes());
                    chunk.extend(r.to_le_bytes());
                } else {
                    rs.push([f32::from(l), f32::from(r)], &mut chunk);
                }
            }
            if chunk.len() >= 4096 && self.pcm.send(std::mem::take(&mut chunk)).is_err() {
                return Ok(()); // nobody listening: flushed
            }
        }
        if !chunk.is_empty() {
            let _ = self.pcm.send(chunk);
        }
        self.say(Message::State(PlayState::Ended));
        self.say(Message::State(PlayState::Stopped));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::scratch;
    use crate::testsignal;

    const FLAT: [[f64; 32]; 2] = [[1.0; 32]; 2];

    fn fixture(name: &str) -> String {
        format!("{}/tests/data/mpeg/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    fn run(path: &str, offset: f64, rate: u32) -> (Vec<Message>, Vec<i16>) {
        let (e, mut pcm) = Engine::start(path, offset, rate, FLAT, Box::new(|| {})).unwrap();
        let mut bytes = Vec::new();
        pcm.read_to_end(&mut bytes).unwrap();
        let mut msgs = Vec::new();
        while !e.has_exited() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        while let Some(m) = e.try_recv() {
            msgs.push(m);
        }
        (msgs, bytes.chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect())
    }

    #[test]
    fn id3v2_versions_and_encodings() {
        let mut tag = testsignal::id3v2(&[("TIT2", "Héllo"), ("TPE1", "Band"), ("TXXX", "skip")]);
        tag.extend([0; 20]); // padding
        let (n, tags) = parse_id3v2(&tag).unwrap();
        assert_eq!(n, tag.len() - 20, "size from the header");
        assert_eq!(tags, [("title".into(), "Héllo".into()), ("artist".into(), "Band".into())]);
        // v2.3, UTF-16 with BOM, plain sizes.
        let mut body = b"TIT2".to_vec();
        let text: Vec<u8> = [1u8, 0xff, 0xfe].into_iter().chain("Ünï".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        body.extend((text.len() as u32).to_be_bytes());
        body.extend([0, 0]);
        body.extend(&text);
        let mut v23 = b"ID3\x03\x00\x00".to_vec();
        v23.extend([0, 0, 0, body.len() as u8]);
        v23.extend(&body);
        assert_eq!(parse_id3v2(&v23).unwrap().1, [("title".into(), "Ünï".into())]);
        // v2.2: three-letter ids, three-byte sizes, Latin-1.
        let mut v22 = b"ID3\x02\x00\x00\x00\x00\x00\x0aTT2\x00\x00\x04\x00Abc".to_vec();
        v22.truncate(10 + 10);
        assert_eq!(parse_id3v2(&v22).unwrap().1, [("title".into(), "Abc".into())]);
        assert!(parse_id3v2(b"ID").is_none());
        assert!(parse_id3v2(b"NOTATAG").is_none());
    }

    #[test]
    fn id3v1() {
        let mut t = b"TAG".to_vec();
        for (s, n) in [("Title", 30), ("Artist", 30), ("Album", 30), ("1999", 4), ("Note", 30)] {
            let mut f = s.as_bytes().to_vec();
            f.resize(n, 0);
            t.extend(f);
        }
        t.push(17);
        let v = parse_id3v1(&t).unwrap();
        assert_eq!(crate::remote::id3v1_fields(&v), ["Title", "Artist", "Album", "1999", "Note", ""]);
        assert!(parse_id3v1(&t[..100]).is_none());
    }

    #[test]
    fn lame_files_report_length_tags_and_play_whole() {
        let d = scratch("builtin-tags");
        let f = d.join("t.mp3");
        let mut data = testsignal::id3v2(&[("TIT2", "Chords"), ("TPE1", "Generator")]);
        data.extend(std::fs::read(fixture("l3-128-joint.mp3")).unwrap());
        std::fs::write(&f, data).unwrap();
        let (msgs, pcm) = run(f.to_str().unwrap(), 0.0, 44100);
        assert!(msgs.contains(&Message::Tag("title".into(), "Chords".into())), "{msgs:?}");
        assert!(msgs.iter().any(|m| matches!(m, Message::Stream(s) if s.rate == 44100 && s.layer == 3 && s.channels == 2 && s.mode == "Joint-Stereo")));
        let Some(Message::Progress(p)) = msgs.iter().find(|m| matches!(m, Message::Progress(_))) else { panic!() };
        // 0.5 s of source; LAME's frame count includes its delay and padding.
        assert!((p.seconds_left - 0.5).abs() < 0.08, "{p:?}");
        assert_eq!(msgs.iter().filter(|m| **m == Message::State(PlayState::Ended)).count(), 1);
        // Stereo out; the Info frame skipped (no leading silent frame).
        assert!(pcm.len() >= 2 * 22050);
        assert!(pcm[..2 * 1152].iter().any(|&s| s.abs() > 100), "audio within the first frame, not a silent Info frame");
    }

    #[test]
    fn same_samples_as_the_plain_decoder() {
        // No resampling, stereo file: exactly what mpeg::decode_all gives,
        // minus the Info frame.
        let (_, pcm) = run(&fixture("l3-128-stereo.mp3"), 0.0, 44100);
        let (raw, _) = crate::mpeg::decode_all(&std::fs::read(fixture("l3-128-stereo.mp3")).unwrap());
        assert_eq!(pcm[..], raw[2 * 1152..2 * 1152 + pcm.len()]);
    }

    #[test]
    fn mono_and_low_rates_come_out_stereo_at_the_session_rate() {
        let (msgs, pcm) = run(&fixture("l3-64-mono.mp3"), 0.0, 44100);
        assert!(msgs.iter().any(|m| matches!(m, Message::Stream(s) if s.channels == 1)));
        assert!(pcm.chunks(2).all(|f| f[0] == f[1]), "mono doubled");
        let (_, pcm) = run(&fixture("l3-mpeg25-8k.mp3"), 0.0, 44100);
        // Every 8 kHz sample the decoder gives becomes 44100/8000 of them.
        let (raw, _) = crate::mpeg::decode_all(&std::fs::read(fixture("l3-mpeg25-8k.mp3")).unwrap());
        let want = raw.len() as f64 * 44100.0 / 8000.0;
        let frames = pcm.len() / 2;
        assert!((frames as f64 - want).abs() <= 2.0, "{frames} vs {want}");
        // And the 300 Hz-ish chords survive: correlation with a sample-and-hold
        // upsampling of the raw decode is high.
        let held: Vec<f64> = (0..frames).map(|i| f64::from(raw[(i * 8000 / 44100).min(raw.len() - 1)])).collect();
        let ours: Vec<f64> = pcm.iter().step_by(2).map(|&v| f64::from(v)).collect();
        let dot: f64 = held.iter().zip(&ours).map(|(a, b)| a * b).sum();
        let norm = (held.iter().map(|a| a * a).sum::<f64>() * ours.iter().map(|b| b * b).sum::<f64>()).sqrt();
        assert!(dot / norm > 0.95, "{}", dot / norm);
    }

    #[test]
    fn seeking_starts_later() {
        let d = scratch("builtin-seek");
        let f = d.join("long.mp3");
        std::fs::write(&f, testsignal::tone(2, 0.8, 4.0)).unwrap();
        let (_, whole) = run(f.to_str().unwrap(), 0.0, 48000);
        let (msgs, tail) = run(f.to_str().unwrap(), 3.0, 48000);
        let Some(Message::Progress(p)) = msgs.iter().find(|m| matches!(m, Message::Progress(_))) else { panic!() };
        assert_eq!(p.seconds, 3.0);
        assert!((p.seconds + p.seconds_left - 4.0).abs() < 0.01, "CBR length from the bitrate: {p:?}");
        let secs = |v: &[i16]| v.len() as f64 / 2.0 / 48000.0;
        assert!((secs(&whole) - 4.0).abs() < 0.01 && (secs(&tail) - 1.0).abs() < 0.02, "{} {}", secs(&whole), secs(&tail));
    }

    #[test]
    fn eq_commands_reach_the_decoder() {
        let d = scratch("builtin-eq");
        let f = d.join("t.mp3");
        std::fs::write(&f, testsignal::tone(2, 0.8, 0.5)).unwrap();
        let peak = |s: &[i16]| s[s.len() / 2..].iter().map(|v| i32::from(*v).abs()).max().unwrap();
        let (_, flat) = run(f.to_str().unwrap(), 0.0, 48000);
        let (mut e, mut pcm) = Engine::start(f.to_str().unwrap(), 0.0, 48000, FLAT, Box::new(|| {})).unwrap();
        e.send(&Command::Eq(3, 2, 0.5)).unwrap();
        let mut bytes = Vec::new();
        pcm.read_to_end(&mut bytes).unwrap();
        let s: Vec<i16> = bytes.chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        // The last half second is all after the command (two chunks in flight at most).
        let ratio = f64::from(peak(&s)) / f64::from(peak(&flat));
        assert!((ratio - 0.5).abs() < 0.02, "{ratio}");
    }

    #[test]
    fn unreadable_files_say_so() {
        let (msgs, pcm) = run("/nonexistent/x.mp3", 0.0, 44100);
        assert!(pcm.is_empty());
        assert!(matches!(&msgs[..], [Message::Error(e)] if e.contains("/nonexistent/x.mp3")), "{msgs:?}");
        let d = scratch("builtin-junk");
        let f = d.join("junk.mp3");
        std::fs::write(&f, b"this is not audio at all").unwrap();
        let (msgs, _) = run(f.to_str().unwrap(), 0.0, 44100);
        assert!(matches!(&msgs[..], [Message::Error(e)] if e.contains("no MPEG audio")), "{msgs:?}");
        let (msgs, _) = run("http://radio.example/stream", 0.0, 44100);
        assert!(matches!(&msgs[..], [Message::Error(e)] if e.contains("streams need mpg123")), "{msgs:?}");
    }
}
