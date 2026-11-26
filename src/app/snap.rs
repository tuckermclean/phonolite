//! Window docking and snapping, as classic players do it: a window being
//! dragged snaps when an edge comes within [`SNAP`] pixels of another
//! window's edge (side by side, or aligned) or the screen's; windows that
//! share an edge are docked, and the main window carries everything
//! docked to it, directly or through others.

/// How close an edge must come to snap, in screen pixels.
pub const SNAP: i32 = 10;

/// A window's place on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// Left.
    pub x: i32,
    /// Top.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
}

impl Bounds {
    /// A rectangle.
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Bounds {
        Bounds { x, y, w, h }
    }
    /// One past the right edge.
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    /// One past the bottom edge.
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    /// Moved to `(x, y)`.
    pub fn at(self, x: i32, y: i32) -> Bounds {
        Bounds { x, y, ..self }
    }
}

/// `a` and `b` share an edge: they touch, overlapping along it.
pub fn docked(a: Bounds, b: Bounds) -> bool {
    let v_overlap = a.y < b.bottom() && b.y < a.bottom();
    let h_overlap = a.x < b.right() && b.x < a.right();
    ((a.right() == b.x || b.right() == a.x) && v_overlap) || ((a.bottom() == b.y || b.bottom() == a.y) && h_overlap)
}

/// The windows reachable from `start` through shared edges, `start`
/// first. `None` marks a window that is hidden (it docks nothing).
pub fn group(windows: &[Option<Bounds>], start: usize) -> Vec<usize> {
    let mut out = vec![start];
    let mut i = 0;
    while i < out.len() {
        let Some(a) = windows[out[i]] else { break };
        for (j, b) in windows.iter().enumerate() {
            if let Some(b) = b {
                if !out.contains(&j) && docked(a, *b) {
                    out.push(j);
                }
            }
        }
        i += 1;
    }
    out
}

/// How far to nudge windows being dragged together (`moving`, where the
/// pointer would put them) so that they snap: on each axis the smallest
/// adjustment, of at most [`SNAP`], that puts an edge of one of them on an
/// edge of another window — side by side, or aligned, when the two are
/// near each other along the other axis — or of the screen.
pub fn snap(moving: &[Bounds], others: &[Bounds], screen: Bounds) -> (i32, i32) {
    let mut dx: Option<i32> = None;
    let mut dy: Option<i32> = None;
    let consider = |best: &mut Option<i32>, d: i32| {
        if d.abs() <= SNAP && best.map_or(true, |b| d.abs() < b.abs()) {
            *best = Some(d);
        }
    };
    for want in moving {
        for o in others {
            if want.y < o.bottom() + SNAP && o.y < want.bottom() + SNAP {
                for (edge, target) in [(want.x, o.right()), (want.right(), o.x), (want.x, o.x), (want.right(), o.right())] {
                    consider(&mut dx, target - edge);
                }
            }
            if want.x < o.right() + SNAP && o.x < want.right() + SNAP {
                for (edge, target) in [(want.y, o.bottom()), (want.bottom(), o.y), (want.y, o.y), (want.bottom(), o.bottom())] {
                    consider(&mut dy, target - edge);
                }
            }
        }
        for (edge, target) in [(want.x, screen.x), (want.right(), screen.right())] {
            consider(&mut dx, target - edge);
        }
        for (edge, target) in [(want.y, screen.y), (want.bottom(), screen.bottom())] {
            consider(&mut dy, target - edge);
        }
    }
    (dx.unwrap_or(0), dy.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Bounds = Bounds::new(0, 0, 1024, 768);
    const MAIN: Bounds = Bounds::new(100, 100, 275, 116);

    /// Where one window dragged to `want` lands.
    fn to(want: Bounds, others: &[Bounds]) -> (i32, i32) {
        let (dx, dy) = snap(&[want], others, SCREEN);
        (want.x + dx, want.y + dy)
    }

    #[test]
    fn docking_is_sharing_an_edge() {
        let below = Bounds::new(100, 216, 275, 116);
        assert!(docked(MAIN, below) && docked(below, MAIN));
        assert!(docked(MAIN, Bounds::new(375, 150, 275, 116)), "to the right, overlapping vertically");
        assert!(!docked(MAIN, Bounds::new(375, 216, 275, 116)), "corner to corner is not an edge");
        assert!(!docked(MAIN, Bounds::new(100, 217, 275, 116)), "a pixel apart");
        assert!(docked(MAIN, Bounds::new(-150, 50, 250, 60)), "to the left");
    }

    #[test]
    fn groups_follow_chains_of_edges() {
        let eq = Bounds::new(100, 216, 275, 116);
        let pl = Bounds::new(100, 332, 275, 232);
        assert_eq!(group(&[Some(MAIN), Some(eq), Some(pl)], 0), [0, 1, 2], "playlist docked to the equalizer, docked to main");
        assert_eq!(group(&[Some(MAIN), None, Some(pl)], 0), [0], "hidden equalizer: the chain is broken");
        let pl_beside = Bounds::new(375, 100, 275, 232);
        assert_eq!(group(&[Some(MAIN), Some(eq), Some(pl_beside)], 0), [0, 1, 2]);
    }

    #[test]
    fn snaps_to_edges_and_alignment() {
        let eq = Bounds::new(0, 0, 275, 116);
        // Dropped just below main, a little off to the right: docks under it, aligned.
        assert_eq!(to(eq.at(106, 222), &[MAIN]), (100, 216));
        // Just right of main, a little low: beside it, tops aligned.
        assert_eq!(to(eq.at(381, 104), &[MAIN]), (375, 100));
        // Left of main.
        assert_eq!(to(eq.at(100 - 275 - 7, 150), &[MAIN]), (100 - 275, 150), "x snaps; y is too far from any edge");
        // Too far away: stays put.
        assert_eq!(to(eq.at(500, 500), &[MAIN]), (500, 500));
        // The screen's edges.
        assert_eq!(to(eq.at(7, 768 - 116 - 11), &[]), (0, 768 - 116 - 11), "left edge; the bottom is 11 px off: too far");
        assert_eq!(to(eq.at(7, 768 - 116 - 9), &[]), (0, 768 - 116), "9 px: snaps");
        assert_eq!(to(eq.at(1024 - 275 + 4, -5), &[]), (1024 - 275, 0));
        // Not snapped sideways to a window it is nowhere near vertically.
        assert_eq!(to(eq.at(381, 400), &[MAIN]), (381, 400));
        // The nearest edge wins.
        let other = Bounds::new(380, 400, 100, 100);
        assert_eq!(to(eq.at(108, 216 + 3), &[MAIN, other]), (100, 216));
    }

    #[test]
    fn a_docked_group_snaps_by_whichever_member_is_nearest() {
        // Main with the equalizer under it, dragged toward the screen's
        // bottom: the equalizer, 4 px off it, decides.
        let main = MAIN.at(300, 768 - 232 + 4);
        let eq = Bounds::new(300, 768 - 116 + 4, 275, 116);
        assert_eq!(snap(&[main, eq], &[], SCREEN), (0, -4));
        // And a window outside the group, 6 px to main's right, beats it.
        let pl = Bounds::new(300 + 275 + 6, 768 - 232, 275, 232);
        assert_eq!(snap(&[main, eq], &[pl], SCREEN), (6, -4));
    }
}
