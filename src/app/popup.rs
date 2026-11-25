//! Popup menus: borderless override-redirect windows holding a
//! [`List`], in the skin's playlist colors.

use protolith::canvas::Canvas;
use protolith::font::{Font, Terminus};
use protolith::ui::{List, Style};
use protolith::x11::{Event, MouseButton, WindowId};
use protolith::Rect;

use super::common::Action;
use crate::skin::PlaylistColors;

/// What a menu item does.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    /// A window action.
    Act(Action),
    /// Apply equalizer preset `i`.
    Preset(usize),
    /// Add a URL (asks for it).
    AddUrl,
    /// Remove the selected entry.
    RemoveSelected,
    /// Keep only the selected entry.
    Crop,
    /// Empty the playlist.
    Clear,
    /// Select the playing entry.
    SelectCurrent,
    /// Select nothing.
    SelectNone,
    /// Sort by title.
    SortTitle,
    /// Sort by path.
    SortPath,
    /// Reverse.
    Reverse,
    /// Shuffle the list itself.
    Randomize,
    /// Save the list (asks where).
    SaveList,
    /// Load a list (asks which).
    LoadList,
    /// Empty the list (same as clear, as LIST > NEW).
    NewList,
    /// Pick a skin (asks which).
    LoadSkin,
    /// Back to the built-in skin.
    BuiltinSkin,
}

/// Rows are this tall.
pub const ROW: u32 = 14;

/// The look of menus and dialogs, from the skin's playlist colors.
pub fn style(c: &PlaylistColors) -> Style {
    let mix = |a: u32, b: u32, t: u32| {
        let ch = |s: u32| ((((a >> s) & 255) * (255 - t) + ((b >> s) & 255) * t) / 255) << s;
        ch(16) | ch(8) | ch(0)
    };
    let face = mix(c.normal_bg, c.normal, 50);
    Style {
        face,
        light: mix(face, 0xffffff, 90),
        shadow: mix(c.normal_bg, 0x000000, 100),
        text: c.normal,
        background: c.normal_bg,
        accent: c.selected_bg,
        accent_text: c.current,
    }
}

/// An open menu.
pub struct Menu {
    /// Its window.
    pub win: WindowId,
    /// The rows.
    pub list: List,
    /// What each row does.
    pub cmds: Vec<Cmd>,
    /// Its picture.
    pub canvas: Canvas,
}

impl Menu {
    /// The size a menu of these labels needs.
    pub fn size(labels: &[String], font: &Terminus) -> (u32, u32) {
        let w = labels.iter().map(|l| font.width(l)).max().unwrap_or(0) + 16;
        (w.max(80), ROW * labels.len() as u32 + 4)
    }

    /// A menu for window `win`.
    pub fn new(win: WindowId, items: Vec<(String, Cmd)>, font: &Terminus) -> Menu {
        let (labels, cmds): (Vec<String>, Vec<Cmd>) = items.into_iter().unzip();
        let (w, h) = Menu::size(&labels, font);
        let mut list = List::new(Rect::new(2, 2, w - 4, h - 4), ROW);
        list.items = labels;
        Menu { win, list, cmds, canvas: Canvas::new(w, h) }
    }

    /// Handle an event for the menu's window: hovering highlights, a
    /// click picks (returning the command).
    pub fn handle(&mut self, e: &Event) -> Option<Cmd> {
        match *e {
            Event::Motion { x, y, .. } => {
                let row = self.list.row_at(x, y);
                if row.is_some() {
                    self.list.select(row);
                }
                None
            }
            Event::Button { button: MouseButton::Left, pressed: true, x, y, .. } => self.list.row_at(x, y).and_then(|r| self.cmds.get(r).cloned()),
            _ => None,
        }
    }

    /// Redraw the picture.
    pub fn draw(&mut self, colors: &PlaylistColors, font: &Terminus) {
        let st = style(colors);
        let b = self.canvas.bounds();
        self.canvas.fill(st.background);
        self.list.draw(&mut self.canvas, font, &Style { text: st.text, ..st });
        // A border in the current-track color.
        let c = colors.current;
        self.canvas.fill_rect(Rect::new(0, 0, b.width, 1), c);
        self.canvas.fill_rect(Rect::new(0, b.height as i32 - 1, b.width, 1), c);
        self.canvas.fill_rect(Rect::new(0, 0, 1, b.height), c);
        self.canvas.fill_rect(Rect::new(b.width as i32 - 1, 0, 1, b.height), c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::common::tests::{drag, press};

    #[test]
    fn hover_highlights_and_click_picks() {
        let font = Terminus::REGULAR_12;
        let win = WindowId::from_xid(0x200001);
        let mut m = Menu::new(win, vec![("One".into(), Cmd::Clear), ("Two".into(), Cmd::Reverse)], &font);
        assert_eq!(m.canvas.height(), 2 * ROW + 4);
        assert_eq!(m.handle(&drag(10, 2 + ROW as i32 + 3)), None);
        assert_eq!(m.list.selected(), Some(1));
        assert_eq!(m.handle(&press(10, 5)), Some(Cmd::Clear));
        assert_eq!(m.handle(&press(10, 200)), None);
        let colors = PlaylistColors { normal: 0x00ff00, current: 0xffffff, normal_bg: 0, selected_bg: 0x0000c6 };
        m.draw(&colors, &font);
        assert_eq!(m.canvas.get(0, 0), Some(0xffffff), "border");
        assert_eq!(m.canvas.get(3, 2 + ROW + 1), Some(0x0000c6), "the highlighted row");
    }
}
