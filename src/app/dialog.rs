//! Dialogs: ordinary decorated windows built from protolith's stock
//! widgets — a file browser (for tracks, skins and playlists), a URL
//! prompt, a playlist saver, and jump-to-file.

use std::path::{Path, PathBuf};

use protolith::canvas::Canvas;
use protolith::font::Terminus;
use protolith::ui::{Button, FieldStyle, List, Response, Style, TextField};
use protolith::x11::{Event, Key, WindowId};
use protolith::Rect;

use crate::playlist::{Entry, AUDIO_EXTENSIONS, LIST_EXTENSIONS};

/// Dialog size.
pub const SIZE: (u32, u32) = (380, 282);
const ROW: u32 = 14;

/// Which dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Add tracks (files, folders, playlists).
    Open,
    /// Choose a skin.
    Skin,
    /// Load a playlist.
    LoadList,
    /// Save the playlist.
    SaveList,
    /// Type a URL.
    Url,
    /// Find a playlist entry by name.
    Jump,
}

impl Kind {
    /// The window title.
    pub fn title(self) -> &'static str {
        match self {
            Kind::Open => "phonolite: add files",
            Kind::Skin => "phonolite: choose a skin",
            Kind::LoadList => "phonolite: load playlist",
            Kind::SaveList => "phonolite: save playlist",
            Kind::Url => "phonolite: add a URL",
            Kind::Jump => "phonolite: jump to file",
        }
    }
}

/// What the dialog wants done.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Close it.
    Close,
    /// Add these (and play the first, if `play`).
    Add(Vec<PathBuf>, bool),
    /// Add a URL (and play it, if `play`).
    AddUrl(String, bool),
    /// Use this skin.
    Skin(PathBuf),
    /// Load this playlist (replacing the current one).
    LoadList(PathBuf),
    /// Save the playlist here.
    SaveList(PathBuf),
    /// Play the playlist entry with this id.
    PlayEntry(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Btn {
    Up,
    Add,
    Play,
    Ok,
    Cancel,
}

/// One row of the list: what it shows and what it is.
#[derive(Debug, Clone, PartialEq)]
enum Row {
    Dir(PathBuf),
    File(PathBuf),
    Entry(u64),
}

/// An open dialog.
pub struct Dialog {
    /// Its window.
    pub win: WindowId,
    /// Which one.
    pub kind: Kind,
    /// The text field (path, URL, file name or filter).
    pub field: TextField,
    /// The rows.
    pub list: List,
    rows: Vec<Row>,
    buttons: Vec<(Button, &'static str, Btn)>,
    /// The folder shown.
    pub dir: PathBuf,
    /// Its picture.
    pub canvas: Canvas,
}

fn wanted(kind: Kind, p: &Path) -> bool {
    let ext = p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    match kind {
        Kind::Open => AUDIO_EXTENSIONS.contains(&ext.as_str()) || LIST_EXTENSIONS.contains(&ext.as_str()),
        Kind::Skin => ext == "wsz" || ext == "zip",
        Kind::LoadList | Kind::SaveList => LIST_EXTENSIONS.contains(&ext.as_str()),
        Kind::Url | Kind::Jump => false,
    }
}

impl Dialog {
    /// A dialog for window `win`, browsing from `dir` (file kinds) or over
    /// `entries` (jump).
    pub fn new(win: WindowId, kind: Kind, dir: PathBuf, entries: &[Entry]) -> Dialog {
        let (w, h) = SIZE;
        let field = TextField::new(Rect::new(8, 8, w - 16, 20));
        let list = List::new(Rect::new(8, 34, w - 16, h - 34 - 40), ROW);
        let labels: &[(&'static str, Btn)] = match kind {
            Kind::Open => &[("Up", Btn::Up), ("Add", Btn::Add), ("Play", Btn::Play), ("Cancel", Btn::Cancel)],
            Kind::Skin => &[("Up", Btn::Up), ("Use skin", Btn::Ok), ("Cancel", Btn::Cancel)],
            Kind::LoadList => &[("Up", Btn::Up), ("Load", Btn::Ok), ("Cancel", Btn::Cancel)],
            Kind::SaveList => &[("Up", Btn::Up), ("Save", Btn::Ok), ("Cancel", Btn::Cancel)],
            Kind::Url => &[("Add", Btn::Add), ("Play", Btn::Play), ("Cancel", Btn::Cancel)],
            Kind::Jump => &[("Play", Btn::Play), ("Cancel", Btn::Cancel)],
        };
        let bw = 84;
        let buttons = labels
            .iter()
            .enumerate()
            .map(|(i, &(label, b))| (Button::new(Rect::new(w as i32 - 8 - (labels.len() - i) as i32 * (bw + 6) + 6, h as i32 - 32, bw as u32, 24)), label, b))
            .collect();
        let mut d = Dialog { win, kind, field, list, rows: Vec::new(), buttons, dir, canvas: Canvas::new(w, h) };
        d.field.set_focused(true);
        match kind {
            Kind::Url => d.field.edit.set_text("http://"),
            Kind::Jump => {}
            Kind::SaveList => {
                let dir = d.dir.clone();
                d.browse(&dir);
                d.field.edit.set_text(&dir.join("playlist.m3u").to_string_lossy());
            }
            _ => {
                let dir = d.dir.clone();
                d.browse(&dir);
            }
        }
        if kind == Kind::Jump {
            d.filter(entries);
        }
        d.field.edit.set_cursor(d.field.edit.text().len(), false);
        d
    }

    /// Show folder `dir`: subfolders, then the files this dialog wants.
    pub fn browse(&mut self, dir: &Path) {
        let Ok(read) = std::fs::read_dir(dir) else { return };
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for e in read.filter_map(Result::ok) {
            let p = e.path();
            let hidden = p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));
            if hidden {
                continue;
            }
            if p.is_dir() {
                dirs.push(p);
            } else if wanted(self.kind, &p) {
                files.push(p);
            }
        }
        dirs.sort();
        files.sort();
        self.dir = dir.to_path_buf();
        self.rows = dirs.into_iter().map(Row::Dir).chain(files.into_iter().map(Row::File)).collect();
        self.list.items = self
            .rows
            .iter()
            .map(|r| match r {
                Row::Dir(p) => format!("{}/", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                Row::File(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                Row::Entry(_) => String::new(),
            })
            .collect();
        self.list.select(None);
        self.list.set_top(0);
        if self.kind != Kind::SaveList {
            self.field.edit.set_text(&dir.to_string_lossy());
            self.field.edit.set_cursor(self.field.edit.text().len(), false);
        }
    }

    /// Jump: show the entries whose name contains every word typed.
    pub fn filter(&mut self, entries: &[Entry]) {
        let words: Vec<String> = self.field.edit.text().to_lowercase().split_whitespace().map(String::from).collect();
        let hits: Vec<(usize, &Entry)> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let name = e.display().to_lowercase();
                words.iter().all(|w| name.contains(w.as_str()))
            })
            .collect();
        self.rows = hits.iter().map(|(_, e)| Row::Entry(e.id)).collect();
        self.list.items = hits.iter().map(|(i, e)| format!("{}. {}", i + 1, e.display())).collect();
        self.list.select(if self.rows.is_empty() { None } else { Some(0) });
        self.list.set_top(0);
    }

    fn activate(&mut self, row: usize, play: bool) -> Option<Outcome> {
        match self.rows.get(row)?.clone() {
            Row::Dir(p) => {
                if self.kind == Kind::Skin && looks_like_skin(&p) && play {
                    return Some(Outcome::Skin(p));
                }
                self.browse(&p);
                None
            }
            Row::File(p) => Some(match self.kind {
                Kind::Skin => Outcome::Skin(p),
                Kind::LoadList => Outcome::LoadList(p),
                Kind::SaveList => {
                    self.field.edit.set_text(&p.to_string_lossy());
                    return None;
                }
                _ => Outcome::Add(vec![p], play),
            }),
            Row::Entry(id) => Some(Outcome::PlayEntry(id)),
        }
    }

    fn button(&mut self, b: Btn) -> Option<Outcome> {
        let text = self.field.edit.text().trim().to_string();
        match b {
            Btn::Cancel => Some(Outcome::Close),
            Btn::Up => {
                if let Some(parent) = self.dir.parent().map(Path::to_path_buf) {
                    self.browse(&parent);
                }
                None
            }
            Btn::Add | Btn::Play | Btn::Ok => {
                let play = b != Btn::Add;
                match self.kind {
                    Kind::Url => (!text.is_empty() && text != "http://").then_some(Outcome::AddUrl(text.clone(), play)),
                    Kind::Jump => self.list.selected().and_then(|i| self.activate(i, true)),
                    Kind::SaveList => (!text.is_empty()).then(|| Outcome::SaveList(PathBuf::from(text))),
                    Kind::Skin => match self.list.selected().map(|i| self.rows[i].clone()) {
                        Some(Row::File(p) | Row::Dir(p)) => Some(Outcome::Skin(p)),
                        _ => Some(Outcome::Skin(self.dir.clone())),
                    },
                    Kind::LoadList => match self.list.selected().map(|i| self.rows[i].clone()) {
                        Some(Row::File(p)) => Some(Outcome::LoadList(p)),
                        _ => None,
                    },
                    Kind::Open => match self.list.selected().map(|i| self.rows[i].clone()) {
                        // A folder adds everything in it.
                        Some(Row::File(p) | Row::Dir(p)) => Some(Outcome::Add(vec![p], play)),
                        _ => Some(Outcome::Add(vec![self.dir.clone()], play)),
                    },
                }
            }
        }
    }

    /// Handle an event for the dialog's window. `entries` is the playlist
    /// (for jump's filter).
    pub fn handle(&mut self, e: &Event, entries: &[Entry], font: &Terminus) -> Option<Outcome> {
        if let Event::CloseRequested { .. } | Event::Key { key: Key::Escape, pressed: true, .. } = e {
            return Some(Outcome::Close);
        }
        // Up and down move through the list even while typing.
        if let Event::Key { key: Key::Up | Key::Down | Key::PageUp | Key::PageDown, pressed: true, .. } = e {
            let was = self.list.is_focused();
            self.list.set_focused(true);
            self.list.handle(e);
            self.list.set_focused(was);
            if let Some(i) = self.list.selected() {
                let vis = self.list.visible_rows().max(1);
                if i < self.list.top() {
                    self.list.set_top(i);
                } else if i >= self.list.top() + vis {
                    self.list.set_top(i + 1 - vis);
                }
            }
            return None;
        }
        let before = self.field.edit.text().to_string();
        match self.field.handle(e, font) {
            Response::Submit => {
                let text = self.field.edit.text().trim().to_string();
                return match self.kind {
                    Kind::Jump => self.list.selected().and_then(|i| self.activate(i, true)),
                    Kind::Url => self.button(Btn::Play),
                    Kind::SaveList => self.button(Btn::Ok),
                    _ => {
                        let p = PathBuf::from(&text);
                        if p.is_dir() {
                            self.browse(&p);
                            None
                        } else if p.exists() {
                            match self.kind {
                                Kind::Skin => Some(Outcome::Skin(p)),
                                Kind::LoadList => Some(Outcome::LoadList(p)),
                                _ => Some(Outcome::Add(vec![p], true)),
                            }
                        } else if let Some(i) = self.list.selected() {
                            self.activate(i, true)
                        } else {
                            None
                        }
                    }
                };
            }
            Response::Cancel => return Some(Outcome::Close),
            _ => {}
        }
        if self.kind == Kind::Jump && self.field.edit.text() != before {
            self.filter(entries);
        }
        match self.list.handle(e) {
            Response::Submit => {
                if let Some(i) = self.list.selected() {
                    return self.activate(i, true);
                }
            }
            Response::Changed if self.kind == Kind::SaveList => {
                if let Some(Row::File(p)) = self.list.selected().map(|i| self.rows[i].clone()) {
                    self.field.edit.set_text(&p.to_string_lossy());
                }
            }
            _ => {}
        }
        // The field keeps the keyboard: clicks on the list should not take it.
        if matches!(e, Event::Button { .. }) {
            self.field.set_focused(true);
            self.list.set_focused(false);
        }
        let mut pressed = None;
        for (b, _, what) in &mut self.buttons {
            if b.handle(e) == Response::Clicked {
                pressed = Some(*what);
            }
        }
        pressed.and_then(|b| self.button(b))
    }

    /// Redraw the picture.
    pub fn draw(&mut self, style: &Style, field_style: &FieldStyle, font: &Terminus) {
        self.canvas.fill(style.face);
        self.field.draw(&mut self.canvas, font, field_style);
        if self.kind != Kind::Url {
            self.list.draw(&mut self.canvas, font, style);
        } else {
            use protolith::font::Font;
            let hint = "Type a stream or file URL, then Enter.";
            font.draw(&mut self.canvas, hint, 10, 40, style.text);
        }
        for (b, label, _) in &self.buttons {
            b.draw(&mut self.canvas, font, label, style);
        }
    }
}

/// A folder holding a classic skin's main bitmap.
pub fn looks_like_skin(p: &Path) -> bool {
    std::fs::read_dir(p).is_ok_and(|r| r.filter_map(Result::ok).any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case("main.bmp")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::common::tests::{click, key, press_at, release};
    use protolith::x11::{Key, Modifiers};

    fn tree(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("phonolite-dlg-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Albums/One")).unwrap();
        std::fs::create_dir_all(d.join(".hidden")).unwrap();
        for f in ["b.mp3", "a.MP3", "notes.txt", "list.m3u", "Cool.wsz", "Albums/One/t.mp3"] {
            std::fs::write(d.join(f), b"").unwrap();
        }
        d
    }

    fn typed(c: char) -> Event {
        Event::Key {
            window: WindowId::from_xid(1),
            key: Key::Character(c),
            keysym: 0,
            keycode: 0,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }
    }

    #[test]
    fn browsing_folders_and_adding() {
        let d = tree("browse");
        let font = Terminus::REGULAR_12;
        let mut dlg = Dialog::new(WindowId::from_xid(1), Kind::Open, d.clone(), &[]);
        assert_eq!(dlg.list.items, ["Albums/", "a.MP3", "b.mp3", "list.m3u"], "folders first; only what plays");
        assert_eq!(dlg.field.edit.text(), d.to_string_lossy());
        // Double-click the folder: in.
        let y = 34 + 3;
        dlg.handle(&press_at(20, y, 100), &[], &font);
        dlg.handle(&release(20, y), &[], &font);
        dlg.handle(&press_at(20, y, 200), &[], &font);
        assert_eq!(dlg.dir, d.join("Albums"));
        // Up, then pick b.mp3 with the keyboard and press Play.
        let up: Vec<Option<Outcome>> = click(dlg.buttons[0].0.rect().x + 5, 260).iter().map(|e| dlg.handle(e, &[], &font)).collect();
        assert!(up.iter().all(Option::is_none));
        assert_eq!(dlg.dir, d);
        dlg.handle(&key(Key::Down), &[], &font);
        dlg.handle(&key(Key::Down), &[], &font);
        dlg.handle(&key(Key::Down), &[], &font);
        let play = dlg.buttons[2].0.rect();
        let out: Vec<Option<Outcome>> = click(play.x + 5, play.y + 5).iter().map(|e| dlg.handle(e, &[], &font)).collect();
        assert_eq!(out[1], Some(Outcome::Add(vec![d.join("b.mp3")], true)));
        assert_eq!(dlg.handle(&key(Key::Escape), &[], &font), Some(Outcome::Close));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn typing_a_path_and_enter() {
        let d = tree("type");
        let font = Terminus::REGULAR_12;
        let mut dlg = Dialog::new(WindowId::from_xid(1), Kind::Skin, d.clone(), &[]);
        assert_eq!(dlg.list.items, ["Albums/", "Cool.wsz"]);
        dlg.field.edit.set_text(&d.join("Cool.wsz").to_string_lossy());
        assert_eq!(dlg.handle(&key(Key::Enter), &[], &font), Some(Outcome::Skin(d.join("Cool.wsz"))));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn jump_filters_as_you_type() {
        let font = Terminus::REGULAR_12;
        let mut entries: Vec<Entry> = ["/m/Alpha Song.mp3", "/m/Beta Tune.mp3", "/m/alphabet soup.mp3"].iter().map(|p| Entry::new(*p)).collect();
        for (i, e) in entries.iter_mut().enumerate() {
            e.id = 10 + i as u64;
        }
        let mut dlg = Dialog::new(WindowId::from_xid(1), Kind::Jump, PathBuf::from("/"), &entries);
        assert_eq!(dlg.list.items.len(), 3);
        for c in "alp so".chars() {
            dlg.handle(&typed(c), &entries, &font);
        }
        assert_eq!(dlg.list.items, ["1. Alpha Song", "3. alphabet soup"]);
        dlg.handle(&key(Key::Down), &entries, &font);
        assert_eq!(dlg.handle(&key(Key::Enter), &entries, &font), Some(Outcome::PlayEntry(12)));
    }

    #[test]
    fn url_and_save() {
        let font = Terminus::REGULAR_12;
        let mut dlg = Dialog::new(WindowId::from_xid(1), Kind::Url, PathBuf::from("/"), &[]);
        assert_eq!(dlg.handle(&key(Key::Enter), &[], &font), None, "nothing typed yet");
        for c in "radio.example/x".chars() {
            dlg.handle(&typed(c), &[], &font);
        }
        assert_eq!(dlg.handle(&key(Key::Enter), &[], &font), Some(Outcome::AddUrl("http://radio.example/x".into(), true)));
        let d = tree("save");
        let mut dlg = Dialog::new(WindowId::from_xid(1), Kind::SaveList, d.clone(), &[]);
        assert_eq!(dlg.list.items, ["Albums/", "list.m3u"]);
        assert_eq!(dlg.handle(&key(Key::Enter), &[], &font), Some(Outcome::SaveList(d.join("playlist.m3u"))));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
