//! The playlist: entries, the current one, shuffle and repeat, and M3U /
//! PLS files.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// File extensions mpg123 plays (MPEG-1/2/2.5 audio, layers I–III).
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "mp2", "mp1", "mpga", "mpa"];

/// Playlist file extensions.
pub const LIST_EXTENSIONS: &[&str] = &["m3u", "m3u8", "pls"];

/// One track.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// A file path or a URL.
    pub path: String,
    /// "Artist - Title", from a playlist file or learned while playing.
    pub title: Option<String>,
    /// Length in seconds, if known.
    pub duration: Option<f64>,
    /// Unique within its playlist (assigned when added; 0 before), so a
    /// track can be found again after the list is edited.
    pub id: u64,
}

impl Entry {
    /// A track with nothing known about it yet.
    pub fn new(path: impl Into<String>) -> Entry {
        Entry { path: path.into(), title: None, duration: None, id: 0 }
    }

    /// What to show: the title, else the file name without extension (or
    /// the URL as is).
    pub fn display(&self) -> String {
        if let Some(t) = &self.title {
            return t.clone();
        }
        if is_url(&self.path) {
            return self.path.clone();
        }
        let p = Path::new(&self.path);
        p.file_stem().or(p.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| self.path.clone())
    }
}

/// `http://…` and friends, which mpg123 streams.
pub fn is_url(s: &str) -> bool {
    s.split_once("://").is_some_and(|(scheme, _)| !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+'))
}

fn has_extension(p: &Path, exts: &[&str]) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// `m:ss`, or `h:mm:ss` from an hour up; negative times get a minus.
pub fn format_time(seconds: f64) -> String {
    let neg = seconds < 0.0;
    let s = seconds.abs().floor() as u64;
    let body = if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) };
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

/// A small, seedable random source for shuffling (xorshift64*).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// Seeded; zero is replaced.
    pub fn new(seed: u64) -> Rng {
        Rng(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    /// Seeded from the clock and process id.
    pub fn from_time() -> Rng {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        Rng::new(t ^ (u64::from(std::process::id()) << 32))
    }

    /// The next number.
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `0..n` (`n > 0`).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// The playlist.
#[derive(Debug, Clone)]
pub struct Playlist {
    /// The tracks, in display order.
    pub entries: Vec<Entry>,
    current: Option<usize>,
    shuffle: bool,
    /// Play order: a permutation of entry indices (identity unless
    /// shuffling).
    order: Vec<usize>,
    /// Loop the list at its end.
    pub repeat: bool,
    rng: Rng,
    next_id: u64,
}

impl Default for Playlist {
    fn default() -> Playlist {
        Playlist::new(Rng::from_time())
    }
}

impl Playlist {
    /// An empty list.
    pub fn new(rng: Rng) -> Playlist {
        Playlist { entries: Vec::new(), current: None, shuffle: false, order: Vec::new(), repeat: false, rng, next_id: 1 }
    }

    /// The current track's index.
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    /// The current track.
    pub fn current_entry(&self) -> Option<&Entry> {
        self.current.and_then(|i| self.entries.get(i))
    }

    /// Make `i` current (no check that it exists beyond bounds).
    pub fn set_current(&mut self, i: Option<usize>) {
        self.current = i.filter(|&i| i < self.entries.len());
    }

    /// Shuffling.
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Turn shuffle on (a new order, starting from the current track) or off.
    pub fn set_shuffle(&mut self, on: bool) {
        self.shuffle = on;
        self.reorder();
    }

    fn reorder(&mut self) {
        let n = self.entries.len();
        self.order = (0..n).collect();
        if self.shuffle {
            for i in (1..n).rev() {
                let j = self.rng.below(i + 1);
                self.order.swap(i, j);
            }
            // The current track leads, so "next" never repeats it early.
            if let Some(c) = self.current {
                if let Some(at) = self.order.iter().position(|&x| x == c) {
                    self.order.remove(at);
                    self.order.insert(0, c);
                }
            }
        }
    }

    /// The play order (for tests and display).
    pub fn order(&self) -> &[usize] {
        &self.order
    }

    /// Append entries (each gets a fresh id).
    pub fn extend(&mut self, entries: impl IntoIterator<Item = Entry>) {
        for mut e in entries {
            e.id = self.next_id;
            self.next_id += 1;
            self.entries.push(e);
        }
        self.reorder();
    }

    /// Where the entry with this id is now.
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    /// Remove the entry at `i`.
    pub fn remove(&mut self, i: usize) {
        if i >= self.entries.len() {
            return;
        }
        self.entries.remove(i);
        self.current = match self.current {
            Some(c) if c == i => None,
            Some(c) if c > i => Some(c - 1),
            c => c,
        };
        self.reorder();
    }

    /// Remove everything.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = None;
        self.reorder();
    }

    /// Move entry `from` to position `to`, keeping the current track.
    pub fn move_entry(&mut self, from: usize, to: usize) {
        if from >= self.entries.len() || to >= self.entries.len() || from == to {
            return;
        }
        let cur = self.current.map(|c| self.entries[c].path.clone());
        let e = self.entries.remove(from);
        self.entries.insert(to, e);
        if let Some(c) = self.current {
            self.current = Some(if c == from {
                to
            } else if from < c && c <= to {
                c - 1
            } else if to <= c && c < from {
                c + 1
            } else {
                c
            });
        }
        debug_assert_eq!(cur, self.current.map(|c| self.entries[c].path.clone()));
        self.reorder();
    }

    /// Sort by what is displayed (case-insensitive), keeping the current track.
    pub fn sort_by_title(&mut self) {
        self.sort_by_key(|e| e.display().to_lowercase());
    }

    /// Sort by path, keeping the current track.
    pub fn sort_by_path(&mut self) {
        self.sort_by_key(|e| e.path.clone());
    }

    /// Reverse, keeping the current track.
    pub fn reverse(&mut self) {
        let n = self.entries.len();
        self.entries.reverse();
        self.current = self.current.map(|c| n - 1 - c);
        self.reorder();
    }

    /// Shuffle the list itself (not just the play order).
    pub fn randomize(&mut self) {
        let mut keyed: Vec<(u64, usize)> = (0..self.entries.len()).map(|i| (self.rng.next_u64(), i)).collect();
        keyed.sort_unstable();
        self.permute(keyed.into_iter().map(|(_, i)| i).collect());
    }

    fn sort_by_key<K: Ord>(&mut self, mut f: impl FnMut(&Entry) -> K) {
        let mut idx: Vec<usize> = (0..self.entries.len()).collect();
        idx.sort_by_key(|&i| f(&self.entries[i]));
        self.permute(idx);
    }

    /// Reorder so that new position `k` holds old entry `idx[k]`.
    fn permute(&mut self, idx: Vec<usize>) {
        let cur = self.current;
        let old = std::mem::take(&mut self.entries);
        let mut slots: Vec<Option<Entry>> = old.into_iter().map(Some).collect();
        self.entries = idx.iter().map(|&i| slots[i].take().expect("a permutation")).collect();
        self.current = cur.and_then(|c| idx.iter().position(|&i| i == c));
        self.reorder();
    }

    /// The track after the current one. At the end of the order: wrap
    /// around when repeating or when the user pressed "next", otherwise
    /// nothing (playback stops). A wrap while shuffling starts a new order.
    pub fn next(&mut self, by_user: bool) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }
        let at = self.current.and_then(|c| self.order.iter().position(|&x| x == c));
        let next = match at {
            None => Some(self.order[0]),
            Some(a) if a + 1 < self.order.len() => Some(self.order[a + 1]),
            Some(_) if self.repeat || by_user => {
                if self.shuffle {
                    let last = self.current;
                    self.current = None;
                    self.reorder();
                    // Avoid playing the same track twice in a row.
                    if self.order.len() > 1 && self.order[0] == last.unwrap_or(usize::MAX) {
                        self.order.swap(0, 1);
                    }
                }
                Some(self.order[0])
            }
            Some(_) => None,
        };
        if next.is_some() {
            self.current = next;
        }
        next
    }

    /// The track before the current one, wrapping to the end of the order.
    pub fn previous(&mut self) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }
        let at = self.current.and_then(|c| self.order.iter().position(|&x| x == c));
        let prev = match at {
            Some(a) if a > 0 => self.order[a - 1],
            _ => *self.order.last().expect("non-empty"),
        };
        self.current = Some(prev);
        Some(prev)
    }

    /// Total length of the tracks whose length is known, and whether any
    /// is unknown.
    pub fn total_duration(&self) -> (f64, bool) {
        let known: f64 = self.entries.iter().filter_map(|e| e.duration).sum();
        (known, self.entries.iter().any(|e| e.duration.is_none()))
    }

    /// The list as extended M3U.
    pub fn to_m3u(&self) -> String {
        let mut s = String::from("#EXTM3U\n");
        for e in &self.entries {
            if e.title.is_some() || e.duration.is_some() {
                let secs = e.duration.map_or(-1, |d| d.round() as i64);
                s += &format!("#EXTINF:{secs},{}\n", e.title.clone().unwrap_or_else(|| e.display()));
            }
            s += &e.path;
            s.push('\n');
        }
        s
    }
}

/// Parse an M3U/M3U8 playlist. Relative paths are resolved against `base`
/// (the playlist's directory).
pub fn parse_m3u(text: &str, base: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut info: Option<(Option<f64>, String)> = None;
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if let Some(inf) = line.strip_prefix("#EXTINF:") {
            info = inf.split_once(',').map(|(secs, title)| {
                let d = secs.split_whitespace().next().and_then(|s| s.parse::<f64>().ok()).filter(|d| *d >= 0.0);
                (d, title.trim().to_string())
            });
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut e = Entry::new(resolve(line, base));
        if let Some((d, t)) = info.take() {
            e.duration = d;
            e.title = Some(t).filter(|t| !t.is_empty());
        }
        out.push(e);
    }
    out
}

/// Parse a PLS playlist.
pub fn parse_pls(text: &str, base: &Path) -> Vec<Entry> {
    let mut by_index: Vec<(u32, Entry)> = Vec::new();
    fn slot(v: &mut Vec<(u32, Entry)>, n: u32) -> &mut Entry {
        if let Some(i) = v.iter().position(|(k, _)| *k == n) {
            return &mut v[i].1;
        }
        v.push((n, Entry::new("")));
        &mut v.last_mut().expect("pushed").1
    }
    for line in text.lines() {
        let Some((k, v)) = line.trim().split_once('=') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        let num = |prefix: &str| k.strip_prefix(prefix).and_then(|n| n.parse::<u32>().ok());
        if let Some(n) = num("file") {
            if !v.is_empty() {
                slot(&mut by_index, n).path = resolve(v, base);
            }
        } else if let Some(n) = num("title") {
            slot(&mut by_index, n).title = Some(v.to_string()).filter(|t| !t.is_empty());
        } else if let Some(n) = num("length") {
            slot(&mut by_index, n).duration = v.parse::<f64>().ok().filter(|d| *d >= 0.0);
        }
    }
    by_index.sort_by_key(|(n, _)| *n);
    by_index.into_iter().map(|(_, e)| e).filter(|e| !e.path.is_empty()).collect()
}

fn resolve(p: &str, base: &Path) -> String {
    let p = p.strip_prefix("file://").unwrap_or(p);
    if is_url(p) || Path::new(p).is_absolute() {
        return p.to_string();
    }
    base.join(p).to_string_lossy().into_owned()
}

/// Expand command-line arguments or browser picks into entries: audio
/// files as they are, playlists read, directories searched (recursively,
/// sorted by name). Unreadable things are reported, not fatal.
pub fn expand(paths: &[PathBuf]) -> (Vec<Entry>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for p in paths {
        let s = p.to_string_lossy();
        if is_url(&s) {
            out.push(Entry::new(s));
        } else if p.is_dir() {
            if let Err(e) = scan_dir(p, &mut out, 0) {
                errors.push(format!("{}: {e}", p.display()));
            }
        } else if has_extension(p, LIST_EXTENSIONS) {
            match read_list(p) {
                Ok(v) => out.extend(v),
                Err(e) => errors.push(format!("{}: {e}", p.display())),
            }
        } else if p.exists() {
            out.push(Entry::new(s));
        } else {
            errors.push(format!("{}: not found", p.display()));
        }
    }
    (out, errors)
}

/// Read an M3U or PLS file.
pub fn read_list(p: &Path) -> io::Result<Vec<Entry>> {
    let text = String::from_utf8_lossy(&fs::read(p)?).into_owned();
    let base = p.parent().unwrap_or(Path::new("."));
    Ok(if has_extension(p, &["pls"]) { parse_pls(&text, base) } else { parse_m3u(&text, base) })
}

fn scan_dir(dir: &Path, out: &mut Vec<Entry>, depth: u32) -> io::Result<()> {
    if depth > 16 {
        return Ok(()); // a symlink loop, most likely
    }
    let mut items: Vec<PathBuf> = fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    items.sort();
    for p in items {
        if p.is_dir() {
            let _ = scan_dir(&p, out, depth + 1);
        } else if has_extension(&p, AUDIO_EXTENSIONS) {
            out.push(Entry::new(p.to_string_lossy()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(n: usize) -> Playlist {
        let mut p = Playlist::new(Rng::new(42));
        p.extend((0..n).map(|i| Entry::new(format!("/m/{i}.mp3"))));
        p
    }

    #[test]
    fn next_and_previous_in_order() {
        let mut p = list(3);
        assert_eq!(p.next(false), Some(0), "nothing current: the first");
        assert_eq!(p.next(false), Some(1));
        assert_eq!(p.next(false), Some(2));
        assert_eq!(p.next(false), None, "the end, not repeating");
        assert_eq!(p.current(), Some(2));
        assert_eq!(p.next(true), Some(0), "the user's next wraps");
        assert_eq!(p.previous(), Some(2), "previous wraps too");
        p.repeat = true;
        assert_eq!(p.next(false), Some(0), "repeat wraps");
        assert_eq!(Playlist::new(Rng::new(1)).next(true), None);
    }

    #[test]
    fn shuffle_visits_everything_once_starting_after_current() {
        let mut p = list(20);
        p.set_current(Some(7));
        p.set_shuffle(true);
        assert_eq!(p.order()[0], 7);
        let mut seen = vec![7];
        while let Some(i) = p.next(false) {
            seen.push(i);
        }
        let mut sorted = seen.clone();
        sorted.sort();
        assert_eq!(sorted, (0..20).collect::<Vec<_>>(), "a permutation");
        assert_ne!(seen, (0..20).collect::<Vec<_>>().into_iter().cycle().skip(7).take(20).collect::<Vec<_>>(), "actually shuffled");
        // Wrapping reshuffles and never repeats the last track immediately.
        let last = p.current();
        assert_ne!(p.next(true), last);
        p.set_shuffle(false);
        assert_eq!(p.order(), (0..20).collect::<Vec<_>>());
    }

    #[test]
    fn edits_keep_the_current_track() {
        let mut p = list(5);
        p.set_current(Some(3));
        p.remove(1);
        assert_eq!(p.current_entry().unwrap().path, "/m/3.mp3");
        p.remove(2);
        assert_eq!(p.current(), None, "removing the current track");
        let mut p = list(5);
        p.set_current(Some(1));
        p.move_entry(1, 4);
        assert_eq!((p.current(), p.entries[4].path.as_str()), (Some(4), "/m/1.mp3"));
        p.move_entry(0, 2);
        assert_eq!(p.current_entry().unwrap().path, "/m/1.mp3");
        p.move_entry(4, 0);
        assert_eq!(p.current(), Some(0));
        p.reverse();
        assert_eq!(p.current_entry().unwrap().path, "/m/1.mp3");
        p.entries[2].title = Some("aardvark".into());
        p.sort_by_title();
        assert_eq!(p.entries[4].title.as_deref(), Some("aardvark"), "after the digit-named files");
        assert_eq!(p.current_entry().unwrap().path, "/m/1.mp3");
        p.randomize();
        assert_eq!(p.current_entry().unwrap().path, "/m/1.mp3");
        p.sort_by_path();
        assert_eq!(p.entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["/m/0.mp3", "/m/1.mp3", "/m/2.mp3", "/m/3.mp3", "/m/4.mp3"]);
        p.clear();
        assert_eq!((p.entries.len(), p.current()), (0, None));
    }

    #[test]
    fn m3u_round_trip_and_resolution() {
        let text =
            "\u{feff}#EXTM3U\n#EXTINF:123,Band - Song\nsong.mp3\n\n# comment\n/abs/x.mp3\n#EXTINF:-1,Radio\nhttp://radio.example/stream\nfile:///abs/y.mp3\n";
        let e = parse_m3u(text, Path::new("/lists"));
        assert_eq!(e.len(), 4);
        assert_eq!(e[0], Entry { path: "/lists/song.mp3".into(), title: Some("Band - Song".into()), duration: Some(123.0), id: 0 });
        assert_eq!(e[1], Entry::new("/abs/x.mp3"));
        assert_eq!(e[2], Entry { path: "http://radio.example/stream".into(), title: Some("Radio".into()), duration: None, id: 0 });
        assert_eq!(e[3].path, "/abs/y.mp3");
        let mut p = Playlist::new(Rng::new(1));
        p.extend(e.clone());
        let again = parse_m3u(&p.to_m3u(), Path::new("/elsewhere"));
        assert_eq!(again, e);
        assert_eq!(p.entries.iter().map(|e| e.id).collect::<Vec<_>>(), [1, 2, 3, 4], "ids assigned on adding");
        p.remove(0);
        assert_eq!(p.index_of(3), Some(1));
        assert_eq!(p.index_of(1), None);
    }

    #[test]
    fn pls() {
        let text = "[playlist]\nNumberOfEntries=2\nFile2=http://b/\nTitle2=B\nFile1=a.mp3\nTitle1=A\nLength1=61\nLength2=-1\nFile3=\nVersion=2\n";
        let e = parse_pls(text, Path::new("/d"));
        assert_eq!(
            e,
            [
                Entry { path: "/d/a.mp3".into(), title: Some("A".into()), duration: Some(61.0), id: 0 },
                Entry { path: "http://b/".into(), title: Some("B".into()), duration: None, id: 0 }
            ]
        );
    }

    #[test]
    fn expand_scans_directories_and_reads_lists() {
        let d = std::env::temp_dir().join(format!("phonolite-pl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("b/c")).unwrap();
        for f in ["b/2.MP3", "b/1.mp3", "b/c/3.mp2", "b/notes.txt", "a.mp3"] {
            fs::write(d.join(f), b"").unwrap();
        }
        fs::write(d.join("l.m3u"), "a.mp3\nb/1.mp3\n").unwrap();
        let (e, errors) = expand(&[d.join("b"), d.join("l.m3u"), d.join("missing.mp3"), PathBuf::from("https://x/y")]);
        let names: Vec<String> = e.iter().map(|e| e.path.strip_prefix(d.to_str().unwrap()).unwrap_or(&e.path).to_string()).collect();
        assert_eq!(names, ["/b/1.mp3", "/b/2.MP3", "/b/c/3.mp2", "/a.mp3", "/b/1.mp3", "https://x/y"]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("missing.mp3"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn display_and_time() {
        assert_eq!(Entry::new("/x/Some Song.mp3").display(), "Some Song");
        assert_eq!(Entry::new("http://radio/").display(), "http://radio/");
        assert_eq!(Entry { title: Some("T".into()), ..Entry::new("/a.mp3") }.display(), "T");
        assert!(is_url("http://a") && is_url("https://a") && !is_url("/a://b") && !is_url("://x"));
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(61.9), "1:01");
        assert_eq!(format_time(3599.0), "59:59");
        assert_eq!(format_time(3661.0), "1:01:01");
        assert_eq!(format_time(-75.0), "-1:15");
        let mut p = list(2);
        p.entries[0].duration = Some(10.0);
        assert_eq!(p.total_duration(), (10.0, true));
    }
}
