//! mpg123's generic remote-control protocol (`mpg123 -R`): the lines it
//! prints, parsed into [`Message`]s, and the commands it reads.
//!
//! Pure text in, values out — no processes here (see [`crate::engine`]).

use crate::num::{fixed, parse_decimal};

/// What mpg123 is doing, from its `@P` lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    /// `@P 0`: nothing loaded (stopped, or after the end of a track).
    Stopped,
    /// `@P 1`.
    Paused,
    /// `@P 2`.
    Playing,
    /// `@P 3`: the track played to its end (a `@P 0` follows).
    Ended,
}

/// The stream's format, from the `@S` line after a track loads.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamInfo {
    /// "1.0", "2.0" or "2.5".
    pub version: String,
    /// 1, 2 or 3.
    pub layer: u8,
    /// Sampling rate in Hz.
    pub rate: u32,
    /// "Stereo", "Joint-Stereo", "Dual-Channel" or "Single-Channel".
    pub mode: String,
    /// 1 or 2.
    pub channels: u8,
    /// Bitrate in kbit/s (of the first frame, for VBR).
    pub bitrate: u32,
    /// Variable bitrate.
    pub vbr: bool,
}

impl StreamInfo {
    /// Samples per channel in one frame: 384 for layer I, 1152 for layer
    /// II, 1152 or 576 (MPEG 2/2.5) for layer III.
    pub fn samples_per_frame(&self) -> u32 {
        match (self.layer, self.version.as_str()) {
            (1, _) => 384,
            (3, "2.0" | "2.5") => 576,
            _ => 1152,
        }
    }
}

/// Playback position, from the `@F` lines mpg123 prints every frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    /// Frames decoded.
    pub frame: u64,
    /// Frames left.
    pub frames_left: u64,
    /// Seconds decoded.
    pub seconds: f64,
    /// Seconds left.
    pub seconds_left: f64,
}

/// One line from mpg123, parsed.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// `@R MPG123 …`: the remote interface is up.
    Ready(String),
    /// A metadata field: `("title", "Song")`, `("artist", …)`, `("album", …)`,
    /// `("year", …)`, `("comment", …)`, `("genre", …)`, `("track", …)`, and
    /// for streams `("icy-name", …)`, `("icy-url", …)`, `("stream-title", …)`.
    Tag(String, String),
    /// `@I <name>`: the loaded track has no tags; mpg123 names it after
    /// the file.
    Name(String),
    /// `@S …`.
    Stream(StreamInfo),
    /// `@F …`.
    Progress(Progress),
    /// `@P …`.
    State(PlayState),
    /// `@J <frame>`: a jump landed.
    Jumped(u64),
    /// `@V <percent>%`: the volume was set.
    Volume(f64),
    /// `@E …`: an error (a file that will not open, a bad command).
    Error(String),
    /// Anything else (`@H` help, `@T` tag dumps, EQ echoes, blank `@`).
    Other(String),
}

/// Parse one line (without its newline).
pub fn parse(line: &str) -> Message {
    let line = line.trim_end_matches(['\r', '\n']);
    let Some(rest) = line.strip_prefix('@') else { return Message::Other(line.into()) };
    let (code, body) = match rest.split_once(' ') {
        Some((c, b)) => (c, b),
        None => (rest, ""),
    };
    let other = || Message::Other(line.into());
    match code {
        "R" => Message::Ready(body.into()),
        "I" => parse_info(body).unwrap_or_else(other),
        "S" => parse_stream(body).map(Message::Stream).unwrap_or_else(other),
        "F" => parse_progress(body).map(Message::Progress).unwrap_or_else(other),
        "P" => match body.trim() {
            "0" => Message::State(PlayState::Stopped),
            "1" => Message::State(PlayState::Paused),
            "2" => Message::State(PlayState::Playing),
            "3" => Message::State(PlayState::Ended),
            _ => other(),
        },
        "J" => body.trim().parse().map(Message::Jumped).unwrap_or_else(|_| other()),
        "V" => parse_decimal(body.trim().trim_end_matches('%')).map(Message::Volume).unwrap_or_else(other),
        "E" => Message::Error(body.into()),
        _ => other(),
    }
}

fn parse_info(body: &str) -> Option<Message> {
    if body == "{" || body == "}" {
        return Some(Message::Other(format!("@I {body}")));
    }
    if let Some(v1) = body.strip_prefix("ID3:") {
        // Fixed width in bytes of UTF-8 (mpg123 converts, then pads):
        // title, artist, album (30 each), year (4), comment (30), then the
        // genre's name. A boundary inside a character moves past it.
        let cut = |i: usize| {
            let mut i = i.min(v1.len());
            while !v1.is_char_boundary(i) {
                i += 1;
            }
            i
        };
        let field = |a: usize, b: usize| v1[cut(a)..cut(b).max(cut(a))].trim().to_string();
        // One message per field would need a Vec; v1 arrives as a single
        // `Tag("id3v1", …)` with the fields joined, split by `id3v1_fields`.
        let joined = [field(0, 30), field(30, 60), field(60, 90), field(90, 94), field(94, 124), field(124, usize::MAX)].join("\u{1f}");
        return Some(Message::Tag("id3v1".into(), joined));
    }
    if let Some(kv) = body.strip_prefix("ID3v2.").or_else(|| body.strip_prefix("ID3.")) {
        let (k, v) = kv.split_once(':')?;
        return Some(Message::Tag(k.to_ascii_lowercase(), v.to_string()));
    }
    if let Some(meta) = body.strip_prefix("ICY-META:") {
        let meta = meta.trim();
        let title = meta
            .strip_prefix("StreamTitle='")
            .and_then(|t| t.split_once("';"))
            .map(|(t, _)| t)
            .or_else(|| meta.strip_prefix("StreamTitle='")?.strip_suffix('\''))?;
        return Some(Message::Tag("stream-title".into(), title.to_string()));
    }
    if let Some(name) = body.strip_prefix("ICY-NAME:") {
        return Some(Message::Tag("icy-name".into(), name.trim().to_string()));
    }
    if let Some(url) = body.strip_prefix("ICY-URL:") {
        return Some(Message::Tag("icy-url".into(), url.trim().to_string()));
    }
    Some(Message::Name(body.to_string()))
}

/// The fields of an `("id3v1", …)` tag: title, artist, album, year,
/// comment, genre, each trimmed (empty when absent).
pub fn id3v1_fields(value: &str) -> [&str; 6] {
    let mut out = [""; 6];
    for (slot, f) in out.iter_mut().zip(value.split('\u{1f}')) {
        *slot = f;
    }
    out
}

fn parse_stream(body: &str) -> Option<StreamInfo> {
    let f: Vec<&str> = body.split_whitespace().collect();
    if f.len() < 11 {
        return None;
    }
    Some(StreamInfo {
        version: f[0].into(),
        layer: f[1].parse().ok()?,
        rate: f[2].parse().ok()?,
        mode: f[3].into(),
        channels: f[6].parse().ok()?,
        bitrate: f[10].parse().ok()?,
        vbr: f.get(12).is_some_and(|v| *v != "0"),
    })
}

fn parse_progress(body: &str) -> Option<Progress> {
    let f: Vec<&str> = body.split_whitespace().collect();
    if f.len() != 4 {
        return None;
    }
    Some(Progress { frame: f[0].parse().ok()?, frames_left: f[1].parse().ok()?, seconds: parse_decimal(f[2])?, seconds_left: parse_decimal(f[3])? })
}

/// A command for mpg123, rendered by [`Command::line`].
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Load and play a file or URL.
    Load(String),
    /// Load paused.
    LoadPaused(String),
    /// Toggle pause.
    Pause,
    /// Stop and close the file.
    Stop,
    /// Jump to an absolute position in seconds.
    JumpTo(f64),
    /// Volume in percent (100 is unity gain).
    Volume(f64),
    /// One equalizer value: channel (1 left, 2 right, 3 both), band
    /// `0..32` (mpg123's subbands, not octaves), linear factor.
    Eq(u8, u8, f64),
    /// Stop printing `@F` lines.
    Silence,
    /// Resume `@F` lines.
    Progress,
    /// Exit.
    Quit,
}

impl Command {
    /// The line to write to mpg123's stdin, newline included. `None` for a
    /// path that cannot be sent: mpg123 reads one command per line, so a
    /// name containing a newline would split into two commands.
    pub fn line(&self) -> Option<String> {
        let s = match self {
            Command::Load(p) | Command::LoadPaused(p) if p.contains(['\n', '\r']) => return None,
            Command::Load(p) => format!("LOAD {p}"),
            Command::LoadPaused(p) => format!("LOADPAUSED {p}"),
            Command::Pause => "PAUSE".into(),
            Command::Stop => "STOP".into(),
            Command::JumpTo(s) => format!("JUMP {}s", fixed(s.max(0.0), 3)),
            Command::Volume(v) => format!("VOLUME {}", fixed(v.max(0.0), 1)),
            Command::Eq(c, b, v) => format!("EQ {c} {b} {}", fixed(v.max(0.0), 4)),
            Command::Silence => "SILENCE".into(),
            Command::Progress => "PROGRESS".into(),
            Command::Quit => "QUIT".into(),
        };
        Some(s + "\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Lines below are copied from mpg123 1.32.10's output (see engine tests
    // for the live run).

    #[test]
    fn ready_state_jump_volume_error() {
        assert_eq!(parse("@R MPG123 (ThOr) v11"), Message::Ready("MPG123 (ThOr) v11".into()));
        assert_eq!(parse("@P 0"), Message::State(PlayState::Stopped));
        assert_eq!(parse("@P 1"), Message::State(PlayState::Paused));
        assert_eq!(parse("@P 2"), Message::State(PlayState::Playing));
        assert_eq!(parse("@P 3\n"), Message::State(PlayState::Ended));
        assert_eq!(parse("@P 9"), Message::Other("@P 9".into()));
        assert_eq!(parse("@J 250"), Message::Jumped(250));
        assert_eq!(parse("@V 50.000000%"), Message::Volume(50.0));
        assert_eq!(parse("@E No stream opened. (code 24)"), Message::Error("No stream opened. (code 24)".into()));
        assert_eq!(parse("@"), Message::Other("@".into()));
        assert_eq!(parse("junk"), Message::Other("junk".into()));
        assert_eq!(parse(""), Message::Other("".into()));
    }

    #[test]
    fn stream_info() {
        let Message::Stream(s) = parse("@S 1.0 3 44100 Joint-Stereo 2 417 2 0 0 0 128 0 0") else { panic!() };
        assert_eq!(s, StreamInfo { version: "1.0".into(), layer: 3, rate: 44100, mode: "Joint-Stereo".into(), channels: 2, bitrate: 128, vbr: false });
        assert_eq!(s.samples_per_frame(), 1152);
        let Message::Stream(s) = parse("@S 2.0 3 22050 Single-Channel 0 208 1 0 0 0 64 0 1") else { panic!() };
        assert_eq!((s.channels, s.vbr, s.samples_per_frame()), (1, true, 576));
        let Message::Stream(s) = parse("@S 1.0 1 48000 Stereo 0 384 2 0 0 0 384 0 0") else { panic!() };
        assert_eq!(s.samples_per_frame(), 384);
        assert!(matches!(parse("@S 1.0 3 x"), Message::Other(_)));
    }

    #[test]
    fn progress() {
        assert_eq!(parse("@F 253 122 2.02 0.98"), Message::Progress(Progress { frame: 253, frames_left: 122, seconds: 2.02, seconds_left: 0.98 }));
        assert!(matches!(parse("@F 1 2 3"), Message::Other(_)));
        assert!(matches!(parse("@F a 2 3 4"), Message::Other(_)));
    }

    #[test]
    fn tags() {
        assert_eq!(parse("@I ID3v2.title:Quiet Song"), Message::Tag("title".into(), "Quiet Song".into()));
        assert_eq!(parse("@I ID3v2.artist:A: B"), Message::Tag("artist".into(), "A: B".into()));
        assert_eq!(parse("@I ID3.genre:17"), Message::Tag("genre".into(), "17".into()));
        assert_eq!(parse("@I ID3.track:7"), Message::Tag("track".into(), "7".into()));
        assert_eq!(parse("@I tone3"), Message::Name("tone3".into()));
        assert!(matches!(parse("@I {"), Message::Other(_)));
        assert!(matches!(parse("@I }"), Message::Other(_)));
        assert_eq!(parse("@I ICY-NAME: Radio X"), Message::Tag("icy-name".into(), "Radio X".into()));
        assert_eq!(parse("@I ICY-URL: http://x/"), Message::Tag("icy-url".into(), "http://x/".into()));
        assert_eq!(parse("@I ICY-META: StreamTitle='Band - It''s';StreamUrl='';"), Message::Tag("stream-title".into(), "Band - It''s".into()));
        assert_eq!(parse("@I ICY-META: StreamTitle='No end'"), Message::Tag("stream-title".into(), "No end".into()));
    }

    #[test]
    fn id3v1_is_fixed_width_in_bytes() {
        // Exactly as mpg123 printed it for a generated tag; the title held a
        // Latin-1 byte, which arrives as U+FFFD — three bytes, padded so the
        // field is still 30 bytes.
        let line =
            "@I ID3:Title \u{fffd}                     Artist                        Album                         1999Comment                       Rock";
        let Message::Tag(k, v) = parse(line) else { panic!() };
        assert_eq!(k, "id3v1");
        assert_eq!(id3v1_fields(&v), ["Title \u{fffd}", "Artist", "Album", "1999", "Comment", "Rock"]);
        let Message::Tag(_, v) = parse("@I ID3:Short") else { panic!() };
        assert_eq!(id3v1_fields(&v), ["Short", "", "", "", "", ""]);
        // A field boundary inside a character does not panic.
        let Message::Tag(_, v) = parse(&format!("@I ID3:{}é{}", "x".repeat(29), "y".repeat(40))) else { panic!() };
        assert_eq!(id3v1_fields(&v)[0], format!("{}é", "x".repeat(29)));
    }

    #[test]
    fn commands() {
        assert_eq!(Command::Load("/a b/c.mp3".into()).line().unwrap(), "LOAD /a b/c.mp3\n");
        assert_eq!(Command::LoadPaused("x".into()).line().unwrap(), "LOADPAUSED x\n");
        assert_eq!(Command::Load("evil\nQUIT".into()).line(), None);
        assert_eq!(Command::JumpTo(12.3456).line().unwrap(), "JUMP 12.346s\n");
        assert_eq!(Command::JumpTo(-1.0).line().unwrap(), "JUMP 0.000s\n");
        assert_eq!(Command::Volume(55.56).line().unwrap(), "VOLUME 55.6\n");
        assert_eq!(Command::Eq(3, 31, 1.5).line().unwrap(), "EQ 3 31 1.5000\n");
        for (c, s) in
            [(Command::Pause, "PAUSE"), (Command::Stop, "STOP"), (Command::Silence, "SILENCE"), (Command::Progress, "PROGRESS"), (Command::Quit, "QUIT")]
        {
            assert_eq!(c.line().unwrap(), format!("{s}\n"));
        }
    }
}
