//! The application: three borderless windows — the main window (shaped by
//! the skin), the equalizer and the playlist — that snap together and to
//! the screen's edges as classic players' do (see [`snap`]), plus popup
//! menus and dialog windows; the player and playlist behind them.
//!
//! phonolite moves its windows itself, following the pointer, rather than
//! handing the drag to the window manager: only it knows where its other
//! windows are to snap to, and which are docked to the main window and
//! must move with it.

pub mod common;
pub mod dialog;
pub mod eq_pane;
pub mod main_pane;
pub mod pl_pane;
pub mod popup;
pub mod snap;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use protolith::canvas::{Blit, Canvas};
use protolith::ui::FieldStyle;
use protolith::x11::shape::rects_from_mask;
use protolith::x11::{Connection, Event, Key, MouseButton, WindowId, WindowOptions};
use protolith::Rect;
use snap::Bounds;

use crate::config::Config;
use crate::eq::{Equalizer, PRESETS};
use crate::player::{Decoding, Output, Player, Status, Update};
use crate::playlist::{self, format_time, Entry, Playlist};
use crate::skin::layout::{HEIGHT, WIDTH};
use crate::skin::{self, Skin};
use crate::vis::{Mode, FFT_SIZE};
use common::{local, Action, Fonts, MenuKind, View};
use dialog::{Dialog, Kind, Outcome};
use eq_pane::EqPane;
use main_pane::MainPane;
use pl_pane::PlaylistPane;
use popup::{Cmd, Menu};

/// How the program was started.
#[derive(Debug, Clone)]
pub struct Options {
    /// A skin to wear (overrides the saved one).
    pub skin: Option<PathBuf>,
    /// What decodes: mpg123 or the built-in decoder.
    pub decoding: Decoding,
    /// Where the sound may go, tried in order.
    pub outputs: Vec<Output>,
    /// Animation frames per second while playing.
    pub fps: u32,
    /// Files, folders, playlists or URLs to play (replacing the saved
    /// playlist).
    pub files: Vec<PathBuf>,
    /// Where settings live; `None` keeps nothing.
    pub config_dir: Option<PathBuf>,
}

/// One of the three windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    /// The main window.
    Main,
    /// The equalizer.
    Eq,
    /// The playlist.
    Pl,
}

impl Pane {
    /// All three, in order.
    pub const ALL: [Pane; 3] = [Pane::Main, Pane::Eq, Pane::Pl];
}

/// A window and what it shows.
struct Win {
    id: WindowId,
    /// The pane as last drawn, at 1x.
    canvas: Canvas,
    /// What the window shows now, at 1x.
    shown: Canvas,
    /// The scaled copy sent at double size.
    scaled: Canvas,
    /// Its top-left corner on the screen.
    pos: (i32, i32),
    /// Send all of it next frame.
    full: bool,
}

/// A window being dragged, with what is docked to it.
struct Drag {
    /// Where the pointer was pressed (screen coordinates).
    from: (i32, i32),
    /// The windows moving, and where each started.
    moving: Vec<(Pane, (i32, i32))>,
}

/// The running application.
pub struct App {
    /// The display connection.
    pub conn: Connection,
    /// The main window.
    pub win: WindowId,
    wins: [Win; 3],
    drag: Option<Drag>,
    /// Until when to hold newly mapped windows where they belong: some
    /// window managers place them their own way first (openbox centers
    /// transient windows on their owner; Fluxbox may ignore the position
    /// asked for), but do move them when asked afterwards.
    settle: Option<Instant>,
    /// Playback.
    pub player: Player,
    /// The playlist.
    pub playlist: Playlist,
    skin: Skin,
    fonts: Fonts,
    /// The main pane.
    pub main: MainPane,
    /// The equalizer pane.
    pub eq: EqPane,
    /// The playlist pane.
    pub pl: PlaylistPane,
    /// Settings.
    pub cfg: Config,
    config_dir: Option<PathBuf>,
    fps: u32,
    grab: Option<Pane>,
    keys_to: Pane,
    /// Which windows have the keyboard focus.
    focused: [bool; 3],
    popup: Option<Menu>,
    /// When the popup opened, and whether it has the keyboard (without a
    /// window manager, a new window takes the focus).
    popup_since: Instant,
    popup_focused: bool,
    dialog: Option<Dialog>,
    message: Option<(String, Instant)>,
    errors_in_row: u32,
    started: Instant,
    last_tick: Instant,
    scroll_acc: Duration,
    /// Set when the user asked to quit.
    pub quit: bool,
    /// Panes an event reached since the last frame (main, eq, playlist).
    dirty: [bool; 3],
    /// What the eq and playlist panes last showed, summarized.
    keys: [String; 3],
    rows_version: u64,
}

impl App {
    /// Open the window and get ready.
    pub fn new(mut conn: Connection, opts: Options) -> protolith::Result<App> {
        let mut cfg = opts.config_dir.as_deref().map(Config::load).unwrap_or_default();
        if !cfg.eq_auto {
            cfg.eq = Equalizer::default();
        }
        let mut message = None;
        let skin_path = opts.skin.clone().or_else(|| cfg.skin.clone().map(PathBuf::from));
        let skin = match &skin_path {
            Some(p) => match skin::load(p) {
                Ok(s) => {
                    cfg.skin = Some(p.to_string_lossy().into_owned());
                    s
                }
                Err(e) => {
                    message = Some((e, Instant::now()));
                    cfg.skin = None;
                    skin::builtin::skin()
                }
            },
            None => skin::builtin::skin(),
        };
        let waker = Arc::new(Mutex::new(conn.waker().ok()));
        let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if let Some(w) = waker.lock().unwrap().as_ref() {
                let _ = w.wake();
            }
        });
        let mut player = Player::new(opts.decoding.clone(), opts.outputs.clone(), notify);
        player.set_volume(cfg.volume);
        player.set_balance(cfg.balance);
        let _ = player.set_equalizer(cfg.eq.clone());

        let mut playlist = Playlist::default();
        playlist.repeat = cfg.repeat;
        let autoplay = !opts.files.is_empty();
        if autoplay {
            let (entries, errors) = playlist::expand(&opts.files);
            playlist.extend(entries);
            if let Some(e) = errors.first() {
                message = Some((e.clone(), Instant::now()));
            }
        } else if let Some(d) = &opts.config_dir {
            if let Ok(entries) = playlist::read_list(&d.join("playlist.m3u")) {
                playlist.extend(entries);
                playlist.set_current(cfg.current);
            }
        }
        playlist.set_shuffle(cfg.shuffle);

        let scale = if cfg.double { 2 } else { 1 };
        let pl = PlaylistPane::new(cfg.pl_height);
        let sizes = [(WIDTH, HEIGHT), (WIDTH, HEIGHT), (WIDTH, pl.height())].map(|(w, h)| (w * scale, h * scale));
        let positions = placement(&cfg, sizes, conn.screen_size());
        let mut ids = Vec::new();
        for (i, (title, shown)) in [("phonolite", true), ("phonolite equalizer", cfg.show_eq), ("phonolite playlist", cfg.show_pl)].into_iter().enumerate() {
            let ((w, h), (x, y)) = (sizes[i], positions[i]);
            let mut o = WindowOptions::new(w, h)
                .title(title)
                .class("phonolite", "Phonolite")
                .position(x, y)
                .decorations(false)
                .resizable(false)
                .always_on_top(cfg.on_top)
                .visible(shown);
            if let Some(&main) = ids.first() {
                // The window manager's cue that these belong to the main one.
                o = o.transient_for(main);
            }
            let id = conn.create_window(o)?;
            let _ = conn.set_icon(id, &icon().frame());
            ids.push(id);
        }
        let win = ids[0];
        let wins = [0, 1, 2].map(|i| Win {
            id: ids[i],
            canvas: Canvas::new(1, 1),
            shown: Canvas::new(1, 1),
            scaled: Canvas::new(1, 1),
            pos: positions[i],
            full: true,
        });
        let fonts = Fonts::new(&skin);
        let mut eq = EqPane::new();
        eq.set(&cfg.eq, cfg.eq_auto);
        let mut main = MainPane::new();
        main.spectrum = crate::vis::Spectrum::with_rate(opts.decoding.rate());
        let now = Instant::now();
        let mut app = App {
            conn,
            win,
            wins,
            drag: None,
            settle: Some(Instant::now() + SETTLE),
            player,
            playlist,
            skin,
            fonts,
            main,
            eq,
            pl,
            cfg,
            config_dir: opts.config_dir,
            fps: opts.fps.clamp(1, 100),
            grab: None,
            keys_to: Pane::Main,
            focused: [true, false, false],
            popup: None,
            popup_since: now,
            popup_focused: false,
            dialog: None,
            message,
            errors_in_row: 0,
            started: now,
            last_tick: now,
            scroll_acc: Duration::ZERO,
            quit: false,
            dirty: [true; 3],
            keys: Default::default(),
            rows_version: 0,
        };
        app.refresh_rows();
        app.relayout();
        // Widgets show the settings from the start (a toggle clicked before
        // the first frame must flip the right way).
        let view = app.view();
        app.main.sync(&view);
        if autoplay {
            if let Some(i) = app.playlist.next(false) {
                app.play_index(i);
            }
        }
        Ok(app)
    }

    fn scale(&self) -> u32 {
        if self.cfg.double {
            2
        } else {
            1
        }
    }

    /// A pane's size at 1x.
    fn pane_size(&self, p: Pane) -> (u32, u32) {
        match p {
            Pane::Main | Pane::Eq => (WIDTH, HEIGHT),
            Pane::Pl => (WIDTH, self.pl.height()),
        }
    }

    /// Whether a window is shown.
    pub fn is_shown(&self, p: Pane) -> bool {
        match p {
            Pane::Main => true,
            Pane::Eq => self.cfg.show_eq,
            Pane::Pl => self.cfg.show_pl,
        }
    }

    /// Where a window is on the screen, and its size there.
    pub fn bounds(&self, p: Pane) -> Bounds {
        let (w, h) = self.pane_size(p);
        let (s, (x, y)) = (self.scale() as i32, self.wins[p as usize].pos);
        Bounds::new(x, y, w as i32 * s, h as i32 * s)
    }

    /// The windows shown, by pane (`None`: hidden).
    fn shown_bounds(&self) -> [Option<Bounds>; 3] {
        Pane::ALL.map(|p| self.is_shown(p).then(|| self.bounds(p)))
    }

    /// A window's X id.
    pub fn window(&self, p: Pane) -> WindowId {
        self.wins[p as usize].id
    }

    fn pane_of(&self, w: WindowId) -> Option<Pane> {
        Pane::ALL.into_iter().find(|&p| self.wins[p as usize].id == w)
    }

    fn move_window(&mut self, p: Pane, pos: (i32, i32)) {
        let w = &mut self.wins[p as usize];
        if w.pos != pos {
            w.pos = pos;
            let _ = self.conn.set_position(w.id, pos.0, pos.1);
        }
    }

    /// Ask the server where the shown windows really are: a window
    /// manager may have placed or moved them.
    fn refresh_positions(&mut self) {
        if self.settle.is_some() {
            return; // where they are may still be the window manager's idea
        }
        for p in Pane::ALL {
            if self.is_shown(p) {
                if let Ok(pos) = self.conn.window_position(self.wins[p as usize].id) {
                    self.wins[p as usize].pos = pos;
                }
            }
        }
    }

    fn set_shown(&mut self, p: Pane, on: bool) {
        match p {
            Pane::Main => return,
            Pane::Eq => self.cfg.show_eq = on,
            Pane::Pl => self.cfg.show_pl = on,
        }
        let id = self.wins[p as usize].id;
        let _ = if on { self.conn.show(id) } else { self.conn.hide(id) };
        if on {
            self.settle = Some(Instant::now() + SETTLE);
        }
        self.wins[p as usize].full = true;
    }

    /// Sizes, shape and canvases.
    fn relayout(&mut self) {
        self.cfg.pl_height = self.pl.height();
        let s = self.scale();
        for p in Pane::ALL {
            let (w, h) = self.pane_size(p);
            let win = &mut self.wins[p as usize];
            if win.canvas.width() != w || win.canvas.height() != h {
                win.canvas = Canvas::new(w, h);
                win.shown = Canvas::new(w, h);
            }
            if s > 1 && (win.scaled.width() != w * s || win.scaled.height() != h * s) {
                win.scaled = Canvas::new(w * s, h * s);
            }
            win.full = true;
            if self.conn.window_size(win.id).ok() != Some((w * s, h * s)) {
                let _ = self.conn.set_size(win.id, w * s, h * s);
            }
        }
        // The skin's region shapes the main window.
        if self.conn.supports_shape() {
            match &self.skin.region {
                Some(poly) => {
                    let mask = skin::region_mask(poly, WIDTH, HEIGHT);
                    let rects = rects_from_mask(WIDTH * s, HEIGHT * s, |x, y| mask[((y / s) * WIDTH + x / s) as usize]);
                    let _ = self.conn.set_shape(self.win, &rects);
                }
                None => {
                    let _ = self.conn.clear_shape(self.win);
                }
            }
        }
    }

    // ------------------------------------------------------------ moving

    /// Put back any shown window the window manager placed elsewhere.
    fn hold_places(&mut self) {
        for p in Pane::ALL {
            let (id, pos) = (self.wins[p as usize].id, self.wins[p as usize].pos);
            if self.is_shown(p) && self.conn.window_position(id).is_ok_and(|at| at != pos) {
                let _ = self.conn.set_position(id, pos.0, pos.1);
            }
        }
    }

    /// The pointer went down on `p`'s title bar at `from` (screen
    /// coordinates): the main window takes along everything docked to it,
    /// directly or through others; the others go alone, leaving the group.
    fn start_drag(&mut self, p: Pane, from: (i32, i32)) {
        self.refresh_positions();
        let moving = if p == Pane::Main { snap::group(&self.shown_bounds(), 0) } else { vec![p as usize] };
        self.drag = Some(Drag { from, moving: moving.into_iter().map(|i| (Pane::ALL[i], self.wins[i].pos)).collect() });
    }

    /// The pointer is at `to`: move the dragged windows, snapped.
    fn drag_to(&mut self, to: (i32, i32)) {
        let Some(d) = &self.drag else { return };
        let (dx, dy) = (to.0 - d.from.0, to.1 - d.from.1);
        let moving: Vec<(Pane, (i32, i32))> = d.moving.iter().map(|&(p, (x, y))| (p, (x + dx, y + dy))).collect();
        let want: Vec<Bounds> = moving.iter().map(|&(p, (x, y))| self.bounds(p).at(x, y)).collect();
        let others: Vec<Bounds> = Pane::ALL.into_iter().filter(|p| self.is_shown(*p) && !moving.iter().any(|m| m.0 == *p)).map(|p| self.bounds(p)).collect();
        let (sw, sh) = self.conn.screen_size();
        let (sx, sy) = snap::snap(&want, &others, Bounds::new(0, 0, sw as i32, sh as i32));
        for (p, (x, y)) in moving {
            self.move_window(p, (x + sx, y + sy));
        }
    }

    /// Double size on or off: windows docked to the main one stay docked
    /// (their offsets from it scale too); the rest stay where they are.
    fn toggle_double(&mut self) {
        self.refresh_positions();
        // Hidden windows count: one shown later belongs where it was.
        let all = Pane::ALL.map(|p| Some(self.bounds(p)));
        let docked = snap::group(&all, 0);
        let old = self.scale() as i32;
        self.cfg.double = !self.cfg.double;
        let new = self.scale() as i32;
        let (mx, my) = self.wins[0].pos;
        for i in docked.into_iter().skip(1) {
            let (x, y) = self.wins[i].pos;
            self.move_window(Pane::ALL[i], (mx + (x - mx) * new / old, my + (y - my) * new / old));
        }
        self.relayout();
    }

    /// A new playlist height: what was docked under the playlist stays
    /// docked.
    fn resize_playlist(&mut self, h: u32) {
        let before = self.bounds(Pane::Pl);
        self.pl.set_height(h);
        let after = self.bounds(Pane::Pl);
        for p in [Pane::Main, Pane::Eq] {
            let b = self.bounds(p);
            if after.h != before.h && b.y == before.bottom() && b.x < before.right() && before.x < b.right() {
                self.move_window(p, (b.x, b.y + after.h - before.h));
            }
        }
        self.relayout();
    }

    fn set_skin(&mut self, skin: Skin) {
        if !skin.fallbacks.is_empty() && skin.name != "built-in" {
            self.message = Some((format!("{}: {} from the built-in skin", skin.name, skin.fallbacks.len()), Instant::now()));
        }
        self.fonts = Fonts::new(&skin);
        self.skin = skin;
        self.relayout();
    }

    fn refresh_rows(&mut self) {
        self.rows_version += 1;
        let rows = self.playlist.entries.iter().map(Entry::display).collect();
        self.pl.set_rows(rows);
    }

    /// Show a message in the title for a few seconds, and in full on
    /// stderr (the title has room for little, and scrolls away).
    fn say(&mut self, text: impl Into<String>) {
        let text = text.into();
        eprintln!("phonolite: {text}");
        self.message = Some((text, Instant::now()));
    }

    // ------------------------------------------------------------ playing

    /// The playlist index of what is heard (the playlist's current entry
    /// may already be the queued next one).
    fn heard_index(&self) -> Option<usize> {
        self.player.current().and_then(|(_, id)| self.playlist.index_of(id))
    }

    fn play_index(&mut self, i: usize) {
        let Some(e) = self.playlist.entries.get(i) else { return };
        let (path, id) = (e.path.clone(), e.id);
        self.playlist.set_current(Some(i));
        self.pl.show_row(i);
        self.main.reset_marquee();
        if let Err(e) = self.player.play(&path, id) {
            self.say(e.to_string());
        }
    }

    fn handle_updates(&mut self) {
        for u in self.player.poll() {
            match u {
                Update::TrackChanged(id) => {
                    if let Some(i) = self.playlist.index_of(id) {
                        self.playlist.set_current(Some(i));
                    }
                    self.main.reset_marquee();
                    self.update_entry();
                }
                Update::Info => self.update_entry(),
                Update::NeedNext(id) => {
                    if self.errors_in_row >= 5 {
                        self.say("too many errors in a row: stopped");
                        continue;
                    }
                    if let Some(i) = self.playlist.index_of(id) {
                        self.playlist.set_current(Some(i));
                    }
                    if let Some(n) = self.playlist.next(false) {
                        let e = &self.playlist.entries[n];
                        let (path, nid) = (e.path.clone(), e.id);
                        if let Err(e) = self.player.enqueue(&path, nid) {
                            self.say(e.to_string());
                        }
                    }
                }
                Update::Ended => {
                    // The current entry stays where it was.
                    if let Some(i) = self.heard_index() {
                        self.playlist.set_current(Some(i));
                    }
                }
                Update::Status => {}
                Update::Error(e) => {
                    self.errors_in_row += 1;
                    self.say(e);
                }
            }
        }
        if self.player.status() == Status::Playing && self.player.position() > 1.0 {
            self.errors_in_row = 0;
        }
    }

    /// What mpg123 learned about the heard track goes into its entry.
    fn update_entry(&mut self) {
        let Some((t, id)) = self.player.current() else { return };
        let (title, duration) = (t.tags.display(), t.duration);
        let Some(i) = self.playlist.index_of(id) else { return };
        let e = &mut self.playlist.entries[i];
        let mut changed = false;
        if title.is_some() && e.title != title {
            e.title = title;
            changed = true;
        }
        if duration.is_some() && e.duration.map(f64::round) != duration.map(f64::round) {
            e.duration = duration;
            changed = true;
        }
        if changed {
            self.refresh_rows();
        }
        let name = self.playlist.entries[i].display();
        let _ = self.conn.set_title(self.win, &format!("{}. {name} - phonolite", i + 1));
    }

    // ------------------------------------------------------------ actions

    fn act(&mut self, a: Action) {
        self.dirty = [true; 3];
        match a {
            Action::Previous | Action::Next => {
                if let Some(i) = self.heard_index() {
                    self.playlist.set_current(Some(i));
                }
                let n = if a == Action::Next { self.playlist.next(true) } else { self.playlist.previous() };
                if let Some(i) = n {
                    self.play_index(i);
                }
            }
            Action::Play => match self.player.status() {
                Status::Paused => {
                    let _ = self.player.toggle_pause();
                }
                Status::Playing => {
                    let _ = self.player.seek(0.0);
                }
                Status::Stopped => {
                    let i = self.pl.list.selected().or(self.playlist.current()).or((!self.playlist.entries.is_empty()).then_some(0));
                    match i {
                        Some(i) => self.play_index(i),
                        None => self.open_dialog(Kind::Open),
                    }
                }
            },
            Action::Pause => {
                if let Err(e) = self.player.toggle_pause() {
                    self.say(e.to_string());
                }
            }
            Action::Stop => self.player.stop(),
            Action::Open => self.open_dialog(Kind::Open),
            Action::Jump => self.open_dialog(Kind::Jump),
            Action::Seek(s) => {
                if let Err(e) = self.player.seek(s) {
                    self.say(e.to_string());
                }
            }
            Action::Volume(v) => {
                self.player.set_volume(v);
                self.cfg.volume = v;
            }
            Action::Balance(b) => {
                self.player.set_balance(b);
                self.cfg.balance = b;
            }
            Action::Shuffle(on) => {
                self.playlist.set_shuffle(on);
                self.cfg.shuffle = on;
            }
            Action::Repeat(on) => {
                self.playlist.repeat = on;
                self.cfg.repeat = on;
            }
            Action::ShowEq(on) => self.set_shown(Pane::Eq, on),
            Action::ShowPlaylist(on) => self.set_shown(Pane::Pl, on),
            Action::Minimize => {
                for p in Pane::ALL {
                    if self.is_shown(p) {
                        let _ = self.conn.minimize(self.wins[p as usize].id);
                    }
                }
            }
            Action::Quit => self.quit = true,
            Action::DragWindow(x, y) => self.start_drag(self.grab.unwrap_or(Pane::Main), (x, y)),
            Action::Menu(kind, x, y) => self.open_menu(kind, x, y),
            Action::AlwaysOnTop => {
                self.cfg.on_top = !self.cfg.on_top;
                for w in &self.wins {
                    let _ = self.conn.set_always_on_top(w.id, self.cfg.on_top);
                }
            }
            Action::DoubleSize => self.toggle_double(),
            Action::NextVisMode => self.cfg.vis = self.cfg.vis.next(),
            Action::TimeMode => self.cfg.remaining = !self.cfg.remaining,
            Action::Equalizer(eq) => {
                if let Err(e) = self.player.set_equalizer(eq.clone()) {
                    self.say(e.to_string());
                }
                self.cfg.eq = eq;
            }
            Action::EqAuto(on) => self.cfg.eq_auto = on,
            Action::PlayIndex(i) => self.play_index(i),
            Action::Remove(i) => {
                self.playlist.remove(i);
                self.refresh_rows();
            }
            Action::ResizePlaylist(h) => self.resize_playlist(h),
        }
    }

    fn run(&mut self, c: Cmd) {
        let sel = self.pl.list.selected();
        match c {
            Cmd::Act(a) => self.act(a),
            Cmd::Preset(i) => {
                if let Some(p) = PRESETS.get(i) {
                    let eq = Equalizer { on: true, preamp: p.preamp, bands: p.bands };
                    self.eq.set(&eq, self.cfg.eq_auto);
                    self.act(Action::Equalizer(eq));
                }
            }
            Cmd::AddUrl => self.open_dialog(Kind::Url),
            Cmd::RemoveSelected => {
                if let Some(i) = sel {
                    self.act(Action::Remove(i));
                }
            }
            Cmd::Crop => {
                if let Some(i) = sel {
                    let keep = self.playlist.entries[i].id;
                    while let Some(j) = self.playlist.entries.iter().position(|e| e.id != keep) {
                        self.playlist.remove(j);
                    }
                    self.pl.list.select(Some(0));
                    self.refresh_rows();
                }
            }
            Cmd::Clear | Cmd::NewList => {
                self.playlist.clear();
                self.pl.list.select(None);
                self.refresh_rows();
            }
            Cmd::SelectCurrent => {
                if let Some(i) = self.playlist.current() {
                    self.pl.list.select(Some(i));
                    self.pl.show_row(i);
                }
            }
            Cmd::SelectNone => self.pl.list.select(None),
            Cmd::SortTitle | Cmd::SortPath | Cmd::Reverse | Cmd::Randomize => {
                match c {
                    Cmd::SortTitle => self.playlist.sort_by_title(),
                    Cmd::SortPath => self.playlist.sort_by_path(),
                    Cmd::Reverse => self.playlist.reverse(),
                    _ => self.playlist.randomize(),
                }
                self.refresh_rows();
            }
            Cmd::SaveList => self.open_dialog(Kind::SaveList),
            Cmd::LoadList => self.open_dialog(Kind::LoadList),
            Cmd::LoadSkin => self.open_dialog(Kind::Skin),
            Cmd::BuiltinSkin => {
                self.cfg.skin = None;
                self.set_skin(skin::builtin::skin());
            }
        }
    }

    fn open_menu(&mut self, kind: MenuKind, x: i32, y: i32) {
        self.close_popup();
        let tick = |on: bool, s: &str| format!("{} {s}", if on { "*" } else { " " });
        let items: Vec<(String, Cmd)> = match kind {
            MenuKind::Main => vec![
                ("Add files...".into(), Cmd::Act(Action::Open)),
                ("Add a URL...".into(), Cmd::AddUrl),
                ("Jump to file...".into(), Cmd::Act(Action::Jump)),
                ("Choose a skin...".into(), Cmd::LoadSkin),
                ("Built-in skin".into(), Cmd::BuiltinSkin),
                (tick(self.cfg.show_eq, "Equalizer"), Cmd::Act(Action::ShowEq(!self.cfg.show_eq))),
                (tick(self.cfg.show_pl, "Playlist"), Cmd::Act(Action::ShowPlaylist(!self.cfg.show_pl))),
                (tick(self.cfg.on_top, "Always on top"), Cmd::Act(Action::AlwaysOnTop)),
                (tick(self.cfg.double, "Double size"), Cmd::Act(Action::DoubleSize)),
                (
                    format!(
                        "Visualizer: {}",
                        match self.cfg.vis {
                            Mode::Spectrum => "spectrum",
                            Mode::Scope => "scope",
                            Mode::Off => "off",
                        }
                    ),
                    Cmd::Act(Action::NextVisMode),
                ),
                ("Quit".into(), Cmd::Act(Action::Quit)),
            ],
            MenuKind::Presets => PRESETS.iter().enumerate().map(|(i, p)| (p.name.to_string(), Cmd::Preset(i))).collect(),
            MenuKind::Add => vec![("Add files...".into(), Cmd::Act(Action::Open)), ("Add a URL...".into(), Cmd::AddUrl)],
            MenuKind::Remove => {
                vec![("Remove selected".into(), Cmd::RemoveSelected), ("Crop to selected".into(), Cmd::Crop), ("Remove all".into(), Cmd::Clear)]
            }
            MenuKind::Select => vec![("Select playing".into(), Cmd::SelectCurrent), ("Select none".into(), Cmd::SelectNone)],
            MenuKind::Misc => vec![
                ("Sort by title".into(), Cmd::SortTitle),
                ("Sort by path".into(), Cmd::SortPath),
                ("Reverse".into(), Cmd::Reverse),
                ("Randomize".into(), Cmd::Randomize),
            ],
            MenuKind::List => vec![("New list".into(), Cmd::NewList), ("Load list...".into(), Cmd::LoadList), ("Save list...".into(), Cmd::SaveList)],
        };
        let labels: Vec<String> = items.iter().map(|(l, _)| l.clone()).collect();
        let (w, h) = Menu::size(&labels, &self.fonts.list);
        let (sw, sh) = self.conn.screen_size();
        // Keep it on screen; open upward from buttons near the bottom.
        let x = x.min(sw as i32 - w as i32).max(0);
        let y = if y + h as i32 > sh as i32 { (y - h as i32).max(0) } else { y };
        let Ok(win) = self.conn.create_window(WindowOptions::new(w, h).position(x, y).override_redirect(true).decorations(false).resizable(false)) else {
            return;
        };
        let mut m = Menu::new(win, items, &self.fonts.list);
        m.draw(&self.skin.playlist, &self.fonts.list);
        let _ = self.conn.present(win, &m.canvas.frame());
        self.popup = Some(m);
        self.popup_since = Instant::now();
        self.popup_focused = false;
    }

    fn close_popup(&mut self) {
        if let Some(m) = self.popup.take() {
            let _ = self.conn.destroy_window(m.win);
        }
    }

    fn open_dialog(&mut self, kind: Kind) {
        self.close_popup();
        if let Some(d) = self.dialog.take() {
            let _ = self.conn.destroy_window(d.win);
        }
        let dir = self
            .cfg
            .last_dir
            .clone()
            .map(PathBuf::from)
            .filter(|d| d.is_dir())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        let (w, h) = dialog::SIZE;
        let Ok(win) = self.conn.create_window(WindowOptions::new(w, h).title(kind.title()).class("phonolite", "Phonolite").resizable(false)) else { return };
        let mut d = Dialog::new(win, kind, dir, &self.playlist.entries);
        self.draw_dialog(&mut d);
        let _ = self.conn.focus(win);
        self.dialog = Some(d);
    }

    fn draw_dialog(&mut self, d: &mut Dialog) {
        let st = popup::style(&self.skin.playlist);
        let fs = FieldStyle {
            background: Some(st.background),
            bevel: Some((st.shadow, st.light)),
            text: st.text,
            selection: st.accent,
            selected_text: st.accent_text,
            cursor: Some(st.accent_text),
        };
        d.draw(&st, &fs, &self.fonts.list);
        let _ = self.conn.present(d.win, &d.canvas.frame());
    }

    fn dialog_outcome(&mut self, o: Outcome) {
        if let Some(d) = &self.dialog {
            self.cfg.last_dir = Some(d.dir.to_string_lossy().into_owned());
        }
        let close = |app: &mut App| {
            if let Some(d) = app.dialog.take() {
                let _ = app.conn.destroy_window(d.win);
            }
        };
        match o {
            Outcome::Close => close(self),
            Outcome::Add(paths, play) => {
                close(self);
                let first = self.playlist.entries.len();
                let (entries, errors) = playlist::expand(&paths);
                let added = entries.len();
                self.playlist.extend(entries);
                self.refresh_rows();
                if let Some(e) = errors.first() {
                    self.say(e.clone());
                }
                if play && added > 0 {
                    self.play_index(first);
                }
            }
            Outcome::AddUrl(url, play) => {
                close(self);
                let first = self.playlist.entries.len();
                self.playlist.extend([Entry::new(url)]);
                self.refresh_rows();
                if play {
                    self.play_index(first);
                }
            }
            Outcome::Skin(p) => {
                close(self);
                match skin::load(&p) {
                    Ok(s) => {
                        self.cfg.skin = Some(p.to_string_lossy().into_owned());
                        self.set_skin(s);
                    }
                    Err(e) => self.say(e),
                }
            }
            Outcome::LoadList(p) => {
                close(self);
                match playlist::read_list(&p) {
                    Ok(entries) => {
                        self.player.stop();
                        self.playlist.clear();
                        self.playlist.extend(entries);
                        self.pl.list.select(None);
                        self.refresh_rows();
                    }
                    Err(e) => self.say(format!("{}: {e}", p.display())),
                }
            }
            Outcome::SaveList(p) => {
                close(self);
                let p = if p.extension().is_none() { p.with_extension("m3u") } else { p };
                if let Err(e) = std::fs::write(&p, self.playlist.to_m3u()) {
                    self.say(format!("{}: {e}", p.display()));
                } else {
                    self.say(format!("saved {}", p.display()));
                }
            }
            Outcome::PlayEntry(id) => {
                close(self);
                if let Some(i) = self.playlist.index_of(id) {
                    self.play_index(i);
                }
            }
        }
    }

    // ------------------------------------------------------------ events

    /// Handle one event.
    pub fn handle(&mut self, e: Event) {
        let Some(w) = e.window() else { return };
        if self.popup.as_ref().is_some_and(|m| m.win == w) {
            match e {
                Event::Focused { focused, .. } => self.popup_focused = focused,
                Event::Key { key: Key::Escape, pressed: true, .. } => self.close_popup(),
                _ => {}
            }
            let picked = self.popup.as_mut().and_then(|m| m.handle(&e));
            if let Some(c) = picked {
                self.close_popup();
                self.run(c);
            } else if let Some(m) = &mut self.popup {
                m.draw(&self.skin.playlist, &self.fonts.list);
                let _ = self.conn.present(m.win, &m.canvas.frame());
            }
            return;
        }
        if self.dialog.as_ref().is_some_and(|d| d.win == w) {
            let entries = self.playlist.entries.clone();
            let mut d = self.dialog.take().expect("checked");
            let out = d.handle(&e, &entries, &self.fonts.list);
            if out.is_none() {
                self.draw_dialog(&mut d);
            }
            self.dialog = Some(d);
            if let Some(o) = out {
                self.dialog_outcome(o);
            }
            return;
        }
        let Some(p) = self.pane_of(w) else { return };
        if self.drag.is_some() {
            match e {
                Event::Motion { root_x, root_y, .. } => return self.drag_to((root_x, root_y)),
                Event::Button { button: MouseButton::Left, pressed: false, root_x, root_y, .. } => {
                    self.drag_to((root_x, root_y));
                    self.drag = None;
                    self.grab = None;
                    return;
                }
                _ => {}
            }
        }
        match e {
            Event::CloseRequested { .. } => match p {
                Pane::Main => self.quit = true,
                Pane::Eq => self.act(Action::ShowEq(false)),
                Pane::Pl => self.act(Action::ShowPlaylist(false)),
            },
            Event::Focused { focused, .. } => {
                self.focused[p as usize] = focused;
                self.dirty[p as usize] = true;
                if focused {
                    self.keys_to = p;
                    self.pl.list.set_focused(p == Pane::Pl);
                }
            }
            Event::Resized { .. } => self.wins[p as usize].full = true,
            Event::Key { pressed: true, .. } => self.key(&e),
            Event::Button { pressed: true, .. } if self.popup.is_some() => self.close_popup(),
            Event::Button { .. } | Event::Motion { .. } | Event::Scroll { .. } => {
                let s = self.scale() as i32;
                self.dirty[p as usize] = true;
                if let Event::Button { pressed: true, .. } = e {
                    self.grab = Some(p);
                    self.keys_to = p;
                    self.pl.list.set_focused(p == Pane::Pl);
                }
                let le = local(&e, s, 0, 0);
                let view = self.view();
                let actions = match p {
                    Pane::Main => self.main.handle(&le, &view),
                    Pane::Eq => self.eq.handle(&le),
                    Pane::Pl => self.pl.handle(&le),
                };
                for a in actions {
                    self.act(a);
                }
                if let Event::Button { pressed: false, .. } = e {
                    self.grab = None;
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, e: &Event) {
        let Event::Key { key, modifiers: m, .. } = *e else { return };
        if key == Key::Escape {
            self.close_popup();
            return;
        }
        // The playlist takes its own keys when it has the keyboard.
        if self.keys_to == Pane::Pl && matches!(key, Key::Up | Key::Down | Key::PageUp | Key::PageDown | Key::Home | Key::End | Key::Enter | Key::Delete) {
            self.dirty[2] = true;
            let le = local(e, 1, 0, 0);
            for a in self.pl.handle(&le) {
                self.act(a);
            }
            return;
        }
        let ch = match key {
            Key::Character(c) => Some(c.to_ascii_lowercase()),
            _ => None,
        };
        let a = match (key, ch) {
            (_, Some('d')) if m.ctrl => Action::DoubleSize,
            (_, Some('a')) if m.ctrl => Action::AlwaysOnTop,
            (_, Some('q')) if m.ctrl => Action::Quit,
            (_, Some('g')) if m.alt => Action::ShowEq(!self.cfg.show_eq),
            (_, Some('e')) if m.alt => Action::ShowPlaylist(!self.cfg.show_pl),
            (_, Some('z')) | (Key::MediaPrevious, _) => Action::Previous,
            (_, Some('x')) => Action::Play,
            (_, Some('c')) | (Key::MediaPause, _) => Action::Pause,
            (Key::MediaPlay, _) => {
                if self.player.status() == Status::Stopped {
                    Action::Play
                } else {
                    Action::Pause
                }
            }
            (_, Some('v')) | (Key::MediaStop, _) => Action::Stop,
            (_, Some('b')) | (Key::MediaNext, _) => Action::Next,
            (_, Some('l')) => Action::Open,
            (_, Some('j')) => Action::Jump,
            (_, Some('s')) => Action::Shuffle(!self.playlist.shuffle()),
            (_, Some('r')) => Action::Repeat(!self.playlist.repeat),
            (Key::Left, _) => Action::Seek((self.player.position() - 5.0).max(0.0)),
            (Key::Right, _) => Action::Seek(self.player.position() + 5.0),
            (Key::Up, _) | (Key::VolumeUp, _) => Action::Volume((self.cfg.volume + 5).min(100)),
            (Key::Down, _) | (Key::VolumeDown, _) => Action::Volume(self.cfg.volume.saturating_sub(5)),
            (Key::Mute, _) => Action::Volume(0),
            _ => return,
        };
        self.act(a);
    }

    // ------------------------------------------------------------ time

    /// How long the loop may sleep before the next tick (`None`: until an
    /// event).
    pub fn timeout(&self) -> Option<Duration> {
        let frame = Duration::from_millis(1000 / u64::from(self.fps));
        if self.message.is_some() || self.popup.is_some() || self.settle.is_some() {
            return Some(frame.max(Duration::from_millis(50)));
        }
        match self.player.status() {
            Status::Playing => Some(frame),
            Status::Paused => Some(Duration::from_millis(250)),
            Status::Stopped => {
                if self.main.spectrum.is_idle() && 5 * self.view().marquee.chars().count() <= 154 {
                    None
                } else {
                    Some(Duration::from_millis(50))
                }
            }
        }
    }

    /// Gather what the windows draw from.
    pub fn view(&self) -> View {
        let status = self.player.status();
        let current = self.player.current();
        let track = current.map(|(t, _)| t);
        let stream = track.and_then(|t| t.stream.as_ref());
        let marquee = match &self.message {
            Some((m, _)) => m.clone(),
            None => {
                let idx = current.and_then(|(_, id)| self.playlist.index_of(id)).or(self.playlist.current());
                match idx.and_then(|i| self.playlist.entries.get(i).map(|e| (i, e))) {
                    Some((i, e)) => {
                        let dur = e.duration.or(track.and_then(|t| t.duration)).map(|d| format!(" ({})", format_time(d))).unwrap_or_default();
                        format!("{}. {}{dur}", i + 1, e.display())
                    }
                    None => format!("PHONOLITE {}", env!("CARGO_PKG_VERSION")),
                }
            }
        };
        View {
            status,
            position: self.player.position(),
            duration: track.and_then(|t| t.duration),
            marquee,
            kbps: stream.map(|s| s.bitrate),
            khz: stream.map(|s| (s.rate + 500) / 1000),
            channels: stream.map(|s| s.channels),
            focused: self.focused[0],
            remaining: self.cfg.remaining,
            blink: self.started.elapsed().as_millis() % 1000 < 600,
            on_top: self.cfg.on_top,
            double: self.cfg.double,
            vis: self.cfg.vis,
            shuffle: self.playlist.shuffle(),
            repeat: self.playlist.repeat,
            show_eq: self.cfg.show_eq,
            show_pl: self.cfg.show_pl,
            volume: self.cfg.volume,
            balance: self.cfg.balance,
        }
    }

    /// Advance: take the player's news, animate.
    pub fn tick(&mut self) {
        self.handle_updates();
        match self.settle {
            Some(until) if Instant::now() < until => self.hold_places(),
            Some(_) => self.settle = None,
            None => {}
        }
        // A click in another program takes the focus from us: close the
        // menu then (after a moment, so a focus hand-off between our own
        // windows does not count).
        if self.popup.is_some() && !self.focused.contains(&true) && !self.popup_focused && self.popup_since.elapsed() > Duration::from_millis(300) {
            self.close_popup();
        }
        let now = Instant::now();
        let dt = now - self.last_tick;
        self.last_tick = now;
        if self.message.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(5)) {
            self.message = None;
            self.main.reset_marquee();
        }
        let view = self.view();
        self.main.sync(&view);
        if !self.eq_dragging() {
            self.eq.set(&self.cfg.eq, self.cfg.eq_auto);
        }
        // The title scrolls 20 px a second, whatever the frame rate.
        self.scroll_acc += dt;
        while self.scroll_acc >= Duration::from_millis(50) {
            self.scroll_acc -= Duration::from_millis(50);
            self.main.tick(&view);
        }
        if self.cfg.vis == Mode::Spectrum {
            let samples = self.player.samples(FFT_SIZE);
            self.main.spectrum.update(samples.as_deref());
        }
    }

    fn eq_dragging(&self) -> bool {
        self.grab == Some(Pane::Eq)
    }

    /// Draw what changed and send it. The main window redraws every time;
    /// the equalizer and playlist only when an event reached them or what
    /// they show changed — during playback that is rare, and redrawing and
    /// comparing them every frame would be most of the work.
    pub fn render(&mut self) -> protolith::Result<()> {
        let view = self.view();
        let samples = if self.cfg.vis == Mode::Scope { self.player.samples(576) } else { None };
        let current = self.heard_index().or(self.playlist.current());
        let shown = Pane::ALL.map(|p| self.is_shown(p));
        let full = [0, 1, 2].map(|i| shown[i] && std::mem::take(&mut self.wins[i].full));
        let e = &self.cfg.eq;
        let eq_key = format!("{}{}{:?}{}{}", e.on, e.preamp.to_bits(), e.bands.map(f64::to_bits), self.cfg.eq_auto, self.focused[1]);
        let mini = if view.status == Status::Stopped { String::new() } else { format!("{}{}", view.position as u64, view.remaining) };
        let pl_key = format!("{}|{current:?}|{mini}|{:?}|{}", self.rows_version, view.status, self.focused[2]);
        let redraw_eq = shown[1] && (full[1] || self.dirty[1] || self.keys[1] != eq_key);
        let redraw_pl = shown[2] && (full[2] || self.dirty[2] || self.keys[2] != pl_key);
        self.keys[1] = eq_key;
        self.keys[2] = pl_key;
        let main_key = self.main.look(&view);
        let redraw_main = full[0] || self.dirty[0] || self.keys[0] != main_key;
        self.keys[0] = main_key;
        self.dirty = [false; 3];

        // Rows drawn, per window.
        let mut bands: [Vec<(u32, u32)>; 3] = Default::default();
        let [mw, ew, pw] = &mut self.wins;
        if redraw_main {
            self.main.draw(&mut mw.canvas, &self.skin, &self.fonts, &view, samples.as_deref());
            bands[0].push((0, HEIGHT));
        } else {
            // Most frames: just the visualizer and the scrolling title.
            use crate::skin::layout::main as m;
            self.main.draw_vis(&mut mw.canvas, &self.skin, &view, samples.as_deref());
            self.main.draw_marquee(&mut mw.canvas, &self.skin, &self.fonts, &view);
            for r in [m::VIS, m::MARQUEE] {
                bands[0].push((r.y as u32, r.height));
            }
        }
        if redraw_eq {
            self.eq.draw(&mut ew.canvas, &self.skin, self.focused[1]);
            bands[1].push((0, HEIGHT));
        }
        if redraw_pl {
            let v = View { focused: self.focused[2], ..view.clone() };
            self.pl.draw(&mut pw.canvas, &self.skin, &self.fonts, &self.playlist.entries, current, &v);
            bands[2].push((0, pw.canvas.height()));
        }
        let s = self.scale();
        for (i, w) in self.wins.iter_mut().enumerate() {
            if !shown[i] {
                continue;
            }
            let rects =
                if full[i] { vec![w.canvas.bounds()] } else { bands[i].iter().flat_map(|&(y, h)| common::damage_rows(&w.shown, &w.canvas, y, h)).collect() };
            for r in &rects {
                if s == 1 {
                    self.conn.present_rect(w.id, &w.canvas.frame(), *r)?;
                } else {
                    let to = Rect::new(r.x * s as i32, r.y * s as i32, r.width * s, r.height * s);
                    w.scaled.blit_with(&w.canvas.frame(), *r, to, Blit::Copy);
                    self.conn.present_rect(w.id, &w.scaled.frame(), to)?;
                }
                // What is on screen now.
                w.shown.blit(&w.canvas.frame(), *r, r.x, r.y);
            }
        }
        Ok(())
    }

    /// One turn of the loop: wait (up to the tick), handle what came,
    /// tick, draw.
    pub fn step(&mut self) -> protolith::Result<()> {
        // Idle, wake once a second anyway: cheap, and nothing can get stuck.
        let first = self.conn.wait_event_timeout(self.timeout().unwrap_or(Duration::from_secs(1)))?;
        if let Some(e) = first {
            self.handle(e);
        }
        while let Some(e) = self.conn.poll_event()? {
            self.handle(e);
        }
        self.tick();
        self.render()
    }

    /// Save settings and the playlist.
    pub fn save(&mut self) {
        let Some(d) = self.config_dir.clone() else { return };
        self.refresh_positions();
        self.cfg.positions = [0, 1, 2].map(|i| Some(self.wins[i].pos));
        self.cfg.current = self.heard_index().or(self.playlist.current());
        if let Err(e) = self.cfg.save(&d) {
            eprintln!("phonolite: saving settings: {e}");
        }
        if let Err(e) = std::fs::write(d.join("playlist.m3u"), self.playlist.to_m3u()) {
            eprintln!("phonolite: saving the playlist: {e}");
        }
    }

    /// A pane drawn from scratch, nothing cached — what its window should
    /// show (for checking the caching in [`App::render`]).
    pub fn render_all(&mut self, p: Pane) -> Canvas {
        let view = self.view();
        let (w, h) = self.pane_size(p);
        let mut c = Canvas::new(w, h);
        match p {
            Pane::Main => {
                let samples = if self.cfg.vis == Mode::Scope { self.player.samples(576) } else { None };
                self.main.draw(&mut c, &self.skin, &self.fonts, &view, samples.as_deref());
            }
            Pane::Eq => self.eq.draw(&mut c, &self.skin, self.focused[1]),
            Pane::Pl => {
                let current = self.heard_index().or(self.playlist.current());
                let v = View { focused: self.focused[2], ..view };
                self.pl.draw(&mut c, &self.skin, &self.fonts, &self.playlist.entries, current, &v);
            }
        }
        c
    }

    /// What was last sent to a window, at 1x.
    pub fn frame(&self, p: Pane) -> &Canvas {
        &self.wins[p as usize].shown
    }

    /// The display scale (2 at double size).
    pub fn scale_factor(&self) -> u32 {
        self.scale()
    }

    /// The skin worn.
    pub fn skin(&self) -> &Skin {
        &self.skin
    }

    /// The text on screen (marquee), for tests.
    pub fn marquee(&self) -> String {
        self.view().marquee
    }

    /// Whether a popup is open.
    pub fn popup_open(&self) -> bool {
        self.popup.is_some()
    }

    /// Whether a dialog is open, and which.
    pub fn dialog_kind(&self) -> Option<Kind> {
        self.dialog.as_ref().map(|d| d.kind)
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.save();
    }
}

/// How long newly mapped windows are held in place.
const SETTLE: Duration = Duration::from_secs(2);

/// Where the windows go: where they were last time (as long as the main
/// window is still on the screen), else stacked, docked, in the middle of
/// the screen — the shown ones first, then the hidden.
fn placement(cfg: &Config, sizes: [(u32, u32); 3], (sw, sh): (u32, u32)) -> [(i32, i32); 3] {
    let order = [true, cfg.show_eq, cfg.show_pl];
    let shown_h: u32 = (0..3).filter(|&i| order[i]).map(|i| sizes[i].1).sum();
    let x = (sw as i32 - sizes[0].0 as i32) / 2;
    let mut y = ((sh as i32 - shown_h as i32) / 2).max(0);
    let mut out = [(x, 0); 3];
    for i in (0..3).filter(|&i| order[i]).chain((0..3).filter(|&i| !order[i])) {
        out[i].1 = y;
        y += sizes[i].1 as i32;
    }
    let main_visible = cfg.positions[0]
        .is_some_and(|(x, y)| Bounds::new(x, y, sizes[0].0 as i32, sizes[0].1 as i32).right() > 8 && x < sw as i32 - 8 && y >= -8 && y < sh as i32 - 8);
    if main_visible {
        for (slot, saved) in out.iter_mut().zip(cfg.positions) {
            if let Some(p) = saved {
                *slot = p;
            }
        }
    }
    out
}

/// A 32x32 icon: a faceted stone on dark.
pub fn icon() -> Canvas {
    let mut c = Canvas::new(32, 32);
    c.fill(0x08100e);
    for y in 0..32i32 {
        let half = 13 - (y - 16).abs() * 13 / 16;
        if half > 0 {
            let shade = 0x3b4642 + ((y as u32 * 3) << 8);
            c.fill_rect(Rect::new(16 - half, y, (2 * half) as u32, 1), shade);
        }
    }
    for (k, col) in [(0, 0x8ff0c8u32), (1, 0x63c9a3), (2, 0x3f9a7a)] {
        c.fill_rect(Rect::new(9 + 5 * k, 10 + 3 * k, 5, 12 - 3 * k as u32), col);
    }
    c.fill_rect(Rect::new(12, 6, 3, 3), 0xffffff);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: [(u32, u32); 3] = [(275, 116), (275, 116), (275, 232)];

    #[test]
    fn first_run_stacks_the_windows_docked_in_the_middle() {
        let cfg = Config::default();
        assert_eq!(placement(&cfg, SIZES, (1024, 768)), [(374, 152), (374, 268), (374, 384)]);
        // The equalizer hidden: the playlist docks to the main window, and
        // the equalizer waits below it.
        let cfg = Config { show_eq: false, ..Config::default() };
        assert_eq!(placement(&cfg, SIZES, (1024, 768)), [(374, 210), (374, 558), (374, 326)]);
        // A screen smaller than the stack: from the top.
        assert_eq!(placement(&Config::default(), SIZES, (640, 400))[0], (182, 0));
    }

    #[test]
    fn saved_places_are_kept_unless_the_main_window_would_be_lost() {
        let saved = [Some((10, 20)), Some((600, 20)), None];
        let cfg = Config { positions: saved, ..Config::default() };
        assert_eq!(placement(&cfg, SIZES, (1024, 768)), [(10, 20), (600, 20), (374, 384)], "a missing one takes its default");
        // Saved on a bigger screen, now off this one: everything resets.
        let cfg = Config { positions: [Some((1500, 900)), Some((1500, 1016)), Some((1500, 1132))], ..Config::default() };
        assert_eq!(placement(&cfg, SIZES, (1024, 768)), placement(&Config::default(), SIZES, (1024, 768)));
        // Partly off the left edge, but still there to grab: kept.
        let cfg = Config { positions: [Some((-200, 100)), None, None], ..Config::default() };
        assert_eq!(placement(&cfg, SIZES, (1024, 768))[0], (-200, 100));
    }
}
