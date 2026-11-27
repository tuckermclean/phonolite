//! A PulseAudio client: just enough of the native protocol to play one
//! stream. Safe Rust over a Unix socket, as protolith speaks X11. It plays
//! to PulseAudio itself and to everything that speaks its protocol:
//! PipeWire's Pulse layer (most current desktops) and WSLg.
//!
//! The protocol: packets of a 20-byte header (length, channel, offset
//! high/low, flags — big-endian words) and a payload. Commands go on
//! channel `!0` as "tagstructs" (typed fields, each a tag byte and its
//! value); audio goes on the stream's channel. The server says how much
//! audio it wants with `REQUEST`; the client sends no more than that.
//!
//! We claim protocol version 13 — the first with property lists, so the
//! fewest fields to get right — and no shared memory, so audio travels in
//! the socket. Constants and field orders follow PulseAudio's
//! `pulsecore/native-common.h`, `tagstruct.c` and `protocol-native.c`.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Commands (pulsecore/native-common.h).
const ERROR: u32 = 0;
const REPLY: u32 = 2;
const CREATE_PLAYBACK_STREAM: u32 = 3;
const AUTH: u32 = 8;
const SET_CLIENT_NAME: u32 = 9;
const DRAIN_PLAYBACK_STREAM: u32 = 12;
const REQUEST: u32 = 61;
const PLAYBACK_STREAM_KILLED: u32 = 64;

/// The protocol version we speak.
pub const VERSION: u32 = 13;
const VERSION_MASK: u32 = 0x0000_FFFF;
const CONTROL: u32 = u32::MAX;
const INVALID_INDEX: u32 = u32::MAX;
const COOKIE_LENGTH: usize = 256;
/// `PA_SAMPLE_S16LE`.
const S16LE: u8 = 3;
/// `PA_VOLUME_NORM`.
const VOLUME_NORM: u32 = 0x1_0000;
/// `PA_CHANNEL_POSITION_FRONT_LEFT`, `_RIGHT`.
const FRONT_LEFT: u8 = 1;
const FRONT_RIGHT: u8 = 2;

// ------------------------------------------------------------ tagstructs

/// A tagstruct being written.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tags(pub Vec<u8>);

impl Tags {
    /// `'L'` and a big-endian u32.
    pub fn u32(mut self, v: u32) -> Tags {
        self.0.push(b'L');
        self.0.extend(v.to_be_bytes());
        self
    }
    /// `'t'` and the string with its NUL, or `'N'` for none.
    pub fn string(mut self, s: Option<&str>) -> Tags {
        match s {
            Some(s) => {
                self.0.push(b't');
                self.0.extend(s.as_bytes());
                self.0.push(0);
            }
            None => self.0.push(b'N'),
        }
        self
    }
    /// `'1'` or `'0'`.
    pub fn boolean(mut self, b: bool) -> Tags {
        self.0.push(if b { b'1' } else { b'0' });
        self
    }
    /// `'x'`, a u32 length and the bytes.
    pub fn arbitrary(mut self, b: &[u8]) -> Tags {
        self.0.push(b'x');
        self.0.extend((b.len() as u32).to_be_bytes());
        self.0.extend(b);
        self
    }
    /// `'a'`: format, channels, rate.
    pub fn sample_spec(mut self, format: u8, channels: u8, rate: u32) -> Tags {
        self.0.extend([b'a', format, channels]);
        self.0.extend(rate.to_be_bytes());
        self
    }
    /// `'m'`: count, then positions.
    pub fn channel_map(mut self, positions: &[u8]) -> Tags {
        self.0.extend([b'm', positions.len() as u8]);
        self.0.extend(positions);
        self
    }
    /// `'v'`: count, then u32 volumes.
    pub fn cvolume(mut self, volumes: &[u32]) -> Tags {
        self.0.extend([b'v', volumes.len() as u8]);
        for v in volumes {
            self.0.extend(v.to_be_bytes());
        }
        self
    }
    /// `'P'`, then per entry the key, `'L'` length and `'x'` value (string
    /// values carry their NUL), then `'N'`.
    pub fn proplist(mut self, entries: &[(&str, &str)]) -> Tags {
        self.0.push(b'P');
        for (k, v) in entries {
            let mut val = v.as_bytes().to_vec();
            val.push(0);
            self = self.string(Some(k)).u32(val.len() as u32).arbitrary(&val);
        }
        self.string(None)
    }
}

/// Reading a tagstruct.
pub struct Fields<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Fields<'a> {
    /// Over these bytes.
    pub fn new(b: &'a [u8]) -> Fields<'a> {
        Fields { b, at: 0 }
    }
    fn bad() -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, "PulseAudio: malformed reply")
    }
    /// The next field, a u32.
    pub fn u32(&mut self) -> io::Result<u32> {
        let b = self.b.get(self.at..self.at + 5).ok_or_else(Fields::bad)?;
        if b[0] != b'L' {
            return Err(Fields::bad());
        }
        self.at += 5;
        Ok(u32::from_be_bytes([b[1], b[2], b[3], b[4]]))
    }
}

/// A packet: header and payload.
pub fn packet(channel: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(20 + payload.len());
    for w in [payload.len() as u32, channel, 0, 0, 0] {
        p.extend(w.to_be_bytes());
    }
    p.extend(payload);
    p
}

// ------------------------------------------------------------ connection

/// Where the server is: `$PULSE_SERVER`'s first local entry (`unix:PATH`
/// or `PATH`), else `$XDG_RUNTIME_DIR/pulse/native`.
pub fn server_path(pulse_server: Option<&str>, runtime_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(list) = pulse_server {
        for entry in list.split_whitespace() {
            // "{machine-id}unix:/path" names the machine first; skip it.
            let entry = entry.strip_prefix('{').and_then(|e| e.split_once('}')).map_or(entry, |(_, rest)| rest);
            if let Some(p) = entry.strip_prefix("unix:") {
                return Some(PathBuf::from(p));
            }
            if entry.starts_with('/') {
                return Some(PathBuf::from(entry));
            }
        }
        // Only TCP entries: not ours.
        return None;
    }
    runtime_dir.map(|d| d.join("pulse/native"))
}

/// The client cookie: `$PULSE_COOKIE`, `~/.config/pulse/cookie` or
/// `~/.pulse-cookie`; zeros if there is none (PipeWire ignores it, and
/// servers allowing anonymous clients take anything).
fn cookie() -> Vec<u8> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".config")));
    let candidates = [std::env::var_os("PULSE_COOKIE").map(PathBuf::from), config.map(|c| c.join("pulse/cookie")), home.map(|h| h.join(".pulse-cookie"))];
    for p in candidates.into_iter().flatten() {
        if let Ok(c) = std::fs::read(&p) {
            if c.len() >= COOKIE_LENGTH {
                return c[..COOKIE_LENGTH].to_vec();
            }
        }
    }
    vec![0; COOKIE_LENGTH]
}

struct Conn {
    sock: UnixStream,
    tag: u32,
    channel: u32,
    credit: usize,
}

impl Conn {
    fn send(&mut self, channel: u32, payload: &[u8]) -> io::Result<()> {
        self.sock.write_all(&packet(channel, payload))
    }

    fn read_packet(&mut self) -> io::Result<(u32, Vec<u8>)> {
        let mut h = [0u8; 20];
        self.sock.read_exact(&mut h)?;
        let word = |i: usize| u32::from_be_bytes([h[4 * i], h[4 * i + 1], h[4 * i + 2], h[4 * i + 3]]);
        let len = word(0) as usize;
        if len > 16 << 20 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "PulseAudio: oversized packet"));
        }
        let mut body = vec![0u8; len];
        self.sock.read_exact(&mut body)?;
        Ok((word(1), body))
    }

    /// Take one packet the server sent on its own: a request for more
    /// audio adds credit; our stream being killed is an error.
    fn absorb(&mut self, channel: u32, body: &[u8]) -> io::Result<Option<(u32, u32, Vec<u8>)>> {
        if channel != CONTROL {
            return Ok(None); // audio for a record stream: not ours
        }
        let mut f = Fields::new(body);
        let (cmd, tag) = (f.u32()?, f.u32()?);
        match cmd {
            REQUEST => {
                let (ch, bytes) = (f.u32()?, f.u32()?);
                if ch == self.channel {
                    self.credit += bytes as usize;
                }
                Ok(None)
            }
            PLAYBACK_STREAM_KILLED => Err(io::Error::new(io::ErrorKind::BrokenPipe, "PulseAudio: the server ended the stream")),
            REPLY | ERROR => Ok(Some((cmd, tag, body[f.at..].to_vec()))),
            _ => Ok(None), // underflow, started, latency updates…
        }
    }

    /// Send a command and wait for its reply (the payload after the tag).
    fn command(&mut self, cmd: u32, fields: Tags) -> io::Result<Vec<u8>> {
        self.tag = self.tag.wrapping_add(1);
        let tag = self.tag;
        let mut payload = Tags::default().u32(cmd).u32(tag).0;
        payload.extend(fields.0);
        self.send(CONTROL, &payload)?;
        loop {
            let (channel, body) = self.read_packet()?;
            if let Some((kind, t, rest)) = self.absorb(channel, &body)? {
                if t != tag {
                    continue;
                }
                if kind == ERROR {
                    let code = Fields::new(&rest).u32().unwrap_or(0);
                    return Err(io::Error::other(format!("PulseAudio refused (error {code}{})", error_name(code))));
                }
                return Ok(rest);
            }
        }
    }
}

fn error_name(code: u32) -> &'static str {
    match code {
        1 => ": access denied — no valid cookie?",
        3 => ": invalid argument",
        5 => ": no such entity",
        7 => ": protocol error",
        _ => "",
    }
}

/// The audio connection, as the pump writes it.
pub struct Writer {
    conn: Arc<Mutex<Conn>>,
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut c = self.conn.lock().unwrap();
        while c.credit == 0 {
            let (channel, body) = c.read_packet()?;
            c.absorb(channel, &body)?;
        }
        let n = buf.len().min(c.credit).min(64 * 1024);
        let channel = c.channel;
        c.send(channel, &buf[..n])?;
        c.credit -= n;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The player's handle: cut the stream off, or let it play out.
pub struct Control {
    conn: Arc<Mutex<Conn>>,
    sock: Option<UnixStream>,
}

impl Control {
    /// Let the server play what it holds (drain), then hang up — in the
    /// background, as draining takes as long as the buffer.
    pub fn finish(mut self) {
        let sock = self.sock.take();
        let conn = self.conn.clone();
        std::thread::spawn(move || {
            {
                let mut c = conn.lock().unwrap();
                let _ = c.sock.set_read_timeout(Some(Duration::from_secs(5)));
                let channel = c.channel;
                let _ = c.command(DRAIN_PLAYBACK_STREAM, Tags::default().u32(channel));
            }
            if let Some(s) = sock {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
        });
    }
}

impl Drop for Control {
    /// Cut it off: hanging up makes the server drop the stream and what it
    /// had queued, and unblocks a writer waiting for credit.
    fn drop(&mut self) {
        if let Some(s) = self.sock.take() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

/// Connect to the server at `path` and open a 16-bit stereo playback
/// stream at `rate`.
pub fn open(path: &Path, rate: u32) -> io::Result<(Control, Writer)> {
    let what = |e: io::Error| io::Error::new(e.kind(), format!("PulseAudio at {}: {e}", path.display()));
    let sock = UnixStream::connect(path).map_err(what)?;
    // A server that accepts but never answers must not hang playback.
    sock.set_read_timeout(Some(Duration::from_secs(3))).map_err(what)?;
    let mut c = Conn { sock: sock.try_clone().map_err(what)?, tag: 0, channel: 0, credit: 0 };
    let reply = c.command(AUTH, Tags::default().u32(VERSION).arbitrary(&cookie())).map_err(what)?;
    let server = Fields::new(&reply).u32().map_err(what)? & VERSION_MASK;
    if server < 13 {
        return Err(io::Error::new(io::ErrorKind::Unsupported, format!("PulseAudio at {}: protocol {server} is older than 13", path.display())));
    }
    c.command(SET_CLIENT_NAME, Tags::default().proplist(&[("application.name", "phonolite"), ("application.icon_name", "audio-x-generic")])).map_err(what)?;
    // About 0.2 s of buffer at the server.
    let tlength = (rate as usize * 4 / 5) as u32;
    let create = Tags::default()
        .sample_spec(S16LE, 2, rate)
        .channel_map(&[FRONT_LEFT, FRONT_RIGHT])
        .u32(INVALID_INDEX) // the default sink
        .string(None)
        .u32(u32::MAX) // maxlength: the server's choice
        .boolean(false) // not corked
        .u32(tlength)
        .u32(u32::MAX) // prebuf: default (a full buffer)
        .u32(u32::MAX) // minreq: default
        .u32(0) // sync id
        .cvolume(&[VOLUME_NORM, VOLUME_NORM])
        // v12: no_remap, no_remix, fix_format, fix_rate, fix_channels, no_move, variable_rate
        .boolean(false)
        .boolean(false)
        .boolean(false)
        .boolean(false)
        .boolean(false)
        .boolean(false)
        .boolean(false)
        // v13: muted, adjust_latency, properties
        .boolean(false)
        .boolean(true)
        .proplist(&[("media.name", "phonolite"), ("media.role", "music")]);
    let reply = c.command(CREATE_PLAYBACK_STREAM, create).map_err(what)?;
    let mut f = Fields::new(&reply);
    c.channel = f.u32().map_err(what)?;
    let _sink_input = f.u32().map_err(what)?;
    c.credit += f.u32().map_err(what)? as usize;
    // From here a writer may wait for credit as long as playback lasts.
    c.sock.set_read_timeout(None).map_err(what)?;
    let conn = Arc::new(Mutex::new(c));
    Ok((Control { conn: conn.clone(), sock: Some(sock) }, Writer { conn }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn tagstruct_bytes_match_pulseaudios_encoding() {
        // As tagstruct.c writes them.
        assert_eq!(Tags::default().u32(0x0102_0304).0, b"L\x01\x02\x03\x04");
        assert_eq!(Tags::default().string(Some("ab")).string(None).0, b"tab\0N");
        assert_eq!(Tags::default().boolean(true).boolean(false).0, b"10");
        assert_eq!(Tags::default().arbitrary(b"xyz").0, b"x\0\0\0\x03xyz");
        assert_eq!(Tags::default().sample_spec(S16LE, 2, 44100).0, [b'a', 3, 2, 0, 0, 0xAC, 0x44]);
        assert_eq!(Tags::default().channel_map(&[1, 2]).0, [b'm', 2, 1, 2]);
        assert_eq!(Tags::default().cvolume(&[VOLUME_NORM]).0, [b'v', 1, 0, 1, 0, 0]);
        assert_eq!(Tags::default().proplist(&[("k", "v")]).0, b"Ptk\0L\0\0\0\x02x\0\0\0\x02v\0N");
        assert_eq!(&packet(7, b"hi")[..20], [0, 0, 0, 2, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let mut f = Fields::new(b"L\0\0\0\x05L\0\0\0\x06t");
        assert_eq!((f.u32().unwrap(), f.u32().unwrap()), (5, 6));
        assert!(f.u32().is_err());
    }

    #[test]
    fn finding_the_server() {
        let rt = Path::new("/run/user/1000");
        assert_eq!(server_path(None, Some(rt)), Some(PathBuf::from("/run/user/1000/pulse/native")));
        assert_eq!(server_path(Some("unix:/mnt/wslg/PulseServer"), Some(rt)), Some(PathBuf::from("/mnt/wslg/PulseServer")), "WSLg");
        assert_eq!(server_path(Some("tcp:host:4713 /tmp/p"), None), Some(PathBuf::from("/tmp/p")), "the first local entry");
        assert_eq!(server_path(Some("{abc}unix:/x/native"), None), Some(PathBuf::from("/x/native")));
        assert_eq!(server_path(Some("tcp:host"), Some(rt)), None, "TCP only: not for us");
        assert_eq!(server_path(None, None), None);
    }

    /// A pretend server, scripted from protocol-native.c's side: it checks
    /// what the client sends, grants credit in small requests, and
    /// collects the audio.
    fn fake_server(path: PathBuf, grant: u32, reject_auth: bool) -> std::thread::JoinHandle<(Vec<Vec<u8>>, Vec<u8>)> {
        let listener = UnixListener::bind(path).unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut commands = Vec::new();
            let mut audio = Vec::new();
            let mut credit = grant as usize;
            let read = |s: &mut UnixStream| -> Option<(u32, Vec<u8>)> {
                let mut h = [0u8; 20];
                s.read_exact(&mut h).ok()?;
                let len = u32::from_be_bytes(h[0..4].try_into().unwrap()) as usize;
                let ch = u32::from_be_bytes(h[4..8].try_into().unwrap());
                let mut b = vec![0; len];
                s.read_exact(&mut b).ok()?;
                Some((ch, b))
            };
            while let Some((ch, body)) = read(&mut s) {
                if ch != CONTROL {
                    assert_eq!(ch, 9, "audio on the stream's channel");
                    assert!(body.len() <= credit, "sent {} bytes with {credit} granted", body.len());
                    credit -= body.len();
                    audio.extend(&body);
                    credit += body.len();
                    // Grant more as it is used.
                    let req = Tags::default().u32(REQUEST).u32(u32::MAX).u32(9).u32(body.len() as u32);
                    s.write_all(&packet(CONTROL, &req.0)).unwrap();
                    continue;
                }
                let mut f = Fields::new(&body);
                let (cmd, tag) = (f.u32().unwrap(), f.u32().unwrap());
                commands.push(body.clone());
                let reply = match cmd {
                    AUTH if reject_auth => Tags::default().u32(ERROR).u32(tag).u32(1),
                    AUTH => {
                        assert_eq!(f.u32().unwrap(), VERSION);
                        Tags::default().u32(REPLY).u32(tag).u32(35 | 0x8000_0000)
                    }
                    SET_CLIENT_NAME => Tags::default().u32(REPLY).u32(tag).u32(42),
                    CREATE_PLAYBACK_STREAM => Tags::default().u32(REPLY).u32(tag).u32(9).u32(3).u32(grant),
                    DRAIN_PLAYBACK_STREAM => Tags::default().u32(REPLY).u32(tag),
                    other => panic!("unexpected command {other}"),
                };
                s.write_all(&packet(CONTROL, &reply.0)).unwrap();
            }
            (commands, audio)
        })
    }

    #[test]
    fn handshake_stream_and_flow_control() {
        let d = std::env::temp_dir().join(format!("phonolite-pulse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("native");
        let server = fake_server(path.clone(), 1000, false);
        let (control, mut w) = open(&path, 44100).unwrap();
        // More than the first grant: the rest waits for REQUESTs.
        let data: Vec<u8> = (0..10_000u32).map(|i| i as u8).collect();
        w.write_all(&data).unwrap();
        drop(w);
        control.finish();
        let (commands, audio) = server.join().unwrap();
        assert_eq!(audio, data, "all of it, in order");
        let create = commands.iter().find(|c| Fields::new(c).u32().ok() == Some(CREATE_PLAYBACK_STREAM)).unwrap();
        // After command and tag: sample spec s16le, 2 channels, 44100 Hz.
        assert_eq!(&create[10..17], [b'a', 3, 2, 0, 0, 0xAC, 0x44]);
        assert!(create.windows(10).any(|w| w == b"media.name"));
        assert!(commands.iter().any(|c| Fields::new(c).u32().ok() == Some(DRAIN_PLAYBACK_STREAM)), "drained at the end");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn refusal_and_absence_are_errors_that_say_so() {
        let d = std::env::temp_dir().join(format!("phonolite-pulse-no-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("native");
        let server = fake_server(path.clone(), 0, true);
        let e = open(&path, 44100).err().unwrap().to_string();
        assert!(e.contains("access denied"), "{e}");
        drop(server);
        let e = open(&d.join("missing"), 44100).err().unwrap().to_string();
        assert!(e.contains("PulseAudio at") && e.contains("missing"), "{e}");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
