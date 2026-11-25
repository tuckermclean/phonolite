//! The mpg123 process: spawned in remote mode, fed [`Command`]s on stdin,
//! its replies (on stderr) read on a thread and handed over as
//! [`Message`]s, its decoded audio (on stdout) handed to [`crate::audio`].

use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command as Process, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::remote::{parse, Command, Message};

/// How to run mpg123.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mpg123 {
    /// The program (default `mpg123`, found on `PATH`).
    pub program: String,
    /// Extra decoder arguments (e.g. `["--resync-limit", "-1"]`). The
    /// output format and remote mode are always added.
    pub args: Vec<String>,
    /// The sample rate to decode to (mpg123 resamples; 22050 halves the
    /// work of a 44.1 kHz file, using its cheaper 2:1 synthesis).
    pub rate: u32,
}

impl Default for Mpg123 {
    fn default() -> Mpg123 {
        Mpg123 { program: "mpg123".into(), args: Vec::new(), rate: crate::audio::RATE }
    }
}

/// Something to call when a message worth redrawing for arrives (from the
/// reader thread): typically [`protolith::x11::Waker::wake`].
pub type Notify = Box<dyn Fn() + Send>;

/// A running `mpg123 -R`, decoding to its stdout. One per track: a fresh
/// process is the only way to drop what is already in its pipe.
pub struct Engine {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    rx: Receiver<Message>,
    exited: Arc<AtomicBool>,
    stderr: Arc<Mutex<String>>,
}

impl Engine {
    /// Start mpg123 in remote mode with its replies on stderr and raw
    /// audio on stdout (take it with [`Engine::take_audio`]). `notify` is
    /// called from the reader thread for every message except progress
    /// (`@F`), which is collected by polling instead.
    pub fn spawn(cfg: &Mpg123, notify: Notify) -> io::Result<Engine> {
        let mut child = Process::new(&cfg.program)
            .args(&cfg.args)
            .args(["-R", "--remote-err", "-s", "--stereo", "-e", "s16", "-r"])
            .arg(cfg.rate.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| io::Error::new(e.kind(), format!("cannot run {}: {e}", cfg.program)))?;
        let stdin = child.stdin.take().ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "no stdin"))?;
        let stdout = child.stdout.take();
        let replies = child.stderr.take().expect("piped");
        let (tx, rx) = mpsc::channel();
        let exited = Arc::new(AtomicBool::new(false));
        let stderr = Arc::new(Mutex::new(String::new()));

        let gone = exited.clone();
        let tail = stderr.clone();
        thread::spawn(move || {
            let mut r = BufReader::new(replies);
            let mut line = Vec::new();
            loop {
                line.clear();
                match r.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let text = String::from_utf8_lossy(&line);
                // Replies start with '@'; anything else is mpg123 (or a
                // library) complaining — keep the last line for errors.
                if !text.starts_with('@') {
                    let t = text.trim();
                    if !t.is_empty() {
                        *tail.lock().unwrap() = t.to_string();
                    }
                    continue;
                }
                let msg = parse(&text);
                let quiet = matches!(msg, Message::Progress(_) | Message::Other(_));
                if tx.send(msg).is_err() {
                    return;
                }
                if !quiet {
                    notify();
                }
            }
            gone.store(true, Ordering::SeqCst);
            let why = tail.lock().unwrap().clone();
            let _ = tx.send(Message::Error(if why.is_empty() { "mpg123 exited".into() } else { format!("mpg123 exited: {why}") }));
            notify();
        });
        Ok(Engine { child, stdin: Some(stdin), stdout, rx, exited, stderr })
    }

    /// The decoded audio (once).
    pub fn take_audio(&mut self) -> Option<ChildStdout> {
        self.stdout.take()
    }

    /// Send a command. Fails if mpg123 has gone away.
    pub fn send(&mut self, cmd: &Command) -> io::Result<()> {
        let Some(line) = cmd.line() else { return Err(io::Error::new(io::ErrorKind::InvalidInput, "a file name with a line break cannot be sent to mpg123")) };
        let stdin = self.stdin.as_mut().ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "mpg123 is closed"))?;
        stdin.write_all(line.as_bytes())?;
        stdin.flush()
    }

    /// The next message, if one has arrived.
    pub fn try_recv(&self) -> Option<Message> {
        self.rx.try_recv().ok()
    }

    /// Wait up to `timeout` for the next message (for tests and tools).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Message> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// mpg123 has exited (its reply stream closed).
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// The last line mpg123 wrote that was not a reply.
    pub fn stderr_tail(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }

    /// End it now (its pipe's contents are abandoned).
    pub fn kill(&mut self) {
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Let it finish: no more commands; it exits once its audio has been
    /// read (stdout then closes, which is how the pump knows the track
    /// ended).
    pub fn finish(&mut self) {
        let _ = self.send(&Command::Quit);
        self.stdin = None;
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Its audio is no longer wanted by anyone holding this: don't let a
        // full pipe keep it alive.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Read everything from a pipe until EOF (for tests that capture PCM).
pub fn read_all(mut r: impl Read) -> Vec<u8> {
    let mut v = Vec::new();
    let _ = r.read_to_end(&mut v);
    v
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::remote::PlayState;
    use crate::testsignal;
    use std::path::PathBuf;

    /// mpg123 is installed (tests that need it skip otherwise, unless
    /// `PHONOLITE_REQUIRE_MPG123` is set).
    pub(crate) fn have_mpg123() -> bool {
        let ok = Process::new("mpg123").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        if !ok && std::env::var_os("PHONOLITE_REQUIRE_MPG123").is_some() {
            panic!("PHONOLITE_REQUIRE_MPG123 is set but mpg123 cannot be run");
        }
        ok
    }

    /// A scratch directory for one test.
    pub(crate) fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("phonolite-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    pub(crate) fn wait_for(e: &Engine, what: &str, mut f: impl FnMut(&Message) -> bool) -> Vec<Message> {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        while std::time::Instant::now() < deadline {
            if let Some(m) = e.recv_timeout(Duration::from_millis(100)) {
                let hit = f(&m);
                seen.push(m);
                if hit {
                    return seen;
                }
            }
        }
        panic!("timed out waiting for {what}; saw {seen:?}");
    }

    #[test]
    fn plays_reports_and_ends() {
        if !have_mpg123() {
            return;
        }
        let dir = scratch("engine");
        let file = dir.join("tagged tone.mp3");
        let mut data = testsignal::id3v2(&[("TIT2", "Tone Song"), ("TPE1", "Generator")]);
        data.extend(testsignal::tone(2, 0.8, 0.5));
        std::fs::write(&file, data).unwrap();

        let woken = Arc::new(Mutex::new(0));
        let w = woken.clone();
        let mut e = Engine::spawn(&Mpg123::default(), Box::new(move || *w.lock().unwrap() += 1)).unwrap();
        let audio = e.take_audio().unwrap();
        assert!(e.take_audio().is_none(), "once");
        let reader = thread::spawn(move || read_all(audio));
        wait_for(&e, "ready", |m| matches!(m, Message::Ready(_)));
        e.send(&Command::Load(file.to_str().unwrap().into())).unwrap();
        let seen = wait_for(&e, "the end", |m| *m == Message::State(PlayState::Ended));
        assert!(seen.contains(&Message::Tag("title".into(), "Tone Song".into())), "{seen:?}");
        assert!(seen.contains(&Message::Tag("artist".into(), "Generator".into())));
        assert!(seen.iter().any(|m| matches!(m, Message::Stream(s) if s.rate == 48000 && s.layer == 1 && s.channels == 2)));
        let last = seen.iter().rev().find_map(|m| if let Message::Progress(p) = m { Some(*p) } else { None }).expect("progress");
        assert!((last.seconds + last.seconds_left - 0.5).abs() < 0.02, "{last:?}");
        assert!(*woken.lock().unwrap() >= 3, "woken for tags, stream, states");
        wait_for(&e, "stopped", |m| *m == Message::State(PlayState::Stopped));
        e.finish();
        // 0.5 s at the fixed output format: 44.1 kHz, stereo, 16-bit.
        let pcm = reader.join().unwrap();
        // mpg123's resampler adds a few frames at the edges.
        let frames = pcm.len() / 4;
        assert!((22050..22050 + 300).contains(&frames), "resampled from 48 kHz to 44.1 kHz: {frames}");
        let peak = pcm.chunks(2).map(|c| i32::from(i16::from_le_bytes([c[0], c[1]])).abs()).max().unwrap();
        assert!(peak > 10000, "{peak}");
    }

    #[test]
    fn errors_and_exit_are_reported() {
        if !have_mpg123() {
            return;
        }
        let mut e = Engine::spawn(&Mpg123::default(), Box::new(|| {})).unwrap();
        e.send(&Command::Load("/nonexistent/phonolite.mp3".into())).unwrap();
        wait_for(&e, "an error", |m| matches!(m, Message::Error(_)));
        assert!(e.send(&Command::Load("a\nb".into())).is_err());
        e.send(&Command::Quit).unwrap();
        let seen = wait_for(&e, "exit", |m| matches!(m, Message::Error(s) if s.starts_with("mpg123 exited")));
        assert!(e.has_exited(), "{seen:?}");
        assert!(Engine::spawn(&Mpg123 { program: "/nonexistent/mpg123".into(), ..Mpg123::default() }, Box::new(|| {})).is_err());
    }
}
