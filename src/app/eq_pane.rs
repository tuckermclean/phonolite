//! The equalizer window: ON/AUTO, presets, preamp and ten bands, and the
//! response curve.

use protolith::canvas::Canvas;
use protolith::ui::{Button, Orientation, Response, Slider, Toggle};
use protolith::x11::{Event, MouseButton};
use protolith::Rect;

use super::common::{Action, MenuKind};
use crate::eq::{Equalizer, RANGE_DB};
use crate::skin::layout::eq as q;
use crate::skin::Skin;

/// Slider steps: one per dB, ±12.
const STEPS: u32 = 24;

/// The equalizer window's widgets.
pub struct EqPane {
    close: Button,
    on: Toggle,
    auto: Toggle,
    presets: Button,
    preamp: Slider,
    bands: [Slider; 10],
}

impl Default for EqPane {
    fn default() -> EqPane {
        EqPane::new()
    }
}

fn to_slider(db: f64) -> u32 {
    ((db.clamp(-RANGE_DB, RANGE_DB) + RANGE_DB) / (2.0 * RANGE_DB) * f64::from(STEPS)).round() as u32
}

fn to_db(v: u32) -> f64 {
    f64::from(v) / f64::from(STEPS) * 2.0 * RANGE_DB - RANGE_DB
}

impl EqPane {
    /// Widgets at their classic places.
    pub fn new() -> EqPane {
        let slider = |x: i32| Slider::new(Rect::new(x, q::BAND_Y, 14, 63), Orientation::Vertical, STEPS).with_thumb(11);
        let mut p = EqPane {
            close: Button::new(q::CLOSE.0),
            on: Toggle::new(q::ON.0, false),
            auto: Toggle::new(q::AUTO.0, false),
            presets: Button::new(q::PRESETS.0),
            preamp: slider(q::PREAMP.x),
            bands: std::array::from_fn(|i| slider(q::BAND_X + q::BAND_STEP * i as i32)),
        };
        p.set(&Equalizer::default(), false);
        p
    }

    /// Show these settings (unless the user is dragging a slider).
    pub fn set(&mut self, eq: &Equalizer, auto: bool) {
        if !self.on.is_down() {
            self.on.set_on(eq.on);
        }
        if !self.auto.is_down() {
            self.auto.set_on(auto);
        }
        if !self.preamp.is_dragging() {
            self.preamp.set_value(to_slider(eq.preamp));
        }
        for (s, db) in self.bands.iter_mut().zip(eq.bands) {
            if !s.is_dragging() {
                s.set_value(to_slider(db));
            }
        }
    }

    /// The settings the widgets show.
    pub fn equalizer(&self) -> Equalizer {
        Equalizer { on: self.on.is_on(), preamp: to_db(self.preamp.value()), bands: std::array::from_fn(|i| to_db(self.bands[i].value())) }
    }

    /// Handle an event (pane coordinates).
    pub fn handle(&mut self, e: &Event) -> Vec<Action> {
        let mut out = Vec::new();
        let mut changed = false;
        if self.close.handle(e) == Response::Clicked {
            out.push(Action::ShowEq(false));
        }
        changed |= self.on.handle(e) == Response::Changed;
        if self.auto.handle(e) == Response::Changed {
            out.push(Action::EqAuto(self.auto.is_on()));
        }
        if self.presets.handle(e) == Response::Clicked {
            if let Event::Button { root_x, root_y, .. } = *e {
                out.push(Action::Menu(MenuKind::Presets, root_x, root_y));
            }
        }
        for s in std::iter::once(&mut self.preamp).chain(self.bands.iter_mut()) {
            changed |= matches!(s.handle(e), Response::Changed);
        }
        if changed {
            out.push(Action::Equalizer(self.equalizer()));
        }
        if let Event::Button { button: MouseButton::Left, pressed: true, x, y, root_x, root_y, .. } = *e {
            let on_control = [q::CLOSE.0, q::ON.0, q::AUTO.0, q::PRESETS.0, self.preamp.rect()].iter().any(|r| r.contains(x, y))
                || self.bands.iter().any(|b| b.rect().contains(x, y));
            if !on_control {
                out.push(Action::DragWindow(root_x, root_y));
            }
        }
        if let Event::Button { button: MouseButton::Right, pressed: true, root_x, root_y, .. } = *e {
            out.push(Action::Menu(MenuKind::Presets, root_x, root_y));
        }
        out
    }

    /// Draw the pane at the canvas origin.
    pub fn draw(&self, c: &mut Canvas, skin: &Skin, focused: bool) {
        let sheet = skin.eqmain.frame();
        c.blit(&sheet, q::BACKGROUND, 0, 0);
        c.blit(&sheet, if focused { q::TITLE_ACTIVE } else { q::TITLE_INACTIVE }, 0, 0);
        self.close.draw_sprite(c, &sheet, &q::CLOSE.1);
        self.on.draw_sprite(c, &sheet, &q::ON.1);
        self.auto.draw_sprite(c, &sheet, &q::AUTO.1);
        self.presets.draw_sprite(c, &sheet, &q::PRESETS.1);
        self.preamp.draw_sprite(c, &sheet, &q::SLIDER_SPRITES);
        for b in &self.bands {
            b.draw_sprite(c, &sheet, &q::SLIDER_SPRITES);
        }
        self.draw_graph(c, skin);
    }

    /// The curve: band values joined across the graph, colored by height
    /// from the skin's color column; the preamp as a line.
    fn draw_graph(&self, c: &mut Canvas, skin: &Skin) {
        let sheet = skin.eqmain.frame();
        let g = q::GRAPH;
        c.blit(&sheet, q::GRAPH_BACKGROUND, g.x, g.y);
        let h = g.height as i32;
        let y_of = |v: u32| (h - 1) - (v as i32 * (h - 1) / STEPS as i32);
        let pre = y_of(self.preamp.value());
        c.blit(&sheet, q::PREAMP_LINE, g.x, g.y + pre);
        let vals: Vec<u32> = self.bands.iter().map(Slider::value).collect();
        // Band i sits at x = 2 + 12 i (113 px across ten bands).
        let mut prev: Option<i32> = None;
        for x in 0..g.width as i32 {
            let t = (f64::from(x - 2) / 12.0).clamp(0.0, 9.0);
            let i = (t.floor() as usize).min(8);
            let f = t - i as f64;
            let v = f64::from(vals[i]) * (1.0 - f) + f64::from(vals[i + 1]) * f;
            let y = y_of(v.round() as u32);
            let (a, b) = match prev {
                Some(p) if p < y => (p + 1, y),
                Some(p) if p > y => (y, p - 1),
                _ => (y, y),
            };
            for yy in a..=b {
                let color = skin.eqmain.get(q::GRAPH_COLORS.x as u32, (q::GRAPH_COLORS.y + yy) as u32).unwrap_or(0xffffff);
                c.put(g.x + x, g.y + yy, color);
            }
            prev = Some(y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::common::tests::{click, drag, press, release};
    use crate::skin::builtin;

    #[test]
    fn values_round_trip() {
        for db in [-12.0, -3.0, 0.0, 5.0, 12.0] {
            assert_eq!(to_db(to_slider(db)), db);
        }
        assert_eq!(to_slider(40.0), STEPS);
        let mut p = EqPane::new();
        let eq = Equalizer { on: true, preamp: -4.0, bands: [1.0, 2.0, 3.0, 4.0, 5.0, -1.0, -2.0, -3.0, -4.0, -5.0] };
        p.set(&eq, true);
        assert_eq!(p.equalizer(), eq);
    }

    #[test]
    fn dragging_a_band_reports_the_curve() {
        let mut p = EqPane::new();
        let x = q::BAND_X + 3 * q::BAND_STEP + 7;
        let mut a = p.handle(&press(x, q::BAND_Y + 30));
        a.extend(p.handle(&drag(x, q::BAND_Y + 2)));
        a.extend(p.handle(&release(x, q::BAND_Y + 2)));
        let Some(Action::Equalizer(eq)) = a.iter().rev().find(|a| matches!(a, Action::Equalizer(_))) else { panic!("{a:?}") };
        assert_eq!(eq.bands[3], 12.0, "the top is +12 dB");
        assert_eq!(eq.bands[2], 0.0);
        assert!(!a.iter().any(|a| matches!(a, Action::DragWindow(..))));
        let a: Vec<Action> = click(q::ON.0.x + 3, q::ON.0.y + 3).iter().flat_map(|e| p.handle(e)).collect();
        assert!(matches!(&a[..], [Action::Equalizer(e)] if e.on), "{a:?}");
        let a: Vec<Action> = click(q::PRESETS.0.x + 3, q::PRESETS.0.y + 3).iter().flat_map(|e| p.handle(e)).collect();
        assert!(matches!(a[..], [Action::Menu(MenuKind::Presets, ..)]));
        let a: Vec<Action> = click(268, 7).iter().flat_map(|e| p.handle(e)).collect();
        assert_eq!(a, [Action::ShowEq(false)]);
        assert!(matches!(p.handle(&press(150, 6))[..], [Action::DragWindow(..)]));
    }

    #[test]
    fn the_curve_follows_the_bands() {
        let skin = builtin::skin();
        let mut p = EqPane::new();
        let mut eq = Equalizer::default();
        eq.bands[0] = 12.0;
        eq.bands[9] = -12.0;
        p.set(&eq, false);
        let mut c = Canvas::new(275, 116);
        p.draw(&mut c, &skin, true);
        let g = q::GRAPH;
        let column = |x: i32| {
            (0..g.height as i32)
                .filter(|&y| {
                    c.get((g.x + x) as u32, (g.y + y) as u32) != skin.eqmain.get((q::GRAPH_BACKGROUND.x + x) as u32, (q::GRAPH_BACKGROUND.y + y) as u32)
                })
                .collect::<Vec<_>>()
        };
        assert!(column(0).contains(&0), "first band at the top: {:?}", column(0));
        assert!(column(112).contains(&18), "last band at the bottom: {:?}", column(112));
        assert!(column(56).contains(&9), "the middle at 0 dB: {:?}", column(56));
    }
}
