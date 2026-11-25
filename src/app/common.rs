//! What the windows share: actions they ask for, the view they draw from,
//! fonts, and event plumbing.

use protolith::canvas::Canvas;
use protolith::font::{SheetFont, Terminus};
use protolith::x11::Event;
use protolith::Rect;

use crate::eq::Equalizer;
use crate::player::Status;
use crate::skin::layout::{NUMBERS_LAYOUT, TEXT_LAYOUT};
use crate::skin::Skin;
use crate::vis::Mode;

/// Which popup menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKind {
    /// The main menu (title bar button, right click).
    Main,
    /// Equalizer presets.
    Presets,
    /// Playlist ADD.
    Add,
    /// Playlist REM.
    Remove,
    /// Playlist SEL.
    Select,
    /// Playlist MISC.
    Misc,
    /// Playlist LIST.
    List,
}

/// Something a window asks the application to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Previous track.
    Previous,
    /// Play (restart if paused? no: resume if paused, else play the
    /// selected or current track).
    Play,
    /// Toggle pause.
    Pause,
    /// Stop.
    Stop,
    /// Next track.
    Next,
    /// Open the file browser to add (and play) files.
    Open,
    /// Seek to seconds.
    Seek(f64),
    /// Set the volume (0..=100).
    Volume(u32),
    /// Set the balance (-100..=100).
    Balance(i32),
    /// Shuffle on/off.
    Shuffle(bool),
    /// Repeat on/off.
    Repeat(bool),
    /// Show the equalizer.
    ShowEq(bool),
    /// Show the playlist.
    ShowPlaylist(bool),
    /// Minimize the window.
    Minimize,
    /// Quit.
    Quit,
    /// Start moving the window (root coordinates of the press).
    DragWindow(i32, i32),
    /// Open a popup menu at root coordinates.
    Menu(MenuKind, i32, i32),
    /// Toggle always on top.
    AlwaysOnTop,
    /// Toggle double size.
    DoubleSize,
    /// Cycle the visualizer.
    NextVisMode,
    /// Toggle elapsed/remaining time.
    TimeMode,
    /// New equalizer settings.
    Equalizer(Equalizer),
    /// The equalizer's AUTO button.
    EqAuto(bool),
    /// Play playlist entry `i`.
    PlayIndex(usize),
    /// Remove playlist entry `i`.
    Remove(usize),
    /// The playlist window's new height.
    ResizePlaylist(u32),
    /// Open the jump-to-file dialog.
    Jump,
}

/// Everything a window draws from, gathered once a frame.
#[derive(Debug, Clone)]
pub struct View {
    /// Transport.
    pub status: Status,
    /// Seconds into the track.
    pub position: f64,
    /// Track length.
    pub duration: Option<f64>,
    /// The scrolling text.
    pub marquee: String,
    /// Bitrate.
    pub kbps: Option<u32>,
    /// Sample rate in kHz.
    pub khz: Option<u32>,
    /// Channels of the stream.
    pub channels: Option<u8>,
    /// The window has keyboard focus.
    pub focused: bool,
    /// Show remaining instead of elapsed time.
    pub remaining: bool,
    /// Blink phase (paused digits hide when false).
    pub blink: bool,
    /// Always on top.
    pub on_top: bool,
    /// Double size.
    pub double: bool,
    /// Visualizer mode.
    pub vis: Mode,
    /// Shuffle.
    pub shuffle: bool,
    /// Repeat.
    pub repeat: bool,
    /// Equalizer shown.
    pub show_eq: bool,
    /// Playlist shown.
    pub show_pl: bool,
    /// Volume slider position.
    pub volume: u32,
    /// Balance.
    pub balance: i32,
}

impl Default for View {
    fn default() -> View {
        View {
            status: Status::Stopped,
            position: 0.0,
            duration: None,
            marquee: String::new(),
            kbps: None,
            khz: None,
            channels: None,
            focused: true,
            remaining: false,
            blink: true,
            on_top: false,
            double: false,
            vis: Mode::Spectrum,
            shuffle: false,
            repeat: false,
            show_eq: true,
            show_pl: true,
            volume: 75,
            balance: 0,
        }
    }
}

/// The fonts a skin implies.
pub struct Fonts {
    /// text.bmp, 5x6.
    pub text: SheetFont,
    /// numbers.bmp / nums_ex.bmp, 9x13.
    pub numbers: SheetFont,
    /// Playlist and dialogs.
    pub list: Terminus,
}

impl Fonts {
    /// Cut the skin's fonts.
    pub fn new(skin: &Skin) -> Fonts {
        Fonts {
            text: SheetFont::new(skin.text.clone(), 5, 6, TEXT_LAYOUT).with_fallback(' '),
            numbers: SheetFont::new(skin.numbers.clone(), 9, 13, NUMBERS_LAYOUT).with_fallback(' '),
            list: Terminus::REGULAR_12,
        }
    }
}

/// An event with its pointer position moved into a pane: divided by the
/// display scale, then offset.
pub fn local(e: &Event, scale: i32, dx: i32, dy: i32) -> Event {
    let s = scale.max(1);
    let f = |x: i32, off: i32| x.div_euclid(s) - off;
    match *e {
        Event::Button { window, button, pressed, x, y, root_x, root_y, modifiers, time } => {
            Event::Button { window, button, pressed, x: f(x, dx), y: f(y, dy), root_x, root_y, modifiers, time }
        }
        Event::Motion { window, x, y, root_x, root_y, buttons, modifiers } => {
            Event::Motion { window, x: f(x, dx), y: f(y, dy), root_x, root_y, buttons, modifiers }
        }
        Event::Scroll { window, dx: sx, dy: sy, x, y, modifiers } => Event::Scroll { window, dx: sx, dy: sy, x: f(x, dx), y: f(y, dy), modifiers },
        other => other,
    }
}

/// The pointer position of a pointer event.
pub fn pointer(e: &Event) -> Option<(i32, i32)> {
    match *e {
        Event::Button { x, y, .. } | Event::Motion { x, y, .. } | Event::Scroll { x, y, .. } => Some((x, y)),
        _ => None,
    }
}

/// Rectangles where `new` differs from `old` (same size): per band of 8
/// rows, the span of changed columns. What a frame needs to send.
pub fn damage(old: &Canvas, new: &Canvas) -> Vec<Rect> {
    damage_rows(old, new, 0, new.height())
}

/// [`damage`] within rows `y..y + height` only.
pub fn damage_rows(old: &Canvas, new: &Canvas, y: u32, height: u32) -> Vec<Rect> {
    if old.width() != new.width() || old.height() != new.height() {
        return vec![new.bounds()];
    }
    let (w, h) = (new.width() as usize, (y + height).min(new.height()) as usize);
    let (a, b) = (old.pixels(), new.pixels());
    let mut out = Vec::new();
    for band in (y as usize..h).step_by(8) {
        let rows = band..(band + 8).min(h);
        let (mut x0, mut x1) = (w, 0);
        for y in rows.clone() {
            let (ra, rb) = (&a[y * w..(y + 1) * w], &b[y * w..(y + 1) * w]);
            if ra == rb {
                continue;
            }
            if let Some(first) = ra.iter().zip(rb).position(|(p, q)| p != q) {
                let last = w - 1 - ra.iter().rev().zip(rb.iter().rev()).position(|(p, q)| p != q).expect("a difference");
                x0 = x0.min(first);
                x1 = x1.max(last + 1);
            }
        }
        if x1 > x0 {
            out.push(Rect::new(x0 as i32, band as i32, (x1 - x0) as u32, rows.len() as u32));
        }
    }
    out
}

/// Draw `text` with the skin's 5x6 font, clipped to `area`.
pub fn skin_text(c: &mut Canvas, font: &SheetFont, text: &str, x: i32, y: i32, area: Rect) {
    use protolith::font::Font;
    let saved = c.clip();
    c.set_clip(saved.intersect(&area));
    font.draw(c, &crate::skin::layout::text_for_sheet(text), x, y, 0);
    c.set_clip(saved);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use protolith::x11::{Key, Modifiers, MouseButton, MouseButtons, WindowId};

    pub(crate) const WIN: WindowId = WindowId::from_xid(0x200001);

    pub(crate) fn press(x: i32, y: i32) -> Event {
        Event::Button {
            window: WIN,
            button: MouseButton::Left,
            pressed: true,
            x,
            y,
            root_x: x + 100,
            root_y: y + 100,
            modifiers: Modifiers::default(),
            time: 1000,
        }
    }
    pub(crate) fn press_at(x: i32, y: i32, time: u32) -> Event {
        Event::Button { window: WIN, button: MouseButton::Left, pressed: true, x, y, root_x: x + 100, root_y: y + 100, modifiers: Modifiers::default(), time }
    }
    pub(crate) fn release(x: i32, y: i32) -> Event {
        Event::Button {
            window: WIN,
            button: MouseButton::Left,
            pressed: false,
            x,
            y,
            root_x: x + 100,
            root_y: y + 100,
            modifiers: Modifiers::default(),
            time: 1001,
        }
    }
    pub(crate) fn right(x: i32, y: i32) -> Event {
        Event::Button {
            window: WIN,
            button: MouseButton::Right,
            pressed: true,
            x,
            y,
            root_x: x + 100,
            root_y: y + 100,
            modifiers: Modifiers::default(),
            time: 1000,
        }
    }
    pub(crate) fn drag(x: i32, y: i32) -> Event {
        Event::Motion {
            window: WIN,
            x,
            y,
            root_x: x + 100,
            root_y: y + 100,
            buttons: MouseButtons { left: true, ..MouseButtons::default() },
            modifiers: Modifiers::default(),
        }
    }
    pub(crate) fn key(k: Key) -> Event {
        Event::Key { window: WIN, key: k, keysym: 0, keycode: 0, pressed: true, repeat: false, modifiers: Modifiers::default() }
    }
    pub(crate) fn click(x: i32, y: i32) -> [Event; 2] {
        [press(x, y), release(x, y)]
    }

    #[test]
    fn local_divides_then_offsets() {
        let e = local(&press(41, 300), 2, 0, 116);
        assert!(matches!(e, Event::Button { x: 20, y: 34, root_x: 141, .. }));
        let e = local(&press(-1, 0), 2, 0, 0);
        assert!(matches!(e, Event::Button { x: -1, .. }), "floor, not toward zero");
        assert_eq!(pointer(&key(Key::Enter)), None);
    }

    #[test]
    fn damage_finds_changed_spans() {
        let a = Canvas::new(40, 20);
        let mut b = a.clone();
        assert!(damage(&a, &b).is_empty());
        b.put(5, 3, 1);
        b.put(30, 6, 1);
        b.put(10, 17, 1);
        assert_eq!(damage(&a, &b), [Rect::new(5, 0, 26, 8), Rect::new(10, 16, 1, 4)]);
        assert_eq!(damage(&a, &Canvas::new(10, 10)), [Rect::new(0, 0, 10, 10)]);
        assert_eq!(damage_rows(&a, &b, 16, 4), [Rect::new(10, 16, 1, 4)], "only the rows asked about");
    }
}
