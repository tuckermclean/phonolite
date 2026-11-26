//! Playback: one mpg123 per track decoding into the [`Audio`] pump, the
//! sink process the pump writes to, and the settings that go with them.
//!
//! A track that plays to its end has the next one queued behind it (the
//! application says which, on [`Update::NeedNext`]), so the music runs on
//! without a gap. Anything the user does — play, skip, seek, stop — flushes
//! instead: decoders and sink are replaced, and nothing stale is heard.

use std::io::{self, BufRead, BufReader};
use std::process::{Child, Command as Process, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::audio::{Audio, Snapshot, UNITY};
use crate::builtin;
use crate::engine::{Engine, Mpg123};
use crate::eq::Equalizer;
use crate::remote::{id3v1_fields, Command, Message, PlayState, StreamInfo};

/// Transport state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Nothing playing.
    Stopped,
    /// Loaded, paused.
    Paused,
    /// Playing.
    Playing,
}

/// Metadata of a track.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tags {
    /// Title.
    pub title: Option<String>,
    /// Artist.
    pub artist: Option<String>,
    /// Album.
    pub album: Option<String>,
    /// Year.
    pub year: Option<String>,
    /// Genre.
    pub genre: Option<String>,
    /// A stream's current song (ICY metadata).
    pub stream_title: Option<String>,
    /// A stream's station name.
    pub station: Option<String>,
}

impl Tags {
    /// "Artist - Title", or whichever exists; streams show their current
    /// song, else the station.
    pub fn display(&self) -> Option<String> {
        if let Some(s) = &self.stream_title {
            return Some(s.clone());
        }
        match (&self.artist, &self.title) {
            (Some(a), Some(t)) => Some(format!("{a} - {t}")),
            (None, Some(t)) => Some(t.clone()),
            _ => self.station.clone(),
        }
    }

    fn set(&mut self, key: &str, value: &str) {
        let v = value.trim();
        if v.is_empty() {
            return;
        }
        let slot = match key {
            "title" => &mut self.title,
            "artist" => &mut self.artist,
            "album" => &mut self.album,
            "year" | "date" => &mut self.year,
            "genre" => &mut self.genre,
            "stream-title" => &mut self.stream_title,
            "icy-name" => &mut self.station,
            _ => return,
        };
        *slot = Some(v.to_string());
    }

    /// Take one `@I` tag message. ID3v1 fields fill only what ID3v2 left
    /// empty (mpg123 reports both when a file has both).
    pub fn apply(&mut self, key: &str, value: &str) {
        if key != "id3v1" {
            self.set(key, value);
            return;
        }
        let [title, artist, album, year, _, genre] = id3v1_fields(value);
        for (k, v) in [("title", title), ("artist", artist), ("album", album), ("year", year), ("genre", genre)] {
            let empty = match k {
                "title" => self.title.is_none(),
                "artist" => self.artist.is_none(),
                "album" => self.album.is_none(),
                "year" => self.year.is_none(),
                _ => self.genre.is_none(),
            };
            if empty {
                self.set(k, v);
            }
        }
    }
}

/// A track as mpg123 described it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    /// What was loaded.
    pub path: String,
    /// Its metadata.
    pub tags: Tags,
    /// Its format, once known.
    pub stream: Option<StreamInfo>,
    /// Its length in seconds, once known.
    pub duration: Option<f64>,
}

/// Something the application should react to.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// The track tagged so is now the one playing (heard, not just decoded).
    TrackChanged(u64),
    /// Tags, format or length of the playing track changed.
    Info,
    /// The transport state changed.
    Status,
    /// The last queued track has been decoded to its end: queue the next
    /// with [`Player::enqueue`] now for gapless playback, or let it run out.
    NeedNext(u64),
    /// Everything played; stopped.
    Ended,
    /// mpg123 or the sink complained.
    Error(String),
}

/// Volume slider position (`0..=100`) to gain in percent. The square makes
/// the slider's travel even to the ear: halfway is a quarter of the
/// amplitude, -12 dB.
pub fn volume_percent(slider: u32) -> f64 {
    let s = f64::from(slider.min(100));
    s * s / 100.0
}

/// 16.16 gains for left and right from volume and balance (`-100..=100`;
/// the louder side stays at the volume).
pub fn gains(volume: u32, balance: i32) -> [i32; 2] {
    let v = volume_percent(volume) / 100.0;
    let b = f64::from(balance.clamp(-100, 100)) / 100.0;
    [(1.0 - b).min(1.0), (1.0 + b).min(1.0)].map(|s| (v * s * f64::from(UNITY)).round() as i32)
}

/// How to run the sound sink: a program reading raw 44.1 kHz stereo s16le
/// on stdin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinkCommand {
    /// The program.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
}

impl SinkCommand {
    /// `out123` (installed with mpg123) told the format at `rate` Hz,
    /// plus `extra` (e.g. `["-o", "pulse"]`).
    pub fn out123(extra: &[String], rate: u32) -> SinkCommand {
        let mut args: Vec<String> = extra.to_vec();
        args.extend(["-c", "2", "-e", "s16", "-r"].map(String::from));
        args.push(rate.to_string());
        SinkCommand { program: "out123".into(), args }
    }
}

/// Where the sound goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// A program reading raw samples on stdin (out123, aplay, …).
    Command(SinkCommand),
    /// Straight to an OSS device (`/dev/dsp`), at this rate.
    Oss(std::path::PathBuf),
}

impl Output {
    /// The usual choice: through out123 when a sound server is running
    /// (the device belongs to it, and it mixes); else straight to
    /// `/dev/dsp` if there is one (a bare machine: the 486); else out123
    /// (alsa-lib, perhaps with dmix).
    pub fn choose(rate: u32) -> Output {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
        let server = runtime.is_some_and(|d| d.join("pulse/native").exists() || d.join("pipewire-0").exists());
        let out123 =
            Process::new("out123").arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        let dsp = std::path::Path::new("/dev/dsp");
        Output::pick(server, out123, crate::oss::exists(dsp), rate)
    }

    /// [`Output::choose`]'s rule, given what it found.
    pub fn pick(sound_server: bool, out123: bool, dsp: bool, rate: u32) -> Output {
        if (sound_server || !dsp) && out123 {
            Output::Command(SinkCommand::out123(&[], rate))
        } else {
            Output::Oss("/dev/dsp".into())
        }
    }
}

impl From<SinkCommand> for Output {
    fn from(c: SinkCommand) -> Output {
        Output::Command(c)
    }
}

/// The player's handle on the running output.
enum SinkHandle {
    Process(SinkProcess),
    Oss(crate::oss::Control),
}

impl SinkHandle {
    fn open(out: &Output, rate: u32) -> io::Result<(SinkHandle, crate::audio::Sink)> {
        match out {
            Output::Command(cmd) => {
                let (p, stdin) = SinkProcess::spawn(cmd)?;
                Ok((SinkHandle::Process(p), Box::new(stdin)))
            }
            Output::Oss(path) => {
                let (c, f) = crate::oss::open(path, rate)?;
                Ok((SinkHandle::Oss(c), Box::new(f)))
            }
        }
    }

    /// Let it play out what it holds.
    fn finish(self) {
        match self {
            SinkHandle::Process(p) => p.finish(),
            SinkHandle::Oss(c) => c.finish(),
        }
    }

    fn complaint(&self) -> String {
        match self {
            SinkHandle::Process(p) => p.stderr.lock().unwrap().clone(),
            SinkHandle::Oss(_) => String::new(),
        }
    }
}

struct SinkProcess {
    child: Option<Child>,
    stderr: Arc<Mutex<String>>,
}

impl SinkProcess {
    fn spawn(cmd: &SinkCommand) -> io::Result<(SinkProcess, std::process::ChildStdin)> {
        let mut child = Process::new(&cmd.program)
            .args(&cmd.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| io::Error::new(e.kind(), format!("cannot run {}: {e}", cmd.program)))?;
        let stdin = child.stdin.take().expect("piped");
        let stderr = Arc::new(Mutex::new(String::new()));
        if let Some(pipe) = child.stderr.take() {
            let tail = stderr.clone();
            thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    if !line.trim().is_empty() {
                        *tail.lock().unwrap() = line.trim().to_string();
                    }
                }
            });
        }
        Ok((SinkProcess { child: Some(child), stderr }, stdin))
    }

    /// Let it play out what it holds and exit (once the pump has closed
    /// its input); reap it in the background.
    fn finish(mut self) {
        if let Some(mut child) = self.child.take() {
            thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

impl Drop for SinkProcess {
    /// Cut it off: whatever it had queued is never heard.
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// What decodes: mpg123, or the built-in decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoding {
    /// Run mpg123 (one process per track).
    Mpg123(Mpg123),
    /// Decode in-process, at this sample rate.
    Builtin {
        /// Output rate.
        rate: u32,
    },
}

impl Decoding {
    /// mpg123 if it runs, else the built-in decoder at the same rate.
    pub fn prefer_mpg123(m: Mpg123) -> Decoding {
        let runs =
            Process::new(&m.program).arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        if runs {
            Decoding::Mpg123(m)
        } else {
            Decoding::Builtin { rate: m.rate }
        }
    }

    /// The output sample rate.
    pub fn rate(&self) -> u32 {
        match self {
            Decoding::Mpg123(m) => m.rate,
            Decoding::Builtin { rate } => *rate,
        }
    }
}

/// One track's decoder, whichever kind.
enum Backend {
    Mpg123(Engine),
    Builtin(builtin::Engine),
}

impl Backend {
    fn send(&mut self, c: &Command) -> io::Result<()> {
        match self {
            Backend::Mpg123(e) => e.send(c),
            Backend::Builtin(e) => e.send(c),
        }
    }
    fn try_recv(&self) -> Option<Message> {
        match self {
            Backend::Mpg123(e) => e.try_recv(),
            Backend::Builtin(e) => e.try_recv(),
        }
    }
    fn kill(&mut self) {
        match self {
            Backend::Mpg123(e) => e.kill(),
            Backend::Builtin(e) => e.kill(),
        }
    }
    fn finish(&mut self) {
        match self {
            Backend::Mpg123(e) => e.finish(),
            Backend::Builtin(e) => e.finish(),
        }
    }
}

struct Segment {
    id: u64,
    tag: u64,
    engine: Backend,
    track: Track,
    /// Seconds into the track where this segment's audio begins.
    offset: f64,
    /// Decoded to its end (or failed): mpg123 was told to quit.
    ended: bool,
    /// The equalizer as last sent, and the rate it was mapped for.
    sent_eq: Option<([[f64; 32]; 2], u32)>,
}

/// The player.
pub struct Player {
    decoding: Decoding,
    output: Output,
    notify: Arc<dyn Fn() + Send + Sync>,
    audio: Audio,
    sink: Option<SinkHandle>,
    segments: Vec<Segment>,
    next_id: u64,
    status: Status,
    shown: Option<u64>,
    volume: u32,
    balance: i32,
    eq: Equalizer,
}

impl Player {
    /// A player; processes start on first use.
    pub fn new(decoding: Decoding, output: impl Into<Output>, notify: Arc<dyn Fn() + Send + Sync>) -> Player {
        let n = notify.clone();
        let audio = Audio::new(decoding.rate(), Box::new(move || n()));
        let mut p = Player {
            decoding,
            output: output.into(),
            notify,
            audio,
            sink: None,
            segments: Vec::new(),
            next_id: 1,
            status: Status::Stopped,
            shown: None,
            volume: 75,
            balance: 0,
            eq: Equalizer::default(),
        };
        p.apply_gain();
        p
    }

    /// Transport state.
    pub fn status(&self) -> Status {
        self.status
    }

    fn apply_gain(&mut self) {
        let [l, r] = gains(self.volume, self.balance);
        self.audio.set_gain(l, r);
    }

    fn start_sink(&mut self) -> io::Result<()> {
        let (handle, writer) = SinkHandle::open(&self.output, self.decoding.rate())?;
        self.sink = Some(handle);
        self.audio.set_sink(writer);
        Ok(())
    }

    /// Drop everything: decoders, the sink, all that was not yet heard.
    fn flush(&mut self) {
        self.audio.flush();
        self.sink = None; // killed: whatever it had queued is gone
        for s in &mut self.segments {
            s.engine.kill();
        }
        self.segments.clear();
        self.shown = None;
    }

    fn send_eq(eq: &Equalizer, seg: &mut Segment) -> io::Result<()> {
        let rate = seg.track.stream.as_ref().map_or(44100, |s| s.rate);
        let f = eq.factors(0, rate);
        let before = seg.sent_eq.map(|(v, _)| v);
        if before.is_none() && f.iter().flatten().all(|&v| v == 1.0) {
            seg.sent_eq = Some((f, rate)); // mpg123 starts flat
            return Ok(());
        }
        for (ch, bands) in f.iter().enumerate() {
            for (b, &v) in bands.iter().enumerate() {
                if before.map(|x| x[ch][b]) != Some(v) {
                    seg.engine.send(&Command::Eq(ch as u8 + 1, b as u8, v))?;
                }
            }
        }
        seg.sent_eq = Some((f, rate));
        Ok(())
    }

    fn spawn_segment(&mut self, path: &str, tag: u64, offset: f64, keep: Option<Track>) -> io::Result<()> {
        let n = self.notify.clone();
        let id = self.next_id;
        self.next_id += 1;
        let track = keep.unwrap_or_else(|| Track { path: path.into(), ..Track::default() });
        let audio: crate::audio::Source = match &self.decoding {
            Decoding::Mpg123(cfg) => {
                let mut engine = Engine::spawn(cfg, Box::new(move || n()))?;
                let audio = engine.take_audio().ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "no audio pipe"))?;
                let mut seg = Segment { id, tag, engine: Backend::Mpg123(engine), track, offset, ended: false, sent_eq: None };
                Self::send_eq(&self.eq, &mut seg)?;
                if offset > 0.0 {
                    // Loaded paused, so nothing before the jump reaches the pipe.
                    seg.engine.send(&Command::LoadPaused(path.into()))?;
                    seg.engine.send(&Command::JumpTo(offset))?;
                    seg.engine.send(&Command::Pause)?;
                } else {
                    seg.engine.send(&Command::Load(path.into()))?;
                }
                self.segments.push(seg);
                Box::new(audio)
            }
            Decoding::Builtin { rate } => {
                // Its equalizer starts set, so not even the first frame is flat.
                let f = self.eq.factors(0, 44100);
                let (engine, audio) = builtin::Engine::start(path, offset, *rate, f, Box::new(move || n()))?;
                self.segments.push(Segment { id, tag, engine: Backend::Builtin(engine), track, offset, ended: false, sent_eq: Some((f, 44100)) });
                Box::new(audio)
            }
        };
        self.audio.queue(id, audio);
        Ok(())
    }

    /// Play `path` now; `tag` comes back in [`Update::TrackChanged`].
    pub fn play(&mut self, path: &str, tag: u64) -> io::Result<()> {
        self.flush();
        self.status = Status::Stopped;
        self.spawn_segment(path, tag, 0.0, None)?;
        self.start_sink()?;
        self.status = Status::Playing;
        Ok(())
    }

    /// Queue `path` to follow what is playing, without a gap.
    pub fn enqueue(&mut self, path: &str, tag: u64) -> io::Result<()> {
        if self.status == Status::Stopped {
            return self.play(path, tag);
        }
        self.spawn_segment(path, tag, 0.0, None)
    }

    /// Pause or resume. Does nothing when stopped.
    pub fn toggle_pause(&mut self) -> io::Result<()> {
        match self.status {
            Status::Stopped => {}
            Status::Playing => {
                self.audio.pause();
                self.sink = None;
                self.status = Status::Paused;
            }
            Status::Paused => {
                let (handle, writer) = SinkHandle::open(&self.output, self.decoding.rate())?;
                self.sink = Some(handle);
                self.audio.resume(writer);
                self.status = Status::Playing;
            }
        }
        Ok(())
    }

    /// Stop.
    pub fn stop(&mut self) {
        self.flush();
        self.status = Status::Stopped;
    }

    /// Jump to `seconds` into the playing track.
    pub fn seek(&mut self, seconds: f64) -> io::Result<()> {
        let Some(i) = self.audible_index(&self.audio.snapshot()) else { return Ok(()) };
        let (track, tag) = (self.segments[i].track.clone(), self.segments[i].tag);
        let to = match track.duration {
            Some(d) => seconds.clamp(0.0, (d - 0.1).max(0.0)),
            None => seconds.max(0.0),
        };
        let paused = self.status == Status::Paused;
        self.flush();
        let path = track.path.clone();
        self.spawn_segment(&path, tag, to, Some(track))?;
        self.shown = Some(self.segments[0].id);
        self.start_sink()?;
        self.status = Status::Playing;
        if paused {
            self.toggle_pause()?;
        }
        Ok(())
    }

    /// The segment being heard; before the pump has begun any, the first
    /// (it is about to be).
    fn audible_index(&self, snap: &Snapshot) -> Option<usize> {
        let heard = snap.starts.iter().filter(|&&(_, at)| at <= snap.audible).max_by_key(|&&(_, at)| at).map(|&(id, _)| id);
        match heard {
            Some(id) => self.segments.iter().position(|s| s.id == id),
            None => (!self.segments.is_empty()).then_some(0),
        }
    }

    /// The playing track and its tag.
    pub fn current(&self) -> Option<(&Track, u64)> {
        let i = self.audible_index(&self.audio.snapshot())?;
        Some((&self.segments[i].track, self.segments[i].tag))
    }

    /// Seconds into the playing track (what is heard, not what is decoded).
    pub fn position(&self) -> f64 {
        let snap = self.audio.snapshot();
        let Some(i) = self.audible_index(&snap) else { return 0.0 };
        let seg = &self.segments[i];
        let heard = snap.starts.iter().find(|&&(id, _)| id == seg.id).map_or(0, |&(_, at)| snap.audible.saturating_sub(at));
        let p = seg.offset + heard as f64 / f64::from(self.audio.rate());
        seg.track.duration.map_or(p, |d| p.min(d))
    }

    /// The last `n` samples heard, mono, for the visualizer.
    pub fn samples(&self, n: usize) -> Option<Vec<i16>> {
        if self.status != Status::Playing {
            return None;
        }
        self.audio.window(n)
    }

    /// Volume slider position, `0..=100`.
    pub fn volume(&self) -> u32 {
        self.volume
    }

    /// Set the volume (slider position `0..=100`): applied as the samples
    /// go out, so it is heard within [`crate::audio::LEAD`].
    pub fn set_volume(&mut self, v: u32) {
        self.volume = v.min(100);
        self.apply_gain();
    }

    /// Balance, `-100..=100`.
    pub fn balance(&self) -> i32 {
        self.balance
    }

    /// Set the balance.
    pub fn set_balance(&mut self, b: i32) {
        self.balance = b.clamp(-100, 100);
        self.apply_gain();
    }

    /// The equalizer.
    pub fn equalizer(&self) -> &Equalizer {
        &self.eq
    }

    /// Set the equalizer: sent to every live decoder (mpg123 applies it to
    /// its subbands, at no extra cost).
    pub fn set_equalizer(&mut self, eq: Equalizer) -> io::Result<()> {
        self.eq = eq;
        for s in self.segments.iter_mut().filter(|s| !s.ended) {
            Self::send_eq(&self.eq, s)?;
        }
        Ok(())
    }

    /// Collect what happened since last time.
    pub fn poll(&mut self) -> Vec<Update> {
        let mut out = Vec::new();
        fn push(out: &mut Vec<Update>, u: Update) {
            if !out.contains(&u) {
                out.push(u);
            }
        }
        let last_id = self.segments.last().map(|s| s.id);
        let audible = self.audible_index(&self.audio.snapshot()).map(|i| self.segments[i].id);
        for i in 0..self.segments.len() {
            while let Some(m) = self.segments[i].engine.try_recv() {
                let seg = &mut self.segments[i];
                let mine = Some(seg.id) == audible || (audible.is_none() && i == 0);
                match m {
                    Message::Tag(k, v) => {
                        seg.track.tags.apply(&k, &v);
                        if mine {
                            push(&mut out, Update::Info);
                        }
                    }
                    Message::Stream(s) => {
                        let rate = s.rate;
                        seg.track.stream = Some(s);
                        if seg.sent_eq.is_some_and(|(_, r)| r != rate) {
                            if let Err(e) = Self::send_eq(&self.eq, seg) {
                                out.push(Update::Error(e.to_string()));
                            }
                        }
                        if mine {
                            push(&mut out, Update::Info);
                        }
                    }
                    Message::Progress(p) => {
                        if seg.track.duration.is_none() {
                            seg.track.duration = Some(p.seconds + p.seconds_left);
                            // The length is all we need: spare the CPU the
                            // ~40 progress lines a second.
                            let _ = seg.engine.send(&Command::Silence);
                            if mine {
                                push(&mut out, Update::Info);
                            }
                        }
                    }
                    Message::State(PlayState::Ended) => {
                        if !seg.ended {
                            seg.ended = true;
                            seg.engine.finish();
                            if Some(seg.id) == last_id {
                                push(&mut out, Update::NeedNext(seg.tag));
                            }
                        }
                    }
                    Message::Error(e) => {
                        let exited = e.starts_with("mpg123 exited");
                        if exited && seg.ended {
                            continue; // it was told to quit
                        }
                        // The error first, so the application has counted it
                        // when it decides whether to go on.
                        out.push(Update::Error(e));
                        if !seg.ended {
                            // A failed load: no audio will come. Treat it as
                            // the end, so the playlist moves on.
                            seg.ended = true;
                            seg.engine.finish();
                            if Some(seg.id) == last_id {
                                push(&mut out, Update::NeedNext(seg.tag));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let snap = self.audio.snapshot();
        if snap.sink_failed && self.status == Status::Playing {
            let why = self.sink.as_ref().map(SinkHandle::complaint).unwrap_or_default();
            out.push(Update::Error(if why.is_empty() { "the audio output stopped".into() } else { format!("audio output: {why}") }));
            self.stop();
            push(&mut out, Update::Status);
            return out;
        }
        if let Some(i) = self.audible_index(&snap) {
            let (id, tag) = (self.segments[i].id, self.segments[i].tag);
            if self.shown != Some(id) {
                self.shown = Some(id);
                out.push(Update::TrackChanged(tag));
                push(&mut out, Update::Info);
            }
            // Segments before the audible one are over.
            for s in self.segments.drain(..i) {
                self.audio.forget(s.id);
            }
        }
        if self.status == Status::Playing && snap.drained() && self.segments.iter().all(|s| s.ended) {
            // The end of the music: let the sink play out its buffer.
            self.audio.close_sink();
            if let Some(s) = self.sink.take() {
                s.finish();
            }
            self.flush();
            self.status = Status::Stopped;
            out.push(Update::Ended);
            push(&mut out, Update::Status);
        }
        out
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.flush();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::engine::tests::{have_mpg123, scratch};
    use crate::testsignal;
    use std::time::{Duration, Instant};

    /// Both decoders at `rate`: every playback test runs on each.
    fn both(rate: u32) -> [Decoding; 2] {
        [Decoding::Mpg123(Mpg123 { rate, ..Mpg123::default() }), Decoding::Builtin { rate }]
    }

    /// Says which decoder was running when a test fails.
    struct With(&'static str);
    impl Drop for With {
        fn drop(&mut self) {
            if std::thread::panicking() {
                eprintln!("(with the {} decoder)", self.0);
            }
        }
    }

    fn label(d: &Decoding) -> &'static str {
        match d {
            Decoding::Mpg123(_) => "mpg123",
            Decoding::Builtin { .. } => "built-in",
        }
    }

    /// Real-time output without a sound card.
    pub(crate) fn sleep_sink() -> SinkCommand {
        SinkCommand::out123(&["-o".into(), "sleep".into()], crate::audio::RATE)
    }

    /// Output into a raw file (written as fast as the pump paces it).
    pub(crate) fn file_sink(path: &std::path::Path) -> SinkCommand {
        SinkCommand::out123(&["-O".into(), path.to_str().unwrap().into()], crate::audio::RATE)
    }

    /// Read a file a sink writes once it stops growing (the sink plays out
    /// in the background after the end).
    pub(crate) fn settled(path: &std::path::Path) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last = None;
        loop {
            thread::sleep(Duration::from_millis(150));
            let now = std::fs::read(path).unwrap_or_default();
            if last.as_ref() == Some(&now.len()) || Instant::now() > deadline {
                return now;
            }
            last = Some(now.len());
        }
    }

    fn run_until(p: &mut Player, what: &str, mut f: impl FnMut(&mut Player, &[Update]) -> bool) -> Vec<Update> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut all = Vec::new();
        loop {
            let u = p.poll();
            all.extend(u.iter().cloned());
            if f(p, &u) {
                return all;
            }
            assert!(Instant::now() < deadline, "timed out: {what}; saw {all:?}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn tags_prefer_v2_and_display_sensibly() {
        let mut t = Tags::default();
        t.apply("title", "Real Title");
        t.apply("id3v1", "Old Title\u{1f}Old Artist\u{1f}Album\u{1f}1999\u{1f}\u{1f}Rock");
        assert_eq!(
            (t.title.as_deref(), t.artist.as_deref(), t.year.as_deref(), t.genre.as_deref()),
            (Some("Real Title"), Some("Old Artist"), Some("1999"), Some("Rock"))
        );
        assert_eq!(t.display().unwrap(), "Old Artist - Real Title");
        let mut tags = Tags { station: Some("Radio".into()), ..Tags::default() };
        assert_eq!(tags.display().unwrap(), "Radio");
        tags.apply("stream-title", "Band - Song");
        assert_eq!(tags.display().unwrap(), "Band - Song");
        assert_eq!(Tags::default().display(), None);
    }

    #[test]
    fn output_choice() {
        let out123 = Output::Command(SinkCommand::out123(&[], 44100));
        let dsp = Output::Oss("/dev/dsp".into());
        // (sound server, out123 installed, /dev/dsp present)
        assert_eq!(Output::pick(true, true, true, 44100), out123, "a server owns the device and mixes");
        assert_eq!(Output::pick(false, true, true, 44100), dsp, "a bare machine: straight to the device");
        assert_eq!(Output::pick(false, true, false, 44100), out123, "no /dev/dsp: alsa-lib through out123");
        assert_eq!(Output::pick(true, false, true, 44100), dsp, "no out123: the device is all there is");
        assert_eq!(Output::pick(false, false, false, 44100), dsp, "nothing: try the device, and say why it failed");
    }

    #[test]
    fn volume_and_balance_gains() {
        assert_eq!(volume_percent(50), 25.0);
        assert_eq!(volume_percent(300), 100.0);
        assert_eq!(gains(100, 0), [UNITY, UNITY]);
        assert_eq!(gains(50, 0), [UNITY / 4, UNITY / 4]);
        assert_eq!(gains(100, 50), [UNITY / 2, UNITY]);
        assert_eq!(gains(100, -100), [UNITY, 0]);
        assert_eq!(gains(0, 0), [0, 0]);
    }

    pub(crate) fn write_tone(dir: &std::path::Path, name: &str, title: &str, subband: u8, seconds: f64) -> String {
        let f = dir.join(name);
        let mut data = testsignal::id3v2(&[("TIT2", title)]);
        data.extend(testsignal::tone(subband, 0.8, seconds));
        std::fs::write(&f, data).unwrap();
        f.to_str().unwrap().to_string()
    }

    #[test]
    fn plays_tracks_back_to_back_without_a_gap() {
        if !have_mpg123() {
            return;
        }
        for dec in both(44100) {
            gapless(dec);
        }
    }

    fn gapless(dec: Decoding) {
        let _w = With(label(&dec));
        let d = scratch(&format!("player-gapless-{}", label(&dec)));
        let a = write_tone(&d, "a.mp3", "First", 2, 0.4);
        let b = write_tone(&d, "b.mp3", "Second", 6, 0.4);
        let out = d.join("out.raw");
        let mut p = Player::new(dec, file_sink(&out), Arc::new(|| {}));
        p.set_volume(100);
        p.play(&a, 1).unwrap();
        let mut titles = Vec::new();
        let seen = run_until(&mut p, "the end", |p, u| {
            for u in u {
                if *u == Update::NeedNext(1) {
                    p.enqueue(&b, 2).unwrap();
                }
                if let Update::TrackChanged(_) = u {
                    titles.push(p.current().and_then(|(t, _)| t.tags.title.clone()));
                }
            }
            p.status() == Status::Stopped
        });
        assert!(seen.contains(&Update::TrackChanged(1)) && seen.contains(&Update::TrackChanged(2)) && seen.contains(&Update::Ended), "{seen:?}");
        assert!(seen.contains(&Update::NeedNext(2)), "asked again after the second");
        assert_eq!(titles.last().cloned().flatten().as_deref(), Some("Second"));
        drop(p);
        // Both tones, back to back: 0.8 s at 44.1 kHz with no silence between.
        let pcm = settled(&out);
        // Both whole (mpg123's resampler adds a few frames at the edges).
        let frames = pcm.len() / 4;
        assert!((2 * 17640..2 * 17640 + 600).contains(&frames), "both tracks, whole: {frames}");
        let left: Vec<i32> = pcm.chunks(4).map(|c| i32::from(i16::from_le_bytes([c[0], c[1]]))).collect();
        let quiet = left.windows(256).filter(|w| w.iter().all(|v| v.abs() < 100)).count();
        assert_eq!(quiet, 0, "no gap anywhere");
    }

    #[test]
    fn equalizer_volume_and_balance_reach_the_output() {
        if !have_mpg123() {
            return;
        }
        for dec in both(44100) {
            settings(dec);
        }
    }

    fn settings(dec: Decoding) {
        let _w = With(label(&dec));
        let d = scratch(&format!("player-settings-{}", label(&dec)));
        let a = write_tone(&d, "a.mp3", "T", 2, 0.3);
        let run = |volume: u32, balance: i32, eq: Equalizer| {
            let out = d.join(format!("o-{volume}-{balance}-{}.raw", eq.on));
            let mut p = Player::new(dec.clone(), file_sink(&out), Arc::new(|| {}));
            p.set_volume(volume);
            p.set_balance(balance);
            p.set_equalizer(eq).unwrap();
            p.play(&a, 0).unwrap();
            run_until(&mut p, "the end", |p, _| p.status() == Status::Stopped);
            drop(p);
            let pcm = settled(&out);
            let s: Vec<i32> = pcm.chunks(2).map(|c| i32::from(i16::from_le_bytes([c[0], c[1]]))).collect();
            (s.iter().step_by(2).map(|v| v.abs()).max().unwrap(), s.iter().skip(1).step_by(2).map(|v| v.abs()).max().unwrap())
        };
        let (l0, r0) = run(100, 0, Equalizer::default());
        assert!(l0 > 10000 && (l0 - r0).abs() <= 1, "{l0} {r0}");
        let (l, r) = run(50, 0, Equalizer::default());
        assert!((f64::from(l) / f64::from(l0) - 0.25).abs() < 0.01 && (l - r).abs() <= 1, "volume: {l}");
        let (l, r) = run(100, 100, Equalizer::default());
        assert_eq!(l, 0, "balance right");
        assert!((r - r0).abs() <= 1);
        let (l, _) = run(100, 0, Equalizer { on: true, preamp: -6.0, bands: [0.0; 10] });
        assert!((f64::from(l) / f64::from(l0) - 0.501).abs() < 0.01, "preamp -6 dB, in mpg123: {l}");
    }

    #[test]
    fn half_rate_for_slow_machines() {
        if !have_mpg123() {
            return;
        }
        for dec in both(22050) {
            half_rate(dec);
        }
    }

    fn half_rate(dec: Decoding) {
        let _w = With(label(&dec));
        let d = scratch(&format!("player-rate-{}", label(&dec)));
        let a = write_tone(&d, "a.mp3", "T", 2, 0.5);
        let out = d.join("half.raw");
        let mut p = Player::new(dec, SinkCommand::out123(&["-O".into(), out.to_str().unwrap().into()], 22050), Arc::new(|| {}));
        p.play(&a, 0).unwrap();
        run_until(&mut p, "a quarter second", |p, _| p.position() > 0.25);
        assert!(p.position() < 0.45, "the clock runs at the right rate: {}", p.position());
        run_until(&mut p, "the end", |p, _| p.status() == Status::Stopped);
        drop(p);
        let frames = settled(&out).len() / 4;
        assert!((11025..11025 + 300).contains(&frames), "half as many samples: {frames}");
    }

    #[test]
    fn pause_seek_and_position() {
        if !have_mpg123() {
            return;
        }
        for dec in both(44100) {
            pause_seek(dec);
        }
    }

    fn pause_seek(dec: Decoding) {
        let _w = With(label(&dec));
        let d = scratch(&format!("player-seek-{}", label(&dec)));
        let a = write_tone(&d, "a.mp3", "Long", 2, 4.0);
        let mut p = Player::new(dec, sleep_sink(), Arc::new(|| {}));
        p.play(&a, 7).unwrap();
        run_until(&mut p, "half a second in", |p, _| p.position() >= 0.5);
        let t = p.current().unwrap().0.clone();
        assert_eq!(t.tags.title.as_deref(), Some("Long"));
        assert!((t.duration.unwrap() - 4.0).abs() < 0.05, "{t:?}");
        assert!(p.samples(512).is_some_and(|s| s.iter().any(|v| v.abs() > 1000)), "the visualizer sees the tone");
        p.toggle_pause().unwrap();
        let at = p.position();
        thread::sleep(Duration::from_millis(200));
        assert_eq!(p.position(), at, "paused");
        assert_eq!(p.samples(512), None);
        p.toggle_pause().unwrap();
        run_until(&mut p, "moving again", |p, _| p.position() > at + 0.1);
        p.seek(3.0).unwrap();
        assert!((p.position() - 3.0).abs() < 0.05, "{}", p.position());
        let u = run_until(&mut p, "the end", |p, _| p.status() == Status::Stopped);
        assert!(u.contains(&Update::NeedNext(7)) && u.contains(&Update::Ended), "{u:?}");
        p.play("/nonexistent/x.mp3", 9).unwrap();
        let u = run_until(&mut p, "an error", |_, u| u.iter().any(|u| matches!(u, Update::Error(_))));
        assert!(u.contains(&Update::NeedNext(9)), "a bad file moves the playlist on: {u:?}");
    }
}
