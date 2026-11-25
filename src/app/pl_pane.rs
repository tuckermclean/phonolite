//! The playlist window: the track list in the skin's colors, its scroll
//! bar, the menu buttons, mini transport, and the resize grip.

use protolith::canvas::Canvas;
use protolith::font::Font;
use protolith::ui::{Button, List, Orientation, Response, Slider};
use protolith::x11::{Event, Key, MouseButton};
use protolith::Rect;

use super::common::{skin_text, Action, Fonts, MenuKind, View};
use crate::player::Status;
use crate::playlist::{format_time, Entry};
use crate::skin::layout::{pl, WIDTH};
use crate::skin::Skin;

/// Row height of the track list.
pub const ROW: u32 = 13;
const W: i32 = WIDTH as i32;

/// Snap a height to the window's steps.
pub fn snap_height(h: i64) -> u32 {
    let min = i64::from(pl::MIN_HEIGHT);
    let step = i64::from(pl::HEIGHT_STEP);
    let steps = ((h - min) as f64 / step as f64).round().clamp(0.0, 40.0) as i64;
    (min + steps * step) as u32
}

/// The playlist window's widgets.
pub struct PlaylistPane {
    height: u32,
    /// The rows (their text is what keyboard and clicks act on).
    pub list: List,
    scroll: Slider,
    close: Button,
    menus: [Button; 5],
    mini: [Button; 6],
    resize: Option<(i32, u32)>,
}

impl Default for PlaylistPane {
    fn default() -> PlaylistPane {
        PlaylistPane::new(pl::DEFAULT_HEIGHT)
    }
}

impl PlaylistPane {
    /// A pane of the given height.
    pub fn new(height: u32) -> PlaylistPane {
        let mut p = PlaylistPane {
            height: 0,
            list: List::new(Rect::new(0, 0, 1, 1), ROW),
            scroll: Slider::new(Rect::new(0, 0, 1, 1), Orientation::Vertical, 0).with_thumb(18).with_step(1),
            close: Button::new(Rect::new(W + pl::CLOSE_AT.0, pl::CLOSE_AT.1, 9, 9)),
            menus: std::array::from_fn(|_| Button::new(Rect::new(0, 0, 1, 1))),
            mini: std::array::from_fn(|_| Button::new(Rect::new(0, 0, 1, 1))),
            resize: None,
        };
        p.set_height(height);
        p
    }

    /// Height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Lay out for a new height.
    pub fn set_height(&mut self, h: u32) {
        let h = snap_height(i64::from(h));
        self.height = h;
        let (l, t, rgt, b) = pl::LIST_INSET;
        self.list.set_rect(Rect::new(l, t, (W - l - rgt) as u32, h - (t + b) as u32));
        self.scroll.set_rect(Rect::new(W + pl::SCROLL_X, t, 8, h - (t + b) as u32));
        let y = h as i32 + pl::MENU_Y;
        for (k, b) in self.menus.iter_mut().enumerate() {
            let (x, w) = if k < 4 { pl::MENU_BUTTONS[k] } else { (W + pl::LIST_BUTTON.0, pl::LIST_BUTTON.1) };
            b.set_rect(Rect::new(x, y, w, 18));
        }
        let br = W - 150;
        for (b, (x, w)) in self.mini.iter_mut().zip(pl::MINI_BUTTONS) {
            b.set_rect(Rect::new(br + x, h as i32 - 38 + pl::MINI_Y.0, w, pl::MINI_Y.1));
        }
        self.sync_scroll();
    }

    /// Replace the rows (keeps the selection and scroll where possible).
    pub fn set_rows(&mut self, rows: Vec<String>) {
        let sel = self.list.selected();
        let top = self.list.top();
        self.list.items = rows;
        self.list.select(sel.filter(|&s| s < self.list.items.len()));
        self.list.set_top(top);
        self.sync_scroll();
    }

    /// Scroll so row `i` is visible.
    pub fn show_row(&mut self, i: usize) {
        let vis = self.list.visible_rows().max(1);
        if i < self.list.top() {
            self.list.set_top(i);
        } else if i >= self.list.top() + vis {
            self.list.set_top(i + 1 - vis);
        }
        self.sync_scroll();
    }

    fn sync_scroll(&mut self) {
        let max = self.list.max_top() as u32;
        self.scroll.set_max(max);
        self.scroll.set_enabled(max > 0);
        if !self.scroll.is_dragging() {
            self.scroll.set_value(max - self.list.top() as u32);
        }
    }

    /// Handle an event (pane coordinates).
    pub fn handle(&mut self, e: &Event) -> Vec<Action> {
        let mut out = Vec::new();
        let root = match *e {
            Event::Button { root_x, root_y, .. } => (root_x, root_y),
            _ => (0, 0),
        };
        // Resizing by the grip.
        match *e {
            Event::Button { button: MouseButton::Left, pressed: true, x, y, root_y, .. } => {
                let grip = Rect::new(W - 150 + pl::GRIP.x, self.height as i32 - 38 + pl::GRIP.y, pl::GRIP.width, pl::GRIP.height);
                if grip.contains(x, y) {
                    self.resize = Some((root_y, self.height));
                    return out;
                }
            }
            Event::Motion { root_y, .. } if self.resize.is_some() => {
                let (y0, h0) = self.resize.expect("resizing");
                let h = snap_height(i64::from(h0) + i64::from(root_y - y0));
                if h != self.height {
                    out.push(Action::ResizePlaylist(h));
                }
                return out;
            }
            Event::Button { button: MouseButton::Left, pressed: false, .. } if self.resize.is_some() => {
                self.resize = None;
                return out;
            }
            _ => {}
        }
        if let Event::Button { button: MouseButton::Right, pressed: true, root_x, root_y, x, y, .. } = *e {
            if self.list.rect().contains(x, y) {
                if let Some(row) = self.list.row_at(x, y) {
                    self.list.select(Some(row));
                }
                return vec![Action::Menu(MenuKind::Remove, root_x, root_y)];
            }
        }
        match self.list.handle(e) {
            Response::Submit => {
                if let Some(i) = self.list.selected() {
                    out.push(Action::PlayIndex(i));
                }
                self.sync_scroll();
            }
            Response::Changed | Response::Redraw => {
                if let Some(i) = self.list.selected() {
                    if matches!(e, Event::Key { .. }) {
                        self.show_row(i);
                    }
                }
                self.sync_scroll();
            }
            _ => {}
        }
        if let Event::Key { key: Key::Delete, pressed: true, .. } = *e {
            if let (true, Some(i)) = (self.list.is_focused(), self.list.selected()) {
                out.push(Action::Remove(i));
            }
        }
        if let Response::Changed | Response::Submit = self.scroll.handle(e) {
            let max = self.list.max_top();
            self.list.set_top(max.saturating_sub(self.scroll.value() as usize));
        }
        if self.close.handle(e) == Response::Clicked {
            out.push(Action::ShowPlaylist(false));
        }
        const MENUS: [MenuKind; 5] = [MenuKind::Add, MenuKind::Remove, MenuKind::Select, MenuKind::Misc, MenuKind::List];
        for (b, k) in self.menus.iter_mut().zip(MENUS) {
            if b.handle(e) == Response::Clicked {
                out.push(Action::Menu(k, root.0, root.1));
            }
        }
        const MINI: [Action; 6] = [Action::Previous, Action::Play, Action::Pause, Action::Stop, Action::Next, Action::Open];
        for (b, a) in self.mini.iter_mut().zip(MINI) {
            if b.handle(e) == Response::Clicked {
                out.push(a);
            }
        }
        if let Event::Button { button: MouseButton::Left, pressed: true, x, y, root_x, root_y, .. } = *e {
            if y < pl::LIST_INSET.1 && !self.close.rect().contains(x, y) {
                out.push(Action::DragWindow(root_x, root_y));
            }
        }
        out
    }

    /// Draw the pane at the canvas origin.
    pub fn draw(&self, c: &mut Canvas, skin: &Skin, fonts: &Fonts, entries: &[Entry], current: Option<usize>, v: &View) {
        let sheet = skin.pledit.frame();
        let h = self.height as i32;
        let top = if v.focused { pl::TOP_ACTIVE } else { pl::TOP_INACTIVE };
        for x in (25..W - 25).step_by(25) {
            c.blit(&sheet, top[2], x, 0);
        }
        c.blit(&sheet, top[1], (W - 100) / 2, 0);
        c.blit(&sheet, top[0], 0, 0);
        c.blit(&sheet, top[3], W - 25, 0);
        let saved = c.clip();
        c.set_clip(saved.intersect(&Rect::new(0, 20, WIDTH, (h - 58) as u32)));
        for y in (20..h - 38).step_by(29) {
            c.blit(&sheet, pl::LEFT, 0, y);
            c.blit(&sheet, pl::RIGHT, W - 20, y);
        }
        c.set_clip(saved);
        for x in (125..W - 150).step_by(25) {
            c.blit(&sheet, pl::BOTTOM, x, h - 38);
        }
        c.blit(&sheet, pl::BOTTOM_LEFT, 0, h - 38);
        c.blit(&sheet, pl::BOTTOM_RIGHT, W - 150, h - 38);
        if self.close.is_down() {
            c.blit(&sheet, pl::CLOSE_DOWN, W + pl::CLOSE_AT.0, pl::CLOSE_AT.1);
        }

        // Rows.
        let colors = skin.playlist;
        let area = self.list.rect();
        c.fill_rect(area, colors.normal_bg);
        let saved = c.clip();
        c.set_clip(saved.intersect(&area));
        let rows = (area.height as usize).div_ceil(ROW as usize);
        let cw = fonts.list.advance('0') as i32;
        for (n, e) in entries.iter().enumerate().skip(self.list.top()).take(rows) {
            let y = area.y + (n - self.list.top()) as i32 * ROW as i32;
            if Some(n) == self.list.selected() {
                c.fill_rect(Rect::new(area.x, y, area.width, ROW), colors.selected_bg);
            }
            let color = if Some(n) == current { colors.current } else { colors.normal };
            let dur = e.duration.map(format_time).unwrap_or_default();
            let dur_w = cw * dur.chars().count() as i32;
            let room = ((area.width as i32 - 4 - dur_w - cw) / cw).max(0) as usize;
            let mut text = format!("{}. {}", n + 1, e.display());
            if text.chars().count() > room {
                text = text.chars().take(room.saturating_sub(1)).collect::<String>() + "~";
            }
            fonts.list.draw(c, &text, area.x + 2, y, color);
            fonts.list.draw(c, &dur, area.x + area.width as i32 - 2 - dur_w, y, color);
        }
        c.set_clip(saved);
        if self.scroll.is_enabled() {
            let t = self.scroll.thumb_rect();
            c.blit(&sheet, if self.scroll.is_dragging() { pl::SCROLL_THUMB_DOWN } else { pl::SCROLL_THUMB }, t.x, t.y);
        } else {
            c.blit(&sheet, pl::SCROLL_THUMB, W + pl::SCROLL_X, pl::LIST_INSET.1);
        }

        // Running time: selected / total, and the mini clock.
        let br = (W - 150, h - 38);
        let (total, unknown) = {
            let known: f64 = entries.iter().filter_map(|e| e.duration).sum();
            (known, entries.iter().any(|e| e.duration.is_none()))
        };
        let sel = self.list.selected().and_then(|i| entries.get(i)).and_then(|e| e.duration).unwrap_or(0.0);
        let running = format!("{}/{}{}", format_time(sel), format_time(total), if unknown { "+" } else { "" });
        let (rx, ry) = (br.0 + pl::RUNNING_TIME.0, br.1 + pl::RUNNING_TIME.1);
        skin_text(c, &fonts.text, &running, rx, ry, Rect::new(rx, ry, 88, 6));
        if v.status != Status::Stopped {
            let t = match (v.remaining, v.duration) {
                (true, Some(d)) => format!("-{}", format_time((d - v.position).max(0.0))),
                _ => format_time(v.position),
            };
            let (mx, my) = (br.0 + pl::MINI_TIME.0, br.1 + pl::MINI_TIME.1);
            skin_text(c, &fonts.text, &format!("{t:>6}"), mx, my, Rect::new(mx, my, 30, 6));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::common::tests::{click, drag, key, press, press_at, release};
    use crate::skin::builtin;

    fn rows(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("track {i}")).collect()
    }

    #[test]
    fn heights_snap() {
        assert_eq!(snap_height(0), 116);
        assert_eq!(snap_height(232), 232);
        assert_eq!(snap_height(250), 261);
        assert_eq!(snap_height(240), 232);
        assert_eq!(PlaylistPane::new(250).height(), 261);
    }

    #[test]
    fn double_click_plays_and_delete_removes() {
        let mut p = PlaylistPane::new(232);
        p.set_rows(rows(30));
        let y = 20 + 2 * ROW as i32 + 3;
        assert!(p.handle(&press_at(40, y, 100)).is_empty());
        p.handle(&release(40, y));
        assert_eq!(p.handle(&press_at(40, y, 300)), [Action::PlayIndex(2)]);
        p.handle(&release(40, y));
        assert_eq!(p.handle(&key(Key::Delete)), [Action::Remove(2)]);
        p.handle(&key(Key::End));
        assert_eq!(p.list.selected(), Some(29));
        assert!(p.list.top() > 0, "scrolled to show it");
    }

    #[test]
    fn scroll_bar_moves_the_list() {
        let mut p = PlaylistPane::new(232);
        p.set_rows(rows(100));
        let max = p.list.max_top();
        assert!(max > 50);
        let x = W + pl::SCROLL_X + 4;
        p.handle(&press(x, 22));
        p.handle(&drag(x, 232 - 40));
        p.handle(&release(x, 232 - 40));
        assert_eq!(p.list.top(), max, "dragged to the bottom");
    }

    #[test]
    fn grip_resizes_in_steps() {
        let mut p = PlaylistPane::new(232);
        let (gx, gy) = (W - 150 + pl::GRIP.x + 10, 232 - 38 + pl::GRIP.y + 10);
        assert!(p.handle(&press(gx, gy)).is_empty());
        assert_eq!(p.handle(&drag(gx, gy + 31)), [Action::ResizePlaylist(261)]);
        p.set_height(261);
        assert!(p.handle(&drag(gx, gy + 35)).is_empty(), "same step");
        assert!(p.handle(&release(gx, gy + 35)).is_empty());
    }

    #[test]
    fn buttons_and_title() {
        let mut p = PlaylistPane::new(232);
        let a: Vec<Action> = click(20, 232 - 25).iter().flat_map(|e| p.handle(e)).collect();
        assert!(matches!(a[..], [Action::Menu(MenuKind::Add, ..)]), "{a:?}");
        let a: Vec<Action> = click(W - 40, 232 - 25).iter().flat_map(|e| p.handle(e)).collect();
        assert!(matches!(a[..], [Action::Menu(MenuKind::List, ..)]), "{a:?}");
        let (x, _) = pl::MINI_BUTTONS[1];
        let a: Vec<Action> = click(W - 150 + x + 3, 232 - 38 + 24).iter().flat_map(|e| p.handle(e)).collect();
        assert_eq!(a, [Action::Play]);
        let a: Vec<Action> = click(W - 7, 7).iter().flat_map(|e| p.handle(e)).collect();
        assert_eq!(a, [Action::ShowPlaylist(false)]);
        assert!(matches!(p.handle(&press(100, 10))[..], [Action::DragWindow(..)]));
    }

    #[test]
    fn draws_rows_in_the_skin_colors() {
        let skin = builtin::skin();
        let fonts = Fonts::new(&skin);
        let mut p = PlaylistPane::new(232);
        let entries: Vec<Entry> = (0..5).map(|i| Entry { duration: Some(61.0 * f64::from(i)), ..Entry::new(format!("/m/song {i}.mp3")) }).collect();
        p.set_rows(entries.iter().map(Entry::display).collect());
        p.list.select(Some(1));
        let mut c = Canvas::new(275, 232);
        p.draw(&mut c, &skin, &fonts, &entries, Some(2), &View::default());
        let area = p.list.rect();
        let colors_in_row = |row: i32| -> Vec<u32> {
            let mut v: Vec<u32> = (0..ROW as i32)
                .flat_map(|dy| (0..area.width as i32).map(move |dx| (dx, dy)))
                .filter_map(|(dx, dy)| c.get((area.x + dx) as u32, (area.y + row * ROW as i32 + dy) as u32))
                .collect();
            v.sort();
            v.dedup();
            v
        };
        assert!(colors_in_row(1).contains(&skin.playlist.selected_bg), "selected row");
        assert!(colors_in_row(2).contains(&skin.playlist.current), "current track");
        assert!(colors_in_row(0).contains(&skin.playlist.normal) && colors_in_row(0).contains(&skin.playlist.normal_bg));
        assert_eq!(colors_in_row(7), [skin.playlist.normal_bg], "empty rows");
    }
}
