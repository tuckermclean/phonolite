//! The audio path: one mpg123 decodes to a pipe, a pump thread passes the
//! samples to the sound sink (`out123`), applying volume and balance and
//! keeping the last second for the visualizer. No second decoder — on a
//! 486 the decode is the expensive part, and it happens once.
//!
//! Being in the path means phonolite must keep latency down itself:
//!
//! - Writes are paced by the clock, at most [`LEAD`] ahead of what has
//!   played, so the sink's pipe never holds more than that: volume
//!   changes are heard within `LEAD`.
//! - Pausing kills the sink (dropping what it had queued) and rewinds to
//!   the estimated play position; resuming replays from there.
//! - A seek or a skip flushes: everything not yet heard is dropped, the
//!   sink restarts, and the old decoder's pipe is abandoned with it.
//! - Tracks that run into each other naturally are queued, so the next
//!   decoder's samples follow the last one's without a gap.
//!
//! The format is fixed for a session — stereo, signed 16-bit
//! little-endian, at [`RATE`] unless chosen otherwise (22.05 kHz halves
//! the work on slow machines) — and mpg123 is told to produce it, so the
//! sink never restarts for a format change.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

/// The usual sample rate.
pub const RATE: u32 = 44100;
/// Bytes per stereo frame.
const FRAME: usize = 4;
/// How far writes may run ahead of the estimated play position.
pub const LEAD: Duration = Duration::from_millis(300);

/// Frames per read and write.
const CHUNK: usize = 1024;
/// Unity gain.
pub const UNITY: i32 = 1 << 16;

/// Decoded audio arriving: a decoder's stdout.
pub type Source = Box<dyn Read + Send>;
/// The way out: a sink's stdin.
pub type Sink = Box<dyn Write + Send>;

struct State {
    rate: u32,
    queue: VecDeque<(u64, Source)>,
    sink: Option<Sink>,
    sink_gen: u64,
    paused: bool,
    stop: bool,
    epoch: u64,
    history: VecDeque<[i16; 2]>,
    /// Global frame index of `history[0]`.
    hist_start: u64,
    /// Frames read from sources, all told.
    read: u64,
    /// Frames handed to the sink.
    written: u64,
    /// (when, frame): the sink started playing `frame` at `when`.
    anchor: Option<(Instant, u64)>,
    /// Sources begun: id and the global frame they start at.
    starts: Vec<(u64, u64)>,
    /// Sources read to their end.
    finished: Vec<u64>,
    /// The source being read.
    current: Option<u64>,
    gain: [i32; 2],
    sink_failed: bool,
}

impl State {
    fn clock(&self, now: Instant) -> u64 {
        match self.anchor {
            Some((at, frame)) => {
                if now < at {
                    frame
                } else {
                    frame + (now.duration_since(at).as_secs_f64() * f64::from(self.rate)) as u64
                }
            }
            None => self.written,
        }
    }

    fn lead(&self) -> u64 {
        (LEAD.as_secs_f64() * f64::from(self.rate)) as u64
    }

    /// Frames of history kept (for the visualizer, and to replay after a
    /// pause): a second.
    fn history_len(&self) -> usize {
        self.rate as usize
    }

    fn audible(&self, now: Instant) -> u64 {
        self.clock(now).min(self.written)
    }

    fn truncate_history(&mut self, end: u64) {
        let keep = end.saturating_sub(self.hist_start) as usize;
        self.history.truncate(keep);
    }
}

/// What the pump has done, for the player to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Global frame now playing.
    pub audible: u64,
    /// Sources begun, with their global start frames.
    pub starts: Vec<(u64, u64)>,
    /// Sources read to their end.
    pub finished: Vec<u64>,
    /// The source being read, if any.
    pub current: Option<u64>,
    /// Queued sources not yet begun.
    pub queued: usize,
    /// Frames read so far.
    pub read: u64,
    /// Frames handed to the sink.
    pub written: u64,
    /// The sink stopped accepting data while playing.
    pub sink_failed: bool,
}

impl Snapshot {
    /// Everything read has been heard and nothing more is coming.
    pub fn drained(&self) -> bool {
        self.current.is_none() && self.queued == 0 && self.audible >= self.read
    }
}

/// The pump and its shared state.
pub struct Audio {
    shared: Arc<(Mutex<State>, Condvar)>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Audio {
    /// Start the pump for audio at `rate` Hz. `notify` is called when a
    /// source ends or the sink fails (from the pump thread).
    pub fn new(rate: u32, notify: Box<dyn Fn() + Send>) -> Audio {
        let rate = rate.max(1);
        let state = State {
            rate,
            queue: VecDeque::new(),
            sink: None,
            sink_gen: 0,
            paused: false,
            stop: false,
            epoch: 0,
            history: VecDeque::with_capacity(rate as usize),
            hist_start: 0,
            read: 0,
            written: 0,
            anchor: None,
            starts: Vec::new(),
            finished: Vec::new(),
            current: None,
            gain: [UNITY; 2],
            sink_failed: false,
        };
        let shared = Arc::new((Mutex::new(state), Condvar::new()));
        let s = shared.clone();
        let thread = thread::spawn(move || pump(&s, &*notify));
        Audio { shared, thread: Some(thread) }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared.0.lock().unwrap()
    }

    fn changed(&self, g: MutexGuard<'_, State>) {
        drop(g);
        self.shared.1.notify_all();
    }

    /// Queue a source to follow the current one (or start now, if idle).
    pub fn queue(&self, id: u64, source: Source) {
        let mut g = self.lock();
        g.queue.push_back((id, source));
        self.changed(g);
    }

    /// Give the pump a (new) sink. Unwritten frames are written to it first.
    pub fn set_sink(&self, sink: Sink) {
        let mut g = self.lock();
        g.sink = Some(sink);
        g.sink_gen += 1;
        g.anchor = None;
        g.sink_failed = false;
        self.changed(g);
    }

    /// Stop writing; everything after the estimated play position will be
    /// written again to the next sink. The caller kills the old sink
    /// process (dropping what it had queued).
    pub fn pause(&self) {
        let mut g = self.lock();
        let played = g.audible(Instant::now());
        g.paused = true;
        g.written = played;
        g.anchor = None;
        g.sink = None;
        g.sink_gen += 1;
        self.changed(g);
    }

    /// Resume into `sink`.
    pub fn resume(&self, sink: Sink) {
        let mut g = self.lock();
        g.paused = false;
        g.sink = Some(sink);
        g.sink_gen += 1;
        g.anchor = None;
        g.sink_failed = false;
        self.changed(g);
    }

    /// Close the sink's input once everything read has been written, so
    /// it plays out what it holds and exits by itself (the end of the
    /// music, as opposed to a flush, which cuts it off).
    pub fn close_sink(&self) {
        let mut g = self.lock();
        g.sink = None;
        g.sink_gen += 1;
        self.changed(g);
    }

    /// The sample rate.
    pub fn rate(&self) -> u32 {
        self.lock().rate
    }

    /// Paused.
    pub fn is_paused(&self) -> bool {
        self.lock().paused
    }

    /// Drop every source and everything not yet heard; the sink is
    /// dropped too (the caller kills its process and its decoders').
    pub fn flush(&self) {
        let mut g = self.lock();
        let now = g.audible(Instant::now());
        g.epoch += 1;
        g.queue.clear();
        g.current = None;
        g.read = now;
        g.written = now;
        g.truncate_history(now);
        g.anchor = None;
        g.sink = None;
        g.sink_gen += 1;
        g.starts.clear();
        g.finished.clear();
        g.paused = false;
        self.changed(g);
    }

    /// Volume and balance as 16.16 gains per channel.
    pub fn set_gain(&self, left: i32, right: i32) {
        self.lock().gain = [left.max(0), right.max(0)];
    }

    /// The state, for the player.
    pub fn snapshot(&self) -> Snapshot {
        let g = self.lock();
        Snapshot {
            audible: g.audible(Instant::now()),
            starts: g.starts.clone(),
            finished: g.finished.clone(),
            current: g.current,
            queued: g.queue.len(),
            read: g.read,
            written: g.written,
            sink_failed: g.sink_failed,
        }
    }

    /// Forget sources that have ended and been heard.
    pub fn forget(&self, id: u64) {
        let mut g = self.lock();
        g.finished.retain(|&f| f != id);
        g.starts.retain(|&(s, _)| s != id);
    }

    /// The `n` frames ending at the play position, mixed to mono.
    pub fn window(&self, n: usize) -> Option<Vec<i16>> {
        let g = self.lock();
        let end = g.audible(Instant::now());
        let from = end.checked_sub(n as u64)?;
        if from < g.hist_start || end > g.hist_start + g.history.len() as u64 {
            return None;
        }
        let a = (from - g.hist_start) as usize;
        Some(g.history.range(a..a + n).map(|f| ((i32::from(f[0]) + i32::from(f[1])) / 2) as i16).collect())
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        let mut g = self.lock();
        g.stop = true;
        self.changed(g);
        // The pump may be blocked reading a decoder or writing a sink; the
        // owner kills those processes. Don't wait for it here.
        drop(self.thread.take());
    }
}

fn pump(shared: &(Mutex<State>, Condvar), notify: &dyn Fn()) {
    let (m, cv) = shared;
    let mut src: Option<(u64, Source)> = None;
    let mut dst: Option<Sink> = None;
    let mut dst_gen = 0;
    let mut carry: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; CHUNK * FRAME];
    loop {
        // Decide what to do, under the lock.
        enum Do {
            Write(Vec<u8>, u64, u64),
            Read(u64),
        }
        let action = {
            let mut g = m.lock().unwrap();
            loop {
                if g.stop {
                    return;
                }
                if g.sink_gen != dst_gen {
                    dst = g.sink.take();
                    dst_gen = g.sink_gen;
                }
                if g.current.is_none() && src.is_some() {
                    src = None; // flushed
                    carry.clear();
                }
                if src.is_none() {
                    if let Some((id, s)) = g.queue.pop_front() {
                        let at = g.read;
                        g.starts.push((id, at));
                        g.current = Some(id);
                        src = Some((id, s));
                        carry.clear();
                    }
                }
                let now = Instant::now();
                if !g.paused && dst.is_some() && g.written < g.read {
                    let ahead = g.written.saturating_sub(g.clock(now));
                    let lead = g.lead();
                    if g.anchor.is_some() && ahead >= lead {
                        let wait = Duration::from_secs_f64((ahead - lead + 1) as f64 / f64::from(g.rate));
                        g = cv.wait_timeout(g, wait.min(Duration::from_millis(50))).unwrap().0;
                        continue;
                    }
                    // Fallen behind (the decoder was slow, the sink starved):
                    // the sink plays from here, now.
                    if g.anchor.is_some_and(|_| g.clock(now) > g.written) {
                        g.anchor = Some((now, g.written));
                    }
                    let from = g.written;
                    let n = ((g.read - from) as usize).min(CHUNK);
                    let a = (from - g.hist_start) as usize;
                    let gain = g.gain;
                    let mut bytes = Vec::with_capacity(n * FRAME);
                    for f in g.history.range(a..a + n) {
                        for (ch, &s) in f.iter().enumerate() {
                            let v = ((i64::from(s) * i64::from(gain[ch])) >> 16).clamp(-32768, 32767) as i16;
                            bytes.extend(v.to_le_bytes());
                        }
                    }
                    break Do::Write(bytes, from, dst_gen);
                }
                // Read when there is somewhere for it to go soon: not paused,
                // and not too far ahead (the history must hold the backlog).
                if src.is_some() && !g.paused && g.read - g.written < CHUNK as u64 {
                    break Do::Read(g.epoch);
                }
                g = cv.wait_timeout(g, Duration::from_millis(100)).unwrap().0;
            }
        };
        match action {
            Do::Write(bytes, from, gen) => {
                let ok = dst.as_mut().is_some_and(|d| d.write_all(&bytes).and_then(|()| d.flush()).is_ok());
                let mut g = m.lock().unwrap();
                if g.sink_gen != gen {
                    continue; // paused or flushed meanwhile: the write does not count
                }
                if ok {
                    g.written = from + (bytes.len() / FRAME) as u64;
                    if g.anchor.is_none() {
                        g.anchor = Some((Instant::now(), from));
                    }
                } else {
                    dst = None;
                    g.sink_failed = true;
                    drop(g);
                    notify();
                }
            }
            Do::Read(epoch) => {
                let n = match src.as_mut().map(|(_, s)| s.read(&mut buf)) {
                    Some(Ok(n)) => n,
                    _ => 0,
                };
                let mut g = m.lock().unwrap();
                if g.epoch != epoch {
                    src = None; // flushed while reading
                    carry.clear();
                    continue;
                }
                if n == 0 {
                    if let Some((id, _)) = src.take() {
                        g.finished.push(id);
                        g.current = None;
                    }
                    carry.clear();
                    drop(g);
                    cv.notify_all();
                    notify();
                    continue;
                }
                carry.extend_from_slice(&buf[..n]);
                let whole = carry.len() / FRAME * FRAME;
                for f in carry[..whole].chunks_exact(FRAME) {
                    g.history.push_back([i16::from_le_bytes([f[0], f[1]]), i16::from_le_bytes([f[2], f[3]])]);
                }
                g.read += (whole / FRAME) as u64;
                carry.drain(..whole);
                while g.history.len() > g.history_len() {
                    g.history.pop_front();
                    g.hist_start += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A source whose frame `i` holds `(i, -i)` (mod 2^15), so order,
    /// gaps and repeats are visible in what the sink gets.
    fn ramp(start: u64, frames: u64) -> Vec<u8> {
        (start..start + frames).flat_map(|i| [((i % 32768) as i16).to_le_bytes(), (-((i % 32768) as i16)).to_le_bytes()].concat()).collect()
    }

    #[derive(Clone, Default)]
    struct Rec(Arc<Mutex<Vec<u8>>>);
    impl Write for Rec {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Rec {
        fn frames(&self) -> Vec<(i16, i16)> {
            self.0.lock().unwrap().chunks_exact(4).map(|c| (i16::from_le_bytes([c[0], c[1]]), i16::from_le_bytes([c[2], c[3]]))).collect()
        }
    }

    fn wait(what: &str, mut f: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f() {
            assert!(Instant::now() < deadline, "timed out: {what}");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn passes_samples_through_paced_by_the_clock() {
        let a = Audio::new(RATE, Box::new(|| {}));
        let sink = Rec::default();
        a.set_sink(Box::new(sink.clone()));
        let frames = RATE as u64; // one second
        a.queue(1, Box::new(Cursor::new(ramp(0, frames))));
        let t0 = Instant::now();
        thread::sleep(Duration::from_millis(100));
        let early = sink.frames().len() as u64;
        // At 0.1 s, no more than 0.1 s + LEAD (+ a chunk) has gone out.
        assert!(early <= RATE as u64 / 10 + RATE as u64 * 3 / 10 + CHUNK as u64 + 2000, "paced: {early}");
        wait("the end", || a.snapshot().drained());
        let elapsed = t0.elapsed();
        assert!(elapsed >= Duration::from_millis(600), "real time minus the lead: {elapsed:?}");
        let got = sink.frames();
        assert_eq!(got.len() as u64, frames);
        assert!(got.iter().enumerate().all(|(i, &(l, r))| l == (i % 32768) as i16 && r == -((i % 32768) as i16)), "in order, intact");
        let s = a.snapshot();
        assert_eq!((s.starts.clone(), s.finished.clone()), (vec![(1, 0)], vec![1]));
    }

    #[test]
    fn gain_scales_each_channel() {
        let a = Audio::new(RATE, Box::new(|| {}));
        let sink = Rec::default();
        a.set_gain(UNITY / 2, 0);
        a.set_sink(Box::new(sink.clone()));
        a.queue(1, Box::new(Cursor::new(ramp(1000, 2000))));
        wait("written", || sink.frames().len() == 2000);
        assert!(sink.frames().iter().enumerate().all(|(i, &(l, r))| l == (1000 + i as i16) / 2 && r == 0));
    }

    #[test]
    fn queued_sources_follow_without_a_gap() {
        let a = Audio::new(RATE, Box::new(|| {}));
        let sink = Rec::default();
        a.set_sink(Box::new(sink.clone()));
        a.queue(1, Box::new(Cursor::new(ramp(0, 3001))));
        a.queue(2, Box::new(Cursor::new(ramp(3001, 3000))));
        wait("both", || a.snapshot().drained());
        let got = sink.frames();
        assert_eq!(got.len(), 6001);
        assert!(got.iter().enumerate().all(|(i, &(l, _))| l == i as i16), "seamless");
        assert_eq!(a.snapshot().starts, [(1, 0), (2, 3001)]);
    }

    #[test]
    fn pause_rewinds_to_what_was_heard_and_resume_replays_it() {
        let a = Audio::new(RATE, Box::new(|| {}));
        let first = Rec::default();
        a.set_sink(Box::new(first.clone()));
        a.queue(1, Box::new(Cursor::new(ramp(0, RATE as u64))));
        thread::sleep(Duration::from_millis(200));
        a.pause();
        let heard = a.snapshot().audible;
        let sent = first.frames().len() as u64;
        assert!(sent > heard, "the first sink had been sent more than was heard ({sent} vs {heard})");
        assert!((heard as f64 / f64::from(RATE) - 0.2).abs() < 0.05, "heard ≈ 0.2 s: {heard}");
        thread::sleep(Duration::from_millis(100));
        assert_eq!(a.snapshot().audible, heard, "paused: the clock stops");
        let second = Rec::default();
        a.resume(Box::new(second.clone()));
        wait("the end", || a.snapshot().drained());
        let got = second.frames();
        assert_eq!(got[0].0, (heard % 32768) as i16, "resumes exactly where the ear left off");
        assert!(got.iter().enumerate().all(|(i, &(l, _))| l == ((heard + i as u64) % 32768) as i16), "then continues intact");
        assert_eq!(heard + got.len() as u64, RATE as u64);
    }

    #[test]
    fn flush_drops_the_rest() {
        let a = Audio::new(RATE, Box::new(|| {}));
        a.set_sink(Box::new(Rec::default()));
        a.queue(1, Box::new(Cursor::new(ramp(0, RATE as u64))));
        a.queue(2, Box::new(Cursor::new(ramp(0, 100))));
        thread::sleep(Duration::from_millis(100));
        a.flush();
        let s = a.snapshot();
        assert_eq!((s.current, s.queued, s.read, s.written), (None, 0, s.audible, s.audible));
        let after = Rec::default();
        a.set_sink(Box::new(after.clone()));
        a.queue(3, Box::new(Cursor::new(ramp(20000, 500))));
        wait("the new source", || a.snapshot().drained());
        let got = after.frames();
        assert_eq!(got.len(), 500, "nothing of the old sources");
        assert_eq!(got[0].0, 20000);
        assert_eq!(a.snapshot().starts, [(3, s.audible)]);
    }

    #[test]
    fn the_visualizer_window_ends_at_the_play_position() {
        let a = Audio::new(RATE, Box::new(|| {}));
        a.set_sink(Box::new(Rec::default()));
        // Both channels carry the ramp, so the mono mix is the ramp.
        let both: Vec<u8> = (0..RATE as u64).flat_map(|i| [(i as i16).to_le_bytes(), (i as i16).to_le_bytes()].concat()).collect();
        a.queue(1, Box::new(Cursor::new(both)));
        thread::sleep(Duration::from_millis(150));
        a.pause();
        let heard = a.snapshot().audible;
        let w = a.window(512).unwrap();
        assert_eq!(w.len(), 512);
        assert_eq!(*w.last().unwrap(), (heard - 1) as i16, "the last sample heard");
        assert!(w.windows(2).all(|p| p[1] == p[0] + 1));
        assert_eq!(Audio::new(RATE, Box::new(|| {})).window(512), None, "nothing heard yet");
    }

    #[test]
    fn a_failing_sink_is_reported() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let flag = Arc::new(Mutex::new(false));
        let f = flag.clone();
        let a = Audio::new(RATE, Box::new(move || *f.lock().unwrap() = true));
        a.set_sink(Box::new(Broken));
        a.queue(1, Box::new(Cursor::new(ramp(0, 100))));
        wait("failure", || a.snapshot().sink_failed);
        assert!(*flag.lock().unwrap());
    }
}
