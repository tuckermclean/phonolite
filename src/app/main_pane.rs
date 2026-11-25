//! The main window: title bar, clock, visualizer, scrolling title,
//! sliders, transport.

use protolith::canvas::Canvas;
use protolith::font::Font;
use protolith::ui::{Button, Orientation, Response, Slider, Toggle};
use protolith::x11::{Event, MouseButton};

use super::common::{skin_text, Action, Fonts, MenuKind, View};
use crate::player::Status;
use crate::skin::layout::{main as m, r};
use crate::skin::Skin;
use crate::vis::{self, Mode, Spectrum};

/// Pixels the title scrolls per tick.
const SCROLL_STEP: i32 = 1;
/// Between repeats of a scrolling title.
const SEPARATOR: &str = "  ***  ";

/// The main window's widgets and animation state.
pub struct MainPane {
    menu: Button,
    minimize: Button,
    shade: Button,
    close: Button,
    transport: [Button; 6],
    shuffle: Toggle,
    repeat: Toggle,
    eq: Toggle,
    pl: Toggle,
    /// Volume, 0..=100.
    pub volume: Slider,
    /// Balance + 100, 0..=200.
    pub balance: Slider,
    /// Position in tenths of a second.
    pub seek: Slider,
    clutter_down: Option<usize>,
    marquee: i32,
    /// The analyzer's state.
    pub spectrum: Spectrum,
}

impl Default for MainPane {
    fn default() -> MainPane {
        MainPane::new()
    }
}

impl MainPane {
    /// Widgets at their classic places.
    pub fn new() -> MainPane {
        MainPane {
            menu: Button::new(m::MENU.0),
            minimize: Button::new(m::MINIMIZE.0),
            shade: Button::new(m::SHADE.0),
            close: Button::new(m::CLOSE.0),
            transport: m::TRANSPORT.map(|(rect, _)| Button::new(rect)),
            shuffle: Toggle::new(m::SHUFFLE.0, false),
            repeat: Toggle::new(m::REPEAT.0, false),
            eq: Toggle::new(m::EQ_TOGGLE.0, false),
            pl: Toggle::new(m::PL_TOGGLE.0, false),
            volume: Slider::new(m::VOLUME, Orientation::Horizontal, 100).with_thumb(14).with_step(5),
            balance: Slider::new(m::BALANCE, Orientation::Horizontal, 200).with_thumb(14).with_step(10),
            seek: Slider::new(m::POSBAR, Orientation::Horizontal, 1).with_thumb(29).with_step(50),
            clutter_down: None,
            marquee: 0,
            spectrum: Spectrum::new(),
        }
    }

    /// Bring widgets in line with the application's state (except what
    /// the user is dragging).
    pub fn sync(&mut self, v: &View) {
        if !self.shuffle.is_down() {
            self.shuffle.set_on(v.shuffle);
        }
        if !self.repeat.is_down() {
            self.repeat.set_on(v.repeat);
        }
        if !self.eq.is_down() {
            self.eq.set_on(v.show_eq);
        }
        if !self.pl.is_down() {
            self.pl.set_on(v.show_pl);
        }
        if !self.volume.is_dragging() {
            self.volume.set_value(v.volume);
        }
        if !self.balance.is_dragging() {
            self.balance.set_value((v.balance + 100).clamp(0, 200) as u32);
        }
        let seekable = v.status != Status::Stopped && v.duration.is_some_and(|d| d > 0.0);
        self.seek.set_enabled(seekable);
        if let Some(d) = v.duration {
            let max = (d * 10.0).round().max(1.0) as u32;
            if self.seek.max() != max {
                self.seek.set_max(max);
            }
        }
        if !self.seek.is_dragging() {
            self.seek.set_value((v.position * 10.0).round() as u32);
        }
    }

    /// Restart the title's scroll (a new track).
    pub fn reset_marquee(&mut self) {
        self.marquee = 0;
    }

    /// Advance animations by one tick; whether the title scrolls.
    pub fn tick(&mut self, v: &View) -> bool {
        let w = 5 * marquee_text(v, self).chars().count() as i32;
        if w <= m::MARQUEE.width as i32 {
            self.marquee = 0;
            return false;
        }
        let period = w + 5 * SEPARATOR.len() as i32;
        self.marquee = (self.marquee + SCROLL_STEP) % period;
        true
    }

    /// Handle an event (pane coordinates).
    pub fn handle(&mut self, e: &Event, v: &View) -> Vec<Action> {
        let mut out = Vec::new();
        if let Event::Button { button: MouseButton::Right, pressed: true, root_x, root_y, .. } = *e {
            return vec![Action::Menu(MenuKind::Main, root_x, root_y)];
        }
        let root = match *e {
            Event::Button { root_x, root_y, .. } => (root_x, root_y),
            _ => (0, 0),
        };
        if self.menu.handle(e) == Response::Clicked {
            out.push(Action::Menu(MenuKind::Main, root.0, root.1));
        }
        if self.minimize.handle(e) == Response::Clicked {
            out.push(Action::Minimize);
        }
        if self.shade.handle(e) == Response::Clicked {
            let show = !(v.show_eq || v.show_pl);
            out.extend([Action::ShowEq(show), Action::ShowPlaylist(show)]);
        }
        if self.close.handle(e) == Response::Clicked {
            out.push(Action::Quit);
        }
        const TRANSPORT: [Action; 6] = [Action::Previous, Action::Play, Action::Pause, Action::Stop, Action::Next, Action::Open];
        for (b, a) in self.transport.iter_mut().zip(TRANSPORT) {
            if b.handle(e) == Response::Clicked {
                out.push(a);
            }
        }
        if self.shuffle.handle(e) == Response::Changed {
            out.push(Action::Shuffle(self.shuffle.is_on()));
        }
        if self.repeat.handle(e) == Response::Changed {
            out.push(Action::Repeat(self.repeat.is_on()));
        }
        if self.eq.handle(e) == Response::Changed {
            out.push(Action::ShowEq(self.eq.is_on()));
        }
        if self.pl.handle(e) == Response::Changed {
            out.push(Action::ShowPlaylist(self.pl.is_on()));
        }
        if let Response::Changed | Response::Submit = self.volume.handle(e) {
            out.push(Action::Volume(self.volume.value()));
        }
        if let Response::Changed | Response::Submit = self.balance.handle(e) {
            // Snap to the middle near it, as the classic one does.
            let b = self.balance.value() as i32 - 100;
            let b = if b.abs() < 12 { 0 } else { b };
            out.push(Action::Balance(b));
        }
        if self.seek.handle(e) == Response::Submit {
            out.push(Action::Seek(f64::from(self.seek.value()) / 10.0));
        }

        match *e {
            Event::Button { button: MouseButton::Left, pressed: true, x, y, root_x, root_y, .. } => {
                if m::CLUTTER.contains(x, y) {
                    self.clutter_down = m::CLUTTER_BUTTONS.iter().position(|&(dy, h)| (dy..dy + h as i32).contains(&(y - m::CLUTTER.y)));
                } else if m::VIS.contains(x, y) {
                    out.push(Action::NextVisMode);
                } else if m::TIME_AREA.contains(x, y) {
                    out.push(Action::TimeMode);
                } else if !self.on_control(x, y) {
                    out.push(Action::DragWindow(root_x, root_y));
                }
            }
            Event::Button { button: MouseButton::Left, pressed: false, x, y, root_x, root_y, .. } => {
                if let Some(k) = self.clutter_down.take() {
                    let (dy, h) = m::CLUTTER_BUTTONS[k];
                    if m::CLUTTER.contains(x, y) && (dy..dy + h as i32).contains(&(y - m::CLUTTER.y)) {
                        out.push(match k {
                            0 => Action::Menu(MenuKind::Main, root_x, root_y),
                            1 => Action::AlwaysOnTop,
                            2 => Action::Jump,
                            3 => Action::DoubleSize,
                            _ => Action::NextVisMode,
                        });
                    }
                }
            }
            Event::Scroll { dy, x, y, .. } if !m::VOLUME.contains(x, y) && !m::BALANCE.contains(x, y) && !m::POSBAR.contains(x, y) => {
                // The wheel anywhere else is volume, as in most players.
                out.push(Action::Volume((v.volume as i32 - dy * 5).clamp(0, 100) as u32));
            }
            _ => {}
        }
        out
    }

    fn on_control(&self, x: i32, y: i32) -> bool {
        let rects =
            [m::MENU.0, m::MINIMIZE.0, m::SHADE.0, m::CLOSE.0, m::SHUFFLE.0, m::REPEAT.0, m::EQ_TOGGLE.0, m::PL_TOGGLE.0, m::VOLUME, m::BALANCE, m::POSBAR];
        rects.iter().chain(m::TRANSPORT.iter().map(|(r, _)| r)).any(|r| r.contains(x, y))
    }

    /// Redraw only the visualizer (its area of main.bmp, then the bars or
    /// the wave): all that changes most frames.
    pub fn draw_vis(&self, c: &mut Canvas, skin: &Skin, v: &View, samples: Option<&[i16]>) {
        c.blit(&skin.main.frame(), m::VIS, m::VIS.x, m::VIS.y);
        if v.status == Status::Playing {
            match v.vis {
                Mode::Spectrum => vis::draw_spectrum(c, m::VIS, &skin.viscolor, &self.spectrum),
                Mode::Scope => vis::draw_scope(c, m::VIS, &skin.viscolor, samples.unwrap_or(&[])),
                Mode::Off => {}
            }
        }
    }

    /// Redraw only the scrolling title. While seeking it shows the target.
    pub fn draw_marquee(&self, c: &mut Canvas, skin: &Skin, fonts: &Fonts, v: &View) {
        c.blit(&skin.main.frame(), m::MARQUEE, m::MARQUEE.x, m::MARQUEE.y);
        let text = marquee_text(v, self);
        let w = 5 * text.chars().count() as i32;
        let (mx, my) = (m::MARQUEE.x, m::MARQUEE.y);
        if w <= m::MARQUEE.width as i32 {
            skin_text(c, &fonts.text, &text, mx, my, m::MARQUEE);
        } else {
            let lap = format!("{text}{SEPARATOR}");
            let period = 5 * lap.chars().count() as i32;
            let x0 = mx - self.marquee.rem_euclid(period);
            skin_text(c, &fonts.text, &lap, x0, my, m::MARQUEE);
            skin_text(c, &fonts.text, &lap, x0 + period, my, m::MARQUEE);
        }
    }

    /// A summary of everything the pane shows except the visualizer and
    /// the title's scroll: when it is unchanged, redrawing those two is
    /// enough.
    pub fn look(&self, v: &View) -> String {
        let blink = v.status == Status::Paused && v.blink;
        format!(
            "{:?}{}{:?}{:?}{:?}{:?}{}{}{}{}{}{}{}{}{}{}{}{}{}{:?}{}",
            v.status,
            v.position.floor(),
            v.duration.map(f64::floor),
            v.kbps,
            v.khz,
            v.channels,
            v.focused,
            v.remaining,
            blink,
            v.on_top,
            v.double,
            v.shuffle,
            v.repeat,
            v.show_eq,
            v.show_pl,
            self.volume.value(),
            self.balance.value(),
            self.seek.thumb_rect().x,
            self.seek.is_enabled(),
            self.clutter_down,
            marquee_text(v, self).len(),
        )
    }

    /// Draw the pane at the canvas origin.
    pub fn draw(&self, c: &mut Canvas, skin: &Skin, fonts: &Fonts, v: &View, samples: Option<&[i16]>) {
        c.blit(&skin.main.frame(), skin.main.bounds(), 0, 0);
        let title = if v.focused { m::TITLE_ACTIVE } else { m::TITLE_INACTIVE };
        c.blit(&skin.titlebar.frame(), title, 0, 0);
        let tb = skin.titlebar.frame();
        self.menu.draw_sprite(c, &tb, &m::MENU.1);
        self.minimize.draw_sprite(c, &tb, &m::MINIMIZE.1);
        self.shade.draw_sprite(c, &tb, &m::SHADE.1);
        self.close.draw_sprite(c, &tb, &m::CLOSE.1);
        for (k, (dy, h)) in m::CLUTTER_BUTTONS.iter().enumerate() {
            let lit = self.clutter_down == Some(k) || (k == 1 && v.on_top) || (k == 3 && v.double);
            if lit {
                c.blit(&tb, r(m::CLUTTER_DOWN_X + 8 * k as i32, m::CLUTTER_DOWN_Y + dy, 8, *h), m::CLUTTER.x, m::CLUTTER.y + dy);
            }
        }

        // Play state.
        let pp = skin.playpaus.frame();
        let (state, x, y) = (
            match v.status {
                Status::Playing => m::PLAYING,
                Status::Paused => m::PAUSED,
                Status::Stopped => m::STOPPED,
            },
            m::PLAY_STATE.x,
            m::PLAY_STATE.y,
        );
        c.blit(&pp, state, x, y);
        if v.status == Status::Playing {
            c.blit(&pp, m::WORKING, m::WORK_INDICATOR.x, m::WORK_INDICATOR.y);
        }

        // Clock.
        if v.status != Status::Stopped && (v.status == Status::Playing || v.blink) {
            let shown = match (v.remaining, v.duration) {
                (true, Some(d)) => (d - v.position).max(0.0),
                _ => v.position,
            };
            let secs = shown.floor() as u64;
            let mins = (secs / 60).min(99);
            let digits = [mins / 10, mins % 10, (secs % 60) / 10, secs % 10];
            for (d, (dx, dy)) in digits.iter().zip(m::DIGITS) {
                fonts.numbers.draw_char(c, char::from(b'0' + *d as u8), dx, dy, 0);
            }
            if v.remaining && v.duration.is_some() {
                if skin.numbers_ex {
                    fonts.numbers.draw_char(c, '-', m::MINUS_CELL.0, m::MINUS_CELL.1, 0);
                } else {
                    c.blit(&skin.numbers.frame(), m::MINUS_BAR, m::MINUS_BAR_AT.0, m::MINUS_BAR_AT.1);
                }
            }
        }

        self.draw_vis(c, skin, v, samples);
        self.draw_marquee(c, skin, fonts, v);
        if let Some(k) = v.kbps {
            let s = format!("{:>3}", k.min(999));
            skin_text(c, &fonts.text, &s, m::KBPS.x, m::KBPS.y, m::KBPS);
        }
        if let Some(k) = v.khz {
            let s = format!("{:>2}", k.min(99));
            skin_text(c, &fonts.text, &s, m::KHZ.x, m::KHZ.y, m::KHZ);
        }
        let ms = skin.monoster.frame();
        let (stereo, mono) = (v.channels == Some(2), v.channels == Some(1));
        c.blit(&ms, if stereo { m::STEREO.1 } else { m::STEREO.2 }, m::STEREO.0.x, m::STEREO.0.y);
        c.blit(&ms, if mono { m::MONO.1 } else { m::MONO.2 }, m::MONO.0.x, m::MONO.0.y);

        self.volume.draw_sprite(c, &skin.volume.frame(), &m::VOLUME_SPRITES);
        self.balance.draw_sprite(c, &skin.balance.frame(), &m::BALANCE_SPRITES);
        if self.seek.is_enabled() {
            self.seek.draw_sprite(c, &skin.posbar.frame(), &m::POSBAR_SPRITES);
        } else {
            c.blit(&skin.posbar.frame(), m::POSBAR_SPRITES.track.first, m::POSBAR.x, m::POSBAR.y);
        }
        let sr = skin.shufrep.frame();
        self.eq.draw_sprite(c, &sr, &m::EQ_TOGGLE.1);
        self.pl.draw_sprite(c, &sr, &m::PL_TOGGLE.1);
        self.shuffle.draw_sprite(c, &sr, &m::SHUFFLE.1);
        self.repeat.draw_sprite(c, &sr, &m::REPEAT.1);
        let cb = skin.cbuttons.frame();
        for (b, (_, s)) in self.transport.iter().zip(m::TRANSPORT.iter()) {
            b.draw_sprite(c, &cb, s);
        }
    }
}

fn marquee_text(v: &View, pane: &MainPane) -> String {
    if pane.seek.is_dragging() {
        let to = f64::from(pane.seek.value()) / 10.0;
        let d = v.duration.unwrap_or(0.0);
        return format!(
            "SEEK TO: {}/{} ({}%)",
            crate::playlist::format_time(to),
            crate::playlist::format_time(d),
            if d > 0.0 { (to / d * 100.0).round() as u32 } else { 0 }
        );
    }
    if pane.volume.is_dragging() {
        return format!("VOLUME: {}%", pane.volume.value());
    }
    if pane.balance.is_dragging() {
        let b = pane.balance.value() as i32 - 100;
        return match b {
            0 => "BALANCE: CENTER".into(),
            b if b < 0 => format!("BALANCE: {}% LEFT", -b),
            b => format!("BALANCE: {b}% RIGHT"),
        };
    }
    v.marquee.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::common::tests::{click, drag, press, release, right};
    use crate::skin::builtin;

    fn handle_all(p: &mut MainPane, v: &View, events: &[Event]) -> Vec<Action> {
        events.iter().flat_map(|e| p.handle(e, v)).collect()
    }

    #[test]
    fn transport_buttons_fire_their_actions() {
        let mut p = MainPane::new();
        let v = View::default();
        let expect = [Action::Previous, Action::Play, Action::Pause, Action::Stop, Action::Next, Action::Open];
        for ((rect, _), a) in m::TRANSPORT.iter().zip(expect) {
            let (x, y) = (rect.x + 5, rect.y + 5);
            assert_eq!(handle_all(&mut p, &v, &click(x, y)), [a]);
        }
        // Pressed on play, released off it: nothing.
        assert_eq!(handle_all(&mut p, &v, &[press(45, 95), release(200, 20)]), []);
    }

    #[test]
    fn toggles_sliders_and_areas() {
        let mut p = MainPane::new();
        let v = View::default();
        p.sync(&v);
        assert_eq!(handle_all(&mut p, &v, &click(170, 95)), [Action::Shuffle(true)]);
        assert_eq!(handle_all(&mut p, &v, &click(215, 95)), [Action::Repeat(true)]);
        assert_eq!(handle_all(&mut p, &v, &click(225, 62)), [Action::ShowEq(false)], "shown by default; a click hides");
        assert_eq!(handle_all(&mut p, &v, &click(30, 50)), [Action::NextVisMode]);
        assert_eq!(handle_all(&mut p, &v, &click(60, 30)), [Action::TimeMode]);
        // Volume: press at the far right end.
        let a = handle_all(&mut p, &v, &[press(174, 62), release(174, 62)]);
        assert_eq!(a.first(), Some(&Action::Volume(100)));
        // Balance near the middle snaps to it.
        let a = handle_all(&mut p, &v, &[press(m::BALANCE.x + 19, 62), release(m::BALANCE.x + 19, 62)]);
        assert!(a.iter().all(|a| *a == Action::Balance(0)), "{a:?}");
        assert_eq!(handle_all(&mut p, &v, &[right(5, 50)]), [Action::Menu(MenuKind::Main, 105, 150)]);
        assert_eq!(handle_all(&mut p, &v, &[press(150, 6)]), [Action::DragWindow(250, 106)], "the title bar drags");
        assert_eq!(handle_all(&mut p, &v, &click(268, 7)), [Action::Quit]);
        assert_eq!(handle_all(&mut p, &v, &click(248, 7)), [Action::Minimize]);
    }

    #[test]
    fn seek_fires_on_release_only() {
        let mut p = MainPane::new();
        let v = View { status: Status::Playing, duration: Some(100.0), position: 10.0, ..View::default() };
        p.sync(&v);
        assert!(handle_all(&mut p, &v, &[press(30, 77), drag(150, 77)]).is_empty(), "dragging does not seek");
        assert!(marquee_text(&v, &p).starts_with("SEEK TO: "));
        let a = handle_all(&mut p, &v, &[release(150, 77)]);
        let [Action::Seek(s)] = a[..] else { panic!("{a:?}") };
        assert!(s > 40.0 && s < 60.0, "{s}");
        // Stopped: the seek bar is dead.
        let stopped = View::default();
        p.sync(&stopped);
        assert!(handle_all(&mut p, &stopped, &click(150, 77)).iter().all(|a| !matches!(a, Action::Seek(_))));
    }

    #[test]
    fn clutter_bar() {
        let mut p = MainPane::new();
        let v = View::default();
        let y = |k: usize| m::CLUTTER.y + m::CLUTTER_BUTTONS[k].0 + 2;
        assert_eq!(handle_all(&mut p, &v, &click(13, y(1))), [Action::AlwaysOnTop]);
        assert_eq!(handle_all(&mut p, &v, &click(13, y(3))), [Action::DoubleSize]);
        assert_eq!(handle_all(&mut p, &v, &click(13, y(4))), [Action::NextVisMode]);
        assert_eq!(handle_all(&mut p, &v, &[press(13, y(1)), release(13, y(3))]), [], "released on another button");
    }

    #[test]
    fn draws_state_from_the_skin() {
        let skin = builtin::skin();
        let fonts = Fonts::new(&skin);
        let mut c = Canvas::new(275, 116);
        let mut p = MainPane::new();
        let v = View {
            status: Status::Playing,
            position: 754.0,
            duration: Some(800.0),
            marquee: "HELLO".into(),
            kbps: Some(128),
            khz: Some(44),
            channels: Some(2),
            ..View::default()
        };
        p.sync(&v);
        p.draw(&mut c, &skin, &fonts, &v, None);
        let same = |c: &Canvas, sheet: &Canvas, from: protolith::Rect, x: i32, y: i32| {
            (0..from.height).all(|dy| (0..from.width).all(|dx| c.get(x as u32 + dx, y as u32 + dy) == sheet.get(from.x as u32 + dx, from.y as u32 + dy)))
        };
        // 754 s = 12:34: digit 1 from the numbers sheet at the first slot.
        assert!(same(&c, &skin.numbers, r(9, 0, 9, 13), m::DIGITS[0].0, m::DIGITS[0].1));
        assert!(same(&c, &skin.numbers, r(36, 0, 9, 13), m::DIGITS[3].0, m::DIGITS[3].1));
        assert!(same(&c, &skin.monoster, m::STEREO.1, m::STEREO.0.x, m::STEREO.0.y), "stereo lit");
        assert!(same(&c, &skin.monoster, m::MONO.2, m::MONO.0.x, m::MONO.0.y), "mono unlit");
        assert!(same(&c, &skin.playpaus, m::WORKING, 24, 28));
        // The focused title bar, between its buttons.
        assert!(same(&c, &skin.titlebar, r(m::TITLE_ACTIVE.x + 20, 0, 200, 14), 20, 0), "focused title");
        // Remaining time: 46 s, with the nums_ex minus.
        let v = View { remaining: true, ..v };
        p.draw(&mut c, &skin, &fonts, &v, None);
        assert!(same(&c, &skin.numbers, r(99, 0, 9, 13), m::MINUS_CELL.0, m::MINUS_CELL.1));
        assert!(same(&c, &skin.numbers, r(54, 0, 9, 13), m::DIGITS[3].0, m::DIGITS[3].1), "…6");
        // Stopped: no digits (the background shows).
        let stopped = View::default();
        p.draw(&mut c, &skin, &fonts, &stopped, None);
        assert!(same(&c, &skin.main, r(m::DIGITS[0].0, 26, 9, 13), m::DIGITS[0].0, 26));
        assert!(same(&c, &skin.playpaus, m::STOPPED, 24, 28));
    }

    #[test]
    fn long_titles_scroll() {
        let mut p = MainPane::new();
        let short = View { marquee: "SHORT".into(), ..View::default() };
        assert!(!p.tick(&short));
        let long = View { marquee: "A".repeat(40), ..View::default() };
        assert!(p.tick(&long));
        assert_eq!(p.marquee, 1);
        for _ in 0..(5 * 47 - 1) {
            p.tick(&long);
        }
        assert_eq!(p.marquee, 0, "wraps after the text and separator");
    }
}
