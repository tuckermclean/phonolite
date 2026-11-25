//! Where things are: sprite rectangles in the classic 2.x skin sheets, and
//! where each piece goes in the three windows. These are facts about the
//! file format (measured on real sheets), shared by the generated built-in
//! skin and the UI.

use protolith::ui::{ButtonSprites, Frames, SliderSprites, ToggleSprites};
use protolith::Rect;

/// Shorthand.
pub const fn r(x: i32, y: i32, w: u32, h: u32) -> Rect {
    Rect::new(x, y, w, h)
}

/// Window width (all three windows).
pub const WIDTH: u32 = 275;
/// Main and equalizer window height.
pub const HEIGHT: u32 = 116;

/// Sheet sizes, for the built-in skin and for sanity checks.
pub mod size {
    /// main.bmp.
    pub const MAIN: (u32, u32) = (275, 116);
    /// titlebar.bmp.
    pub const TITLEBAR: (u32, u32) = (344, 87);
    /// cbuttons.bmp.
    pub const CBUTTONS: (u32, u32) = (136, 36);
    /// shufrep.bmp.
    pub const SHUFREP: (u32, u32) = (92, 85);
    /// volume.bmp and balance.bmp.
    pub const VOLUME: (u32, u32) = (68, 433);
    /// posbar.bmp.
    pub const POSBAR: (u32, u32) = (307, 10);
    /// playpaus.bmp.
    pub const PLAYPAUS: (u32, u32) = (42, 9);
    /// monoster.bmp.
    pub const MONOSTER: (u32, u32) = (58, 24);
    /// numbers.bmp (nums_ex.bmp is 108 wide: a blank and a minus follow).
    pub const NUMBERS: (u32, u32) = (99, 13);
    /// text.bmp.
    pub const TEXT: (u32, u32) = (155, 18);
    /// pledit.bmp.
    pub const PLEDIT: (u32, u32) = (280, 186);
    /// eqmain.bmp.
    pub const EQMAIN: (u32, u32) = (275, 315);
}

// ------------------------------------------------------------ main window

/// The main window's parts.
pub mod main {
    use super::*;

    /// Title bar, drawn at the top.
    pub const TITLE: Rect = r(0, 0, 275, 14);
    /// titlebar.bmp: focused.
    pub const TITLE_ACTIVE: Rect = r(27, 0, 275, 14);
    /// titlebar.bmp: unfocused.
    pub const TITLE_INACTIVE: Rect = r(27, 15, 275, 14);

    /// Title bar buttons: where, and titlebar.bmp up/down.
    pub const MENU: (Rect, ButtonSprites) = (r(6, 3, 9, 9), ButtonSprites { up: r(0, 0, 9, 9), down: r(0, 9, 9, 9) });
    /// Minimize.
    pub const MINIMIZE: (Rect, ButtonSprites) = (r(244, 3, 9, 9), ButtonSprites { up: r(9, 0, 9, 9), down: r(9, 9, 9, 9) });
    /// Window shade (here: toggles the playlist and equalizer).
    pub const SHADE: (Rect, ButtonSprites) = (r(254, 3, 9, 9), ButtonSprites { up: r(0, 18, 9, 9), down: r(9, 18, 9, 9) });
    /// Close.
    pub const CLOSE: (Rect, ButtonSprites) = (r(264, 3, 9, 9), ButtonSprites { up: r(18, 0, 9, 9), down: r(18, 9, 9, 9) });

    /// The clutter bar's column (O A I D V), on main.bmp.
    pub const CLUTTER: Rect = r(10, 22, 8, 43);
    /// Each clutter button's offset and height within the bar.
    pub const CLUTTER_BUTTONS: [(i32, u32); 5] = [(3, 8), (11, 7), (18, 7), (25, 8), (33, 7)];
    /// titlebar.bmp: clutter button `i` pressed is at (CLUTTER_DOWN_X + 8i, CLUTTER_DOWN_Y + offset).
    pub const CLUTTER_DOWN_X: i32 = 304;
    /// See [`CLUTTER_DOWN_X`].
    pub const CLUTTER_DOWN_Y: i32 = 44;

    /// Play state indicator.
    pub const PLAY_STATE: Rect = r(24, 28, 9, 9);
    /// playpaus.bmp: playing, paused, stopped.
    pub const PLAYING: Rect = r(0, 0, 9, 9);
    /// See [`PLAYING`].
    pub const PAUSED: Rect = r(9, 0, 9, 9);
    /// See [`PLAYING`].
    pub const STOPPED: Rect = r(18, 0, 9, 9);
    /// playpaus.bmp: the strip left of the play arrow, lit while decoding.
    pub const WORKING: Rect = r(39, 0, 3, 9);
    /// See [`WORKING`].
    pub const NOT_WORKING: Rect = r(36, 0, 3, 9);
    /// Where the working strip goes.
    pub const WORK_INDICATOR: Rect = r(24, 28, 3, 9);

    /// Time digits (minutes tens, ones, seconds tens, ones), 9x13 each.
    pub const DIGITS: [(i32, i32); 4] = [(48, 26), (60, 26), (78, 26), (90, 26)];
    /// Where the minus of a remaining time goes (nums_ex.bmp: a whole cell).
    pub const MINUS_CELL: (i32, i32) = (36, 26);
    /// Plain numbers.bmp has no minus: borrow the middle bar of the 2.
    pub const MINUS_BAR: Rect = r(20, 6, 5, 1);
    /// Where the borrowed bar goes.
    pub const MINUS_BAR_AT: (i32, i32) = (38, 32);
    /// The clickable time display (toggles elapsed/remaining).
    pub const TIME_AREA: Rect = r(36, 26, 63, 13);

    /// The visualizer.
    pub const VIS: Rect = r(24, 43, 76, 16);
    /// Scrolling title text.
    pub const MARQUEE: Rect = r(111, 27, 154, 6);
    /// Bitrate, 3 characters right-aligned.
    pub const KBPS: Rect = r(111, 43, 15, 6);
    /// Sample rate, 2 characters.
    pub const KHZ: Rect = r(156, 43, 10, 6);

    /// Mono and stereo lamps: where, and monoster.bmp lit/unlit.
    pub const MONO: (Rect, Rect, Rect) = (r(212, 41, 27, 12), r(29, 0, 27, 12), r(29, 12, 27, 12));
    /// See [`MONO`].
    pub const STEREO: (Rect, Rect, Rect) = (r(239, 41, 29, 12), r(0, 0, 29, 12), r(0, 12, 29, 12));

    /// Volume slider.
    pub const VOLUME: Rect = r(107, 57, 68, 13);
    /// Balance slider.
    pub const BALANCE: Rect = r(177, 57, 38, 13);
    /// volume.bmp: 28 track frames and the thumb.
    pub const VOLUME_SPRITES: SliderSprites =
        SliderSprites { track: Frames::column(r(0, 0, 68, 13), 28, 15), centered: false, thumb: r(15, 422, 14, 11), thumb_down: r(0, 422, 14, 11) };
    /// balance.bmp: 28 frames, 38 wide from x = 9, filled outward from the middle.
    pub const BALANCE_SPRITES: SliderSprites =
        SliderSprites { track: Frames::column(r(9, 0, 38, 13), 28, 15), centered: true, thumb: r(15, 422, 14, 11), thumb_down: r(0, 422, 14, 11) };

    /// Equalizer and playlist window toggles.
    pub const EQ_TOGGLE: (Rect, ToggleSprites) =
        (r(219, 58, 23, 12), ToggleSprites { off: r(0, 61, 23, 12), off_down: r(46, 61, 23, 12), on: r(0, 73, 23, 12), on_down: r(46, 73, 23, 12) });
    /// See [`EQ_TOGGLE`].
    pub const PL_TOGGLE: (Rect, ToggleSprites) =
        (r(242, 58, 23, 12), ToggleSprites { off: r(23, 61, 23, 12), off_down: r(69, 61, 23, 12), on: r(23, 73, 23, 12), on_down: r(69, 73, 23, 12) });

    /// Seek bar.
    pub const POSBAR: Rect = r(16, 72, 248, 10);
    /// posbar.bmp.
    pub const POSBAR_SPRITES: SliderSprites =
        SliderSprites { track: Frames::single(r(0, 0, 248, 10)), centered: false, thumb: r(248, 0, 29, 10), thumb_down: r(278, 0, 29, 10) };

    /// Transport buttons: previous, play, pause, stop, next, eject.
    pub const TRANSPORT: [(Rect, ButtonSprites); 6] = [
        (r(16, 88, 23, 18), ButtonSprites { up: r(0, 0, 23, 18), down: r(0, 18, 23, 18) }),
        (r(39, 88, 23, 18), ButtonSprites { up: r(23, 0, 23, 18), down: r(23, 18, 23, 18) }),
        (r(62, 88, 23, 18), ButtonSprites { up: r(46, 0, 23, 18), down: r(46, 18, 23, 18) }),
        (r(85, 88, 23, 18), ButtonSprites { up: r(69, 0, 23, 18), down: r(69, 18, 23, 18) }),
        (r(108, 88, 22, 18), ButtonSprites { up: r(92, 0, 22, 18), down: r(92, 18, 22, 18) }),
        (r(136, 89, 22, 16), ButtonSprites { up: r(114, 0, 22, 16), down: r(114, 16, 22, 16) }),
    ];

    /// Shuffle and repeat.
    pub const SHUFFLE: (Rect, ToggleSprites) =
        (r(164, 89, 47, 15), ToggleSprites { off: r(28, 0, 47, 15), off_down: r(28, 15, 47, 15), on: r(28, 30, 47, 15), on_down: r(28, 45, 47, 15) });
    /// See [`SHUFFLE`].
    pub const REPEAT: (Rect, ToggleSprites) =
        (r(210, 89, 28, 15), ToggleSprites { off: r(0, 0, 28, 15), off_down: r(0, 15, 28, 15), on: r(0, 30, 28, 15), on_down: r(0, 45, 28, 15) });
}

// --------------------------------------------------------- equalizer window

/// The equalizer window's parts.
pub mod eq {
    use super::*;

    /// eqmain.bmp: the window background.
    pub const BACKGROUND: Rect = r(0, 0, 275, 116);
    /// eqmain.bmp: title bar focused / unfocused.
    pub const TITLE_ACTIVE: Rect = r(0, 134, 275, 14);
    /// See [`TITLE_ACTIVE`].
    pub const TITLE_INACTIVE: Rect = r(0, 149, 275, 14);
    /// Close button.
    pub const CLOSE: (Rect, ButtonSprites) = (r(264, 3, 9, 9), ButtonSprites { up: r(0, 116, 9, 9), down: r(0, 125, 9, 9) });
    /// ON.
    pub const ON: (Rect, ToggleSprites) =
        (r(14, 18, 26, 12), ToggleSprites { off: r(10, 119, 26, 12), off_down: r(128, 119, 26, 12), on: r(69, 119, 26, 12), on_down: r(187, 119, 26, 12) });
    /// AUTO (load presets per track — here: keep the curve across restarts).
    pub const AUTO: (Rect, ToggleSprites) =
        (r(40, 18, 32, 12), ToggleSprites { off: r(36, 119, 32, 12), off_down: r(164, 119, 32, 12), on: r(95, 119, 32, 12), on_down: r(223, 119, 32, 12) });
    /// PRESETS.
    pub const PRESETS: (Rect, ButtonSprites) = (r(217, 18, 44, 12), ButtonSprites { up: r(224, 164, 44, 12), down: r(224, 176, 44, 12) });
    /// The response curve.
    pub const GRAPH: Rect = r(86, 17, 113, 19);
    /// eqmain.bmp: the curve's background.
    pub const GRAPH_BACKGROUND: Rect = r(0, 294, 113, 19);
    /// eqmain.bmp: one column of 19 colors, top to bottom, for the curve.
    pub const GRAPH_COLORS: Rect = r(115, 294, 1, 19);
    /// eqmain.bmp: the preamp level line.
    pub const PREAMP_LINE: Rect = r(0, 314, 113, 1);
    /// Preamp slider.
    pub const PREAMP: Rect = r(21, 38, 14, 63);
    /// Band slider `i` is at x = BAND_X + BAND_STEP * i.
    pub const BAND_X: i32 = 78;
    /// See [`BAND_X`].
    pub const BAND_STEP: i32 = 18;
    /// Band sliders' y.
    pub const BAND_Y: i32 = 38;
    /// eqmain.bmp: 28 slider backgrounds (14 per row) and the thumb.
    pub const SLIDER_SPRITES: SliderSprites = SliderSprites {
        track: Frames { first: r(13, 164, 14, 63), count: 28, columns: 14, step: (15, 65) },
        centered: false,
        thumb: r(0, 164, 11, 11),
        thumb_down: r(0, 176, 11, 11),
    };
}

// ---------------------------------------------------------- playlist window

/// The playlist window's parts (it is resizable: positions near the
/// bottom and right edges are offsets from them).
pub mod pl {
    use super::*;

    /// Smallest height.
    pub const MIN_HEIGHT: u32 = 116;
    /// Height grows in steps of a side tile.
    pub const HEIGHT_STEP: u32 = 29;
    /// Default height.
    pub const DEFAULT_HEIGHT: u32 = 232;

    /// pledit.bmp, focused title bar pieces: left corner, title, filler, right corner.
    pub const TOP_ACTIVE: [Rect; 4] = [r(0, 0, 25, 20), r(26, 0, 100, 20), r(127, 0, 25, 20), r(153, 0, 25, 20)];
    /// Unfocused.
    pub const TOP_INACTIVE: [Rect; 4] = [r(0, 21, 25, 20), r(26, 21, 100, 20), r(127, 21, 25, 20), r(153, 21, 25, 20)];
    /// Left edge tile (12 wide).
    pub const LEFT: Rect = r(0, 42, 12, 29);
    /// Right edge tile, with the scroll groove (20 wide).
    pub const RIGHT: Rect = r(31, 42, 20, 29);
    /// Bottom-left corner, with the ADD/REM/SEL/MISC buttons.
    pub const BOTTOM_LEFT: Rect = r(0, 72, 125, 38);
    /// Bottom-right corner: time readouts, mini transport, LIST button.
    pub const BOTTOM_RIGHT: Rect = r(126, 72, 150, 38);
    /// Bottom filler.
    pub const BOTTOM: Rect = r(179, 0, 25, 38);
    /// Scroll thumb up / dragged.
    pub const SCROLL_THUMB: Rect = r(52, 53, 8, 18);
    /// See [`SCROLL_THUMB`].
    pub const SCROLL_THUMB_DOWN: Rect = r(61, 53, 8, 18);
    /// Close button pressed (the unpressed one is part of the corner);
    /// offset from the right edge.
    pub const CLOSE_DOWN: Rect = r(52, 42, 9, 9);
    /// Close button position: x from the right edge, y.
    pub const CLOSE_AT: (i32, i32) = (-11, 3);

    /// The track list's inset from the window edges: left, top, right, bottom.
    pub const LIST_INSET: (i32, i32, i32, i32) = (12, 20, 20, 38);
    /// The scroll thumb's x from the right edge.
    pub const SCROLL_X: i32 = -15;

    /// Menu buttons along the bottom: ADD, REM, SEL, MISC (x from the left)
    /// and LIST (x from the right), 25x18 (LIST 22x18), y from the bottom.
    pub const MENU_BUTTONS: [(i32, u32); 4] = [(14, 25), (43, 25), (72, 25), (101, 25)];
    /// See [`MENU_BUTTONS`].
    pub const LIST_BUTTON: (i32, u32) = (-46, 22);
    /// See [`MENU_BUTTONS`].
    pub const MENU_Y: i32 = -30;

    /// pledit.bmp menu items: for each menu, the items' x (up, pressed),
    /// the first item's y, and the bar beside them; items are 22x18,
    /// stacked 19 apart, the first at the bottom of the menu.
    pub const MENUS: [(i32, i32, usize); 5] = [(0, 23, 3), (54, 77, 4), (104, 127, 3), (154, 177, 3), (204, 227, 3)];
    /// See [`MENUS`].
    pub const MENU_ITEM_Y: i32 = 111;
    /// See [`MENUS`].
    pub const MENU_BARS: [i32; 5] = [48, 100, 150, 200, 250];

    /// Running time ("sel/total"), relative to the bottom-right corner.
    pub const RUNNING_TIME: (i32, i32) = (7, 10);
    /// The mini time display, relative to the bottom-right corner.
    pub const MINI_TIME: (i32, i32) = (66, 23);
    /// Mini transport buttons (x, width), relative to the bottom-right
    /// corner, y 22..30: previous, play, pause, stop, next, eject.
    pub const MINI_BUTTONS: [(i32, u32); 6] = [(6, 8), (14, 8), (22, 10), (32, 9), (41, 8), (49, 9)];
    /// See [`MINI_BUTTONS`].
    pub const MINI_Y: (i32, u32) = (22, 8);
    /// The resize grip, relative to the bottom-right corner.
    pub const GRIP: Rect = r(130, 18, 20, 20);
}

// --------------------------------------------------------------- text sheets

/// text.bmp's 5x6 cells, row by row (`\0` marks cells never drawn).
pub const TEXT_LAYOUT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ\"@\0\0 \n0123456789\u{2026}.:()-'!_+\\/[]^&%,=$#\n\u{c5}\u{d6}\u{c4}?*";

/// numbers.bmp's 9x13 cells (nums_ex.bmp adds a blank and a minus).
pub const NUMBERS_LAYOUT: &str = "0123456789 -";

/// Map text to what text.bmp can show: upper case, common accents
/// stripped, anything else as a space.
pub fn text_for_sheet(s: &str) -> String {
    s.chars()
        .flat_map(char::to_uppercase)
        .map(|c| match c {
            'À' | 'Á' | 'Â' | 'Ã' => 'A',
            'Ç' => 'C',
            'È' | 'É' | 'Ê' | 'Ë' => 'E',
            'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
            'Ñ' => 'N',
            'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ø' => 'O',
            'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
            'Ý' => 'Y',
            '<' | '{' => '(',
            '>' | '}' => ')',
            '|' => '/',
            '`' | '´' => '\'',
            '~' => '-',
            ';' => ':',
            '\t' => ' ',
            c if c != '\0' && (TEXT_LAYOUT.contains(c)) => c,
            _ => ' ',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_layout_rows_fit_the_sheet() {
        for (row, line) in TEXT_LAYOUT.lines().enumerate() {
            assert!(line.chars().count() <= 31, "row {row}");
        }
        assert_eq!(TEXT_LAYOUT.lines().next().unwrap().chars().position(|c| c == ' '), Some(30));
        let row1: Vec<char> = TEXT_LAYOUT.lines().nth(1).unwrap().chars().collect();
        assert_eq!((row1[10], row1[11], row1[12], row1[30]), ('\u{2026}', '.', ':', '#'));
    }

    #[test]
    fn text_mapping() {
        assert_eq!(text_for_sheet("Héllo <wörld> 1:23 ~ ok?"), "HELLO (WÖRLD) 1:23 - OK?", "Ö is on the sheet");
        assert_eq!(text_for_sheet("日本\0"), "   ");
        assert_eq!(text_for_sheet("Åsa"), "ÅSA");
    }

    #[test]
    fn sprites_lie_inside_their_sheets() {
        let inside = |rr: Rect, (w, h): (u32, u32)| rr.x >= 0 && rr.y >= 0 && rr.x + rr.width as i32 <= w as i32 && rr.y + rr.height as i32 <= h as i32;
        for (_, s) in main::TRANSPORT {
            assert!(inside(s.up, size::CBUTTONS) && inside(s.down, size::CBUTTONS));
        }
        for (_, s) in [main::SHUFFLE, main::REPEAT, main::EQ_TOGGLE, main::PL_TOGGLE] {
            for rr in [s.off, s.off_down, s.on, s.on_down] {
                assert!(inside(rr, size::SHUFREP), "{rr:?}");
            }
        }
        for i in 0..28 {
            assert!(inside(main::VOLUME_SPRITES.track.rect(i), size::VOLUME));
            assert!(inside(main::BALANCE_SPRITES.track.rect(i), size::VOLUME));
            assert!(inside(eq::SLIDER_SPRITES.track.rect(i), size::EQMAIN));
        }
        for (_, s) in [eq::ON, eq::AUTO] {
            for rr in [s.off, s.off_down, s.on, s.on_down] {
                assert!(inside(rr, size::EQMAIN), "{rr:?}");
            }
        }
        for rr in pl::TOP_ACTIVE.iter().chain(&pl::TOP_INACTIVE).chain(&[
            pl::LEFT,
            pl::RIGHT,
            pl::BOTTOM_LEFT,
            pl::BOTTOM_RIGHT,
            pl::BOTTOM,
            pl::SCROLL_THUMB,
            pl::SCROLL_THUMB_DOWN,
        ]) {
            assert!(inside(*rr, size::PLEDIT), "{rr:?}");
        }
        for (k, (dy, h)) in main::CLUTTER_BUTTONS.iter().enumerate() {
            assert!(inside(r(main::CLUTTER_DOWN_X + 8 * k as i32, main::CLUTTER_DOWN_Y + dy, 8, *h), size::TITLEBAR));
        }
    }
}
