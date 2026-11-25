//! phonolite's own skin, drawn in code in the classic sheet layout:
//! speckled volcanic stone, a mint LCD, amber lamps. Original art — no
//! pixel of it comes from another skin.

use protolith::canvas::Canvas;
use protolith::Rect;

use super::layout::{self, eq, main, pl, r, size};
use super::{PlaylistColors, Skin};

// Palette.
const STONE: u32 = 0x3b4642;
const STONE_HI: u32 = 0x5d6a65;
const STONE_LO: u32 = 0x252d2a;
const EDGE_LIGHT: u32 = 0x8c9a94;
const EDGE_DARK: u32 = 0x0b0f0e;
const LCD: u32 = 0x08100e;
const LCD_GRID: u32 = 0x14231e;
const INK: u32 = 0x8ff0c8;
const INK_DIM: u32 = 0x1f4034;
const AMBER: u32 = 0xffb347;
const AMBER_DIM: u32 = 0x4a3416;
const LABEL: u32 = 0xa9b8b1;
const STEEL: u32 = 0x56635e;
const STEEL_DOWN: u32 = 0x404b47;

/// A 3x5 font, one octal digit per row (4 = left pixel, 1 = right).
const GLYPHS: &[(char, u16)] = &[
    ('A', 0o75755),
    ('B', 0o65656),
    ('C', 0o74447),
    ('D', 0o65556),
    ('E', 0o74647),
    ('F', 0o74644),
    ('G', 0o74557),
    ('H', 0o55755),
    ('I', 0o72227),
    ('J', 0o11157),
    ('K', 0o55655),
    ('L', 0o44447),
    ('M', 0o57755),
    ('N', 0o75555),
    ('O', 0o75557),
    ('P', 0o75744),
    ('Q', 0o75571),
    ('R', 0o75765),
    ('S', 0o74717),
    ('T', 0o72222),
    ('U', 0o55557),
    ('V', 0o55552),
    ('W', 0o55775),
    ('X', 0o55255),
    ('Y', 0o55222),
    ('Z', 0o71247),
    ('"', 0o50000),
    ('@', 0o75547),
    ('0', 0o75557),
    ('1', 0o26227),
    ('2', 0o71747),
    ('3', 0o71717),
    ('4', 0o55711),
    ('5', 0o74717),
    ('6', 0o74757),
    ('7', 0o71111),
    ('8', 0o75757),
    ('9', 0o75717),
    ('\u{2026}', 0o00005),
    ('.', 0o00002),
    (':', 0o02020),
    ('(', 0o24442),
    (')', 0o42224),
    ('-', 0o00700),
    ('\'', 0o22000),
    ('!', 0o22202),
    ('_', 0o00007),
    ('+', 0o02720),
    ('\\', 0o44211),
    ('/', 0o11244),
    ('[', 0o64446),
    (']', 0o31113),
    ('^', 0o25000),
    ('&', 0o25253),
    ('%', 0o51245),
    (',', 0o00024),
    ('=', 0o07070),
    ('$', 0o36736),
    ('#', 0o57575),
    ('\u{c5}', 0o27575),
    ('\u{d6}', 0o52552),
    ('\u{c4}', 0o52575),
    ('?', 0o71202),
    ('*', 0o05250),
];

fn glyph(c: &mut Canvas, ch: char, x: i32, y: i32, color: u32) {
    if let Some(&(_, bits)) = GLYPHS.iter().find(|(g, _)| *g == ch) {
        for row in 0..5 {
            for col in 0..3 {
                if bits >> ((4 - row) * 3 + (2 - col)) & 1 == 1 {
                    c.put(x + col, y + row, color);
                }
            }
        }
    }
}

/// 3x5 text, 4 px per character.
fn text(c: &mut Canvas, s: &str, x: i32, y: i32, color: u32) {
    for (i, ch) in s.chars().enumerate() {
        glyph(c, ch, x + 4 * i as i32, y, color);
    }
}

/// Text centered on `cx`.
fn text_centered(c: &mut Canvas, s: &str, cx: i32, y: i32, color: u32) {
    let w = 4 * s.chars().count() as i32 - 1;
    text(c, s, cx - w / 2, y, color);
}

/// A 5x7 alphabet for window titles, one row per byte (bit 4 = left).
/// The 3x5 font is too small to read in a title bar, and doubling it
/// fills the letters in.
const TITLE_GLYPHS: [[u8; 7]; 26] = [
    [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11], // A
    [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e], // B
    [0x0e, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0e], // C
    [0x1e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1e], // D
    [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f], // E
    [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10], // F
    [0x0e, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0f], // G
    [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11], // H
    [0x0e, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e], // I
    [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0c], // J
    [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11], // K
    [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f], // L
    [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11], // M
    [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11], // N
    [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e], // O
    [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10], // P
    [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d], // Q
    [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11], // R
    [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e], // S
    [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04], // T
    [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e], // U
    [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04], // V
    [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0a], // W
    [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11], // X
    [0x11, 0x11, 0x0a, 0x04, 0x04, 0x04, 0x04], // Y
    [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f], // Z
];

/// Letter spacing in titles.
const TITLE_GAP: i32 = 2;

/// A title glyph's rows and its used columns (I is narrower than M).
fn title_glyph(ch: char) -> Option<([u8; 7], i32, i32)> {
    let rows = TITLE_GLYPHS[(ch.to_ascii_uppercase() as usize).checked_sub('A' as usize).filter(|&i| i < 26)?];
    let used = rows.iter().fold(0, |a, r| a | r);
    let first = (0..5).find(|c| used >> (4 - c) & 1 == 1)?;
    let last = (0..5).rev().find(|c| used >> (4 - c) & 1 == 1)?;
    Some((rows, first, last - first + 1))
}

/// Width of a title in pixels.
fn title_width(s: &str) -> i32 {
    let widths: Vec<i32> = s.chars().map(|ch| title_glyph(ch).map_or(3, |(_, _, w)| w)).collect();
    widths.iter().sum::<i32>() + TITLE_GAP * (widths.len() as i32 - 1).max(0)
}

/// A window title, 7 px tall, centered on `cx`, with a dark shadow down
/// and to the right so it stands off the stone.
fn title(c: &mut Canvas, s: &str, cx: i32, y: i32, color: u32) {
    for (dx, dy, col) in [(1, 1, EDGE_DARK), (0, 0, color)] {
        let mut x = cx - title_width(s) / 2;
        for ch in s.chars() {
            let Some((rows, first, w)) = title_glyph(ch) else {
                x += 3 + TITLE_GAP;
                continue;
            };
            for (r, bits) in rows.iter().enumerate() {
                for col_i in 0..w {
                    if bits >> (4 - first - col_i) & 1 == 1 {
                        c.put(x + col_i + dx, y + r as i32 + dy, col);
                    }
                }
            }
            x += w + TITLE_GAP;
        }
    }
}

/// Channel-wise mix, `t` of 255 toward `b`.
fn mix(a: u32, b: u32, t: u32) -> u32 {
    let t = t.min(255);
    let ch = |s: u32| ((((a >> s) & 255) * (255 - t) + ((b >> s) & 255) * t) / 255) << s;
    ch(16) | ch(8) | ch(0)
}

/// A cheap position hash for texture.
fn hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

/// Speckled stone: a base color with dark grains and pale feldspar flecks.
fn stone(c: &mut Canvas, area: Rect, base: u32, seed: u32) {
    for y in area.y..area.y + area.height as i32 {
        for x in area.x..area.x + area.width as i32 {
            let h = hash(x, y, seed) % 1000;
            let col = match h {
                0..=59 => mix(base, 0x000000, 70),
                60..=89 => mix(base, 0xd8e4dc, 60),
                90..=94 => mix(base, 0xe8f0ea, 130),
                _ => mix(base, if h % 2 == 0 { 0x000000 } else { 0xffffff }, h % 17),
            };
            c.put(x, y, col);
        }
    }
}

fn frame(c: &mut Canvas, rr: Rect, top_left: u32, bottom_right: u32) {
    c.fill_rect(r(rr.x, rr.y, rr.width, 1), top_left);
    c.fill_rect(r(rr.x, rr.y, 1, rr.height), top_left);
    c.fill_rect(r(rr.x, rr.y + rr.height as i32 - 1, rr.width, 1), bottom_right);
    c.fill_rect(r(rr.x + rr.width as i32 - 1, rr.y, 1, rr.height), bottom_right);
}

fn raised(c: &mut Canvas, rr: Rect, face: u32) {
    c.fill_rect(rr, face);
    frame(c, rr, EDGE_LIGHT, EDGE_DARK);
}

fn pressed(c: &mut Canvas, rr: Rect, face: u32) {
    c.fill_rect(rr, face);
    frame(c, rr, EDGE_DARK, EDGE_LIGHT);
}

/// A recessed LCD panel.
fn lcd(c: &mut Canvas, rr: Rect) {
    c.fill_rect(rr, LCD);
    frame(c, rr, EDGE_DARK, STONE_HI);
}

/// Every sheet, generated.
pub fn skin() -> Skin {
    Skin {
        name: "built-in".into(),
        main: main_sheet(),
        titlebar: titlebar(),
        cbuttons: cbuttons(),
        shufrep: shufrep(),
        volume: slider_sheet(false),
        balance: slider_sheet(true),
        posbar: posbar(),
        playpaus: playpaus(),
        monoster: monoster(),
        numbers: numbers(),
        numbers_ex: true,
        text: text_sheet(),
        pledit: pledit(),
        eqmain: eqmain(),
        viscolor: viscolor(),
        playlist: PlaylistColors { normal: 0x8ff0c8, current: 0xffffff, normal_bg: 0x08100e, selected_bg: 0x2a5246 },
        region: None,
        fallbacks: Vec::new(),
    }
}

fn viscolor() -> [u32; 24] {
    let mut v = [0; 24];
    v[0] = LCD;
    v[1] = LCD_GRID;
    // Spectrum, top (hot) to bottom (cool): amber through mint to teal.
    for i in 0..16u32 {
        v[2 + i as usize] = if i < 6 { mix(0xffd27a, AMBER, i * 50) } else { mix(0x9ff5d2, 0x1f7a5c, (i - 6) * 25) };
    }
    for (k, col) in [0xeafff6, 0xbff5df, 0x8ff0c8, 0x63c9a3, 0x3f9a7a].iter().enumerate() {
        v[18 + k] = *col;
    }
    v[23] = 0xd0d8d4;
    v
}

fn main_sheet() -> Canvas {
    let (w, h) = size::MAIN;
    let mut c = Canvas::new(w, h);
    {
        let b = c.bounds();
        stone(&mut c, b, STONE, 1);
    }
    // A soft vertical shade, and a carved border.
    for y in 0..h as i32 {
        c.blend_rect(r(0, y, w, 1), 0x000000, (y as u32 * 60 / h) as u8);
    }
    {
        let b = c.bounds();
        frame(&mut c, b, EDGE_LIGHT, EDGE_DARK);
    }
    // Clutter bar: a narrow recess with letters.
    lcd(&mut c, main::CLUTTER);
    for (k, ch) in "OAIDV".chars().enumerate() {
        let (dy, _) = main::CLUTTER_BUTTONS[k];
        glyph(&mut c, ch, main::CLUTTER.x + 2, main::CLUTTER.y + dy + 1, INK_DIM);
    }
    // The display: time, play state, visualizer.
    lcd(&mut c, r(20, 23, 85, 39));
    for y in (main::VIS.y..main::VIS.y + 16).step_by(2) {
        for x in (main::VIS.x..main::VIS.x + 76).step_by(2) {
            c.put(x + 1, y, LCD_GRID);
        }
    }
    // Title, bitrate and rate windows, with engraved labels.
    lcd(&mut c, r(108, 24, 159, 12));
    lcd(&mut c, r(108, 40, 21, 10));
    text(&mut c, "KBPS", 131, 43, LABEL);
    lcd(&mut c, r(153, 40, 16, 10));
    text(&mut c, "KHZ", 171, 43, LABEL);
    // Slider beds.
    for rr in [main::VOLUME, main::BALANCE] {
        c.fill_rect(r(rr.x - 1, rr.y + 3, rr.width + 2, 7), STONE_LO);
    }
    c.fill_rect(r(14, 71, 252, 12), STONE_LO);
    frame(&mut c, r(14, 71, 252, 12), EDGE_DARK, STONE_HI);
    // A faceted stone for a logo, bottom right.
    let (lx, ly) = (252, 90);
    for k in 0..7 {
        let half = if k < 4 { 3 + k } else { 9 - k };
        c.fill_rect(r(lx + 7 - half, ly + 2 * k, 2 * half as u32 + 1, 2), mix(STONE_HI, INK, 40 + 20 * k as u32));
    }
    c.fill_rect(r(lx + 4, ly + 3, 2, 2), 0xffffff);
    c
}

fn titlebar() -> Canvas {
    let (w, h) = size::TITLEBAR;
    let mut c = Canvas::new(w, h);
    {
        let b = c.bounds();
        stone(&mut c, b, STONE_LO, 2);
    }
    for (area, lit) in [(main::TITLE_ACTIVE, true), (main::TITLE_INACTIVE, false)] {
        let base = if lit { mix(STONE, INK, 30) } else { STONE };
        stone(&mut c, area, base, 3);
        frame(&mut c, area, EDGE_LIGHT, EDGE_DARK);
        // Grooves either side of the name.
        let groove = if lit { mix(INK, STONE, 120) } else { STONE_HI };
        for (x0, x1) in [(area.x + 20, area.x + 100), (area.x + 176, area.x + 238)] {
            for k in [5, 8] {
                c.fill_rect(r(x0, area.y + k, (x1 - x0) as u32, 1), groove);
                c.fill_rect(r(x0, area.y + k + 1, (x1 - x0) as u32, 1), EDGE_DARK);
            }
        }
        title(&mut c, "PHONOLITE", area.x + 138, area.y + 3, if lit { 0xe8fff4 } else { LABEL });
    }
    // Title buttons: menu, minimize, close, shade — up and down.
    type Icon = fn(&mut Canvas, i32, i32);
    let icons: [(Rect, Rect, Icon); 4] = [
        (main::MENU.1.up, main::MENU.1.down, |c, x, y| {
            c.fill_rect(r(x + 2, y + 3, 5, 1), INK);
            c.fill_rect(r(x + 3, y + 4, 3, 1), INK);
            c.put(x + 4, y + 5, INK);
        }),
        (main::MINIMIZE.1.up, main::MINIMIZE.1.down, |c, x, y| c.fill_rect(r(x + 2, y + 6, 5, 1), INK)),
        (main::CLOSE.1.up, main::CLOSE.1.down, |c, x, y| {
            for k in 0..5 {
                c.put(x + 2 + k, y + 2 + k, INK);
                c.put(x + 6 - k, y + 2 + k, INK);
            }
        }),
        (main::SHADE.1.up, main::SHADE.1.down, |c, x, y| frame(c, r(x + 2, y + 2, 5, 5), INK, INK)),
    ];
    for (up, down, icon) in icons {
        raised(&mut c, up, STEEL);
        icon(&mut c, up.x, up.y);
        pressed(&mut c, down, STEEL_DOWN);
        icon(&mut c, down.x + 1, down.y);
    }
    // Clutter bar, pressed: each letter lit on a dark key.
    for (k, ch) in "OAIDV".chars().enumerate() {
        let (dy, hh) = main::CLUTTER_BUTTONS[k];
        let rr = r(main::CLUTTER_DOWN_X + 8 * k as i32, main::CLUTTER_DOWN_Y + dy, 8, hh);
        c.fill_rect(rr, LCD);
        glyph(&mut c, ch, rr.x + 2, rr.y + 1, INK);
    }
    c
}

fn cbuttons() -> Canvas {
    let (w, h) = size::CBUTTONS;
    let mut c = Canvas::new(w, h);
    for (i, (_, s)) in main::TRANSPORT.iter().enumerate() {
        for (rr, down) in [(s.up, false), (s.down, true)] {
            if down {
                pressed(&mut c, rr, STEEL_DOWN);
            } else {
                raised(&mut c, rr, STEEL);
                c.fill_rect(r(rr.x + 1, rr.y + 1, rr.width - 2, 1), mix(EDGE_LIGHT, 0xffffff, 80));
            }
            let ink = if down { AMBER } else { INK };
            let (cx, cy) = (rr.x + rr.width as i32 / 2 + i32::from(down), rr.y + rr.height as i32 / 2 + i32::from(down));
            // A triangle with its tall edge at x0, narrowing toward x0 + 4 * dir.
            let tri = |c: &mut Canvas, x0: i32, dir: i32| {
                for k in 0..5 {
                    c.fill_rect(r(x0 + k * dir, cy - 4 + k, 1, (9 - 2 * k) as u32), ink);
                }
            };
            match i {
                0 => {
                    c.fill_rect(r(cx - 5, cy - 4, 2, 9), ink);
                    tri(&mut c, cx + 2, -1);
                }
                1 => tri(&mut c, cx - 2, 1),
                2 => {
                    c.fill_rect(r(cx - 3, cy - 4, 2, 9), ink);
                    c.fill_rect(r(cx + 1, cy - 4, 2, 9), ink);
                }
                3 => c.fill_rect(r(cx - 4, cy - 4, 8, 8), ink),
                4 => {
                    tri(&mut c, cx - 3, 1);
                    c.fill_rect(r(cx + 3, cy - 4, 2, 9), ink);
                }
                _ => {
                    for k in 0..4 {
                        c.fill_rect(r(cx - k, cy - 4 + k, 2 * k as u32 + 1, 1), ink);
                    }
                    c.fill_rect(r(cx - 4, cy + 2, 9, 2), ink);
                }
            }
        }
    }
    c
}

fn shufrep() -> Canvas {
    let (w, h) = size::SHUFREP;
    let mut c = Canvas::new(w, h);
    for (s, label) in [(main::REPEAT.1, "REP"), (main::SHUFFLE.1, "SHUFFLE"), (main::EQ_TOGGLE.1, "EQ"), (main::PL_TOGGLE.1, "PL")] {
        for (rr, on, down) in [(s.off, false, false), (s.off_down, false, true), (s.on, true, false), (s.on_down, true, true)] {
            if down {
                pressed(&mut c, rr, STEEL_DOWN);
            } else {
                raised(&mut c, rr, STEEL);
            }
            let d = i32::from(down);
            let lamp_h = rr.height - 8;
            c.fill_rect(r(rr.x + 3 + d, rr.y + 4 + d, 3, lamp_h), if on { AMBER } else { AMBER_DIM });
            let label_w = 4 * label.len() as i32 - 1;
            let x = rr.x + 7 + (rr.width as i32 - 8 - label_w) / 2;
            text(&mut c, label, x + d, rr.y + (rr.height as i32 - 5) / 2 + d, if on { 0xe8fff4 } else { LABEL });
        }
    }
    c
}

fn slider_sheet(balance: bool) -> Canvas {
    let (w, h) = size::VOLUME;
    let mut c = Canvas::new(w, h);
    c.fill(STONE_LO);
    let s = if balance { main::BALANCE_SPRITES } else { main::VOLUME_SPRITES };
    for i in 0..28u32 {
        let f = s.track.rect(i);
        let groove = r(f.x, f.y + 4, f.width, 5);
        lcd(&mut c, groove);
        let color = mix(0x1f7a5c, if balance { AMBER } else { INK }, i * 255 / 27);
        if balance {
            let half = (f.width / 2 - 1) * i / 27;
            let mid = f.x + f.width as i32 / 2;
            c.fill_rect(r(mid - half as i32 - 1, groove.y + 1, 2 * half + 2, 3), color);
        } else {
            c.fill_rect(r(f.x + 1, groove.y + 1, 1 + (f.width - 3) * i / 27, 3), color);
        }
    }
    for (rr, down) in [(s.thumb, false), (s.thumb_down, true)] {
        if down {
            pressed(&mut c, rr, mix(STEEL, AMBER, 60));
        } else {
            raised(&mut c, rr, 0x9aa8a2);
        }
        for k in [4, 6, 8] {
            c.fill_rect(r(rr.x + k, rr.y + 3, 1, 5), if down { AMBER } else { STONE_LO });
        }
    }
    c
}

fn posbar() -> Canvas {
    let (w, h) = size::POSBAR;
    let mut c = Canvas::new(w, h);
    let bed = main::POSBAR_SPRITES.track.first;
    c.fill_rect(bed, STONE_LO);
    lcd(&mut c, r(bed.x, bed.y + 3, bed.width, 4));
    for x in (bed.x + 4..bed.x + bed.width as i32 - 4).step_by(8) {
        c.put(x, bed.y + 4, INK_DIM);
    }
    for (rr, down) in [(main::POSBAR_SPRITES.thumb, false), (main::POSBAR_SPRITES.thumb_down, true)] {
        if down {
            pressed(&mut c, rr, 0xc07820);
        } else {
            raised(&mut c, rr, AMBER);
        }
        c.fill_rect(r(rr.x + 13, rr.y + 2, 3, 6), if down { 0xffe0b0 } else { 0x804000 });
    }
    c
}

fn playpaus() -> Canvas {
    let (w, h) = size::PLAYPAUS;
    let mut c = Canvas::new(w, h);
    c.fill(LCD);
    // Play.
    for k in 0..4 {
        c.fill_rect(r(3 + k, 1 + k, 1, (7 - 2 * k) as u32), INK);
    }
    // Pause.
    c.fill_rect(r(11, 1, 2, 7), INK);
    c.fill_rect(r(14, 1, 2, 7), INK);
    // Stop.
    c.fill_rect(r(20, 2, 5, 5), INK);
    // Working strip: lit and unlit.
    c.fill_rect(r(main::WORKING.x, 2, 2, 5), AMBER);
    c.fill_rect(r(main::NOT_WORKING.x, 2, 2, 5), AMBER_DIM);
    c
}

fn monoster() -> Canvas {
    let (w, h) = size::MONOSTER;
    let mut c = Canvas::new(w, h);
    for ((_, lit, unlit), label) in [(main::STEREO, "STEREO"), (main::MONO, "MONO")] {
        for (rr, on) in [(lit, true), (unlit, false)] {
            lcd(&mut c, rr);
            text_centered(&mut c, label, rr.x + rr.width as i32 / 2, rr.y + 4, if on { INK } else { INK_DIM });
        }
    }
    c
}

/// Seven-segment digits 0–9, a blank and a minus (nums_ex layout), on LCD.
fn numbers() -> Canvas {
    let mut c = Canvas::new(108, 13);
    c.fill(LCD);
    const SEGS: [u8; 12] = [0x3f, 0x06, 0x5b, 0x4f, 0x66, 0x6d, 0x7d, 0x07, 0x7f, 0x6f, 0x00, 0x40];
    for (d, segs) in SEGS.iter().enumerate() {
        let x = d as i32 * 9;
        let parts = [r(x + 2, 1, 5, 1), r(x + 7, 2, 1, 4), r(x + 7, 7, 1, 4), r(x + 2, 11, 5, 1), r(x + 1, 7, 1, 4), r(x + 1, 2, 1, 4), r(x + 2, 6, 5, 1)];
        for (k, rr) in parts.into_iter().enumerate() {
            let lit = segs >> k & 1 == 1;
            c.fill_rect(rr, if lit { INK } else { LCD_GRID });
            if lit {
                // A brighter core.
                c.fill_rect(
                    Rect::new(
                        rr.x + i32::from(rr.width > 1),
                        rr.y + i32::from(rr.height > 1),
                        rr.width.saturating_sub(2).max(1),
                        rr.height.saturating_sub(2).max(1),
                    ),
                    0xd8fff0,
                );
            }
        }
    }
    c
}

fn text_sheet() -> Canvas {
    let (w, h) = size::TEXT;
    let mut c = Canvas::new(w, h);
    c.fill(LCD);
    for (row, line) in layout::TEXT_LAYOUT.lines().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            glyph(&mut c, ch, col as i32 * 5 + 1, row as i32 * 6, INK);
        }
    }
    c
}

fn pledit() -> Canvas {
    let (w, h) = size::PLEDIT;
    let mut c = Canvas::new(w, h);
    {
        let b = c.bounds();
        stone(&mut c, b, STONE_LO, 5);
    }
    // Title bar pieces.
    for (pieces, lit) in [(pl::TOP_ACTIVE, true), (pl::TOP_INACTIVE, false)] {
        let base = if lit { mix(STONE, INK, 30) } else { STONE };
        for (k, p) in pieces.iter().enumerate() {
            stone(&mut c, *p, base, 6);
            c.fill_rect(r(p.x, p.y, p.width, 1), EDGE_LIGHT);
            c.fill_rect(r(p.x, p.y + 19, p.width, 1), EDGE_DARK);
            let groove = if lit { mix(INK, STONE, 120) } else { STONE_HI };
            match k {
                0 => c.fill_rect(r(p.x, p.y, 1, 20), EDGE_LIGHT),
                1 => title(&mut c, "PLAYLIST", p.x + 50, p.y + 5, if lit { 0xe8fff4 } else { LABEL }),
                _ => {}
            }
            if k != 1 {
                for y in [7, 10] {
                    let x0 = if k == 0 { p.x + 4 } else { p.x };
                    let x1 = if k == 3 { p.x + 2 } else { p.x + p.width as i32 };
                    if x1 > x0 {
                        c.fill_rect(r(x0, p.y + y, (x1 - x0) as u32, 1), groove);
                        c.fill_rect(r(x0, p.y + y + 1, (x1 - x0) as u32, 1), EDGE_DARK);
                    }
                }
            }
            if k == 3 {
                // Close box at the right, as on the main window.
                c.fill_rect(r(p.x + p.width as i32 - 1, p.y, 1, 20), EDGE_DARK);
                raised(&mut c, r(p.x + 14, p.y + 3, 9, 9), STEEL);
                for d in 0..5 {
                    c.put(p.x + 16 + d, p.y + 5 + d, INK);
                    c.put(p.x + 20 - d, p.y + 5 + d, INK);
                }
            }
        }
    }
    // Pressed close.
    let cd = pl::CLOSE_DOWN;
    pressed(&mut c, cd, STEEL_DOWN);
    for d in 0..5 {
        c.put(cd.x + 3 + d, cd.y + 2 + d, AMBER);
        c.put(cd.x + 7 - d, cd.y + 2 + d, AMBER);
    }
    // Side tiles.
    stone(&mut c, pl::LEFT, STONE, 7);
    c.fill_rect(r(pl::LEFT.x, pl::LEFT.y, 1, 29), EDGE_LIGHT);
    c.fill_rect(r(pl::LEFT.x + 11, pl::LEFT.y, 1, 29), EDGE_DARK);
    stone(&mut c, pl::RIGHT, STONE, 8);
    c.fill_rect(r(pl::RIGHT.x, pl::RIGHT.y, 1, 29), STONE_HI);
    c.fill_rect(r(pl::RIGHT.x + 5, pl::RIGHT.y, 10, 29), LCD);
    c.fill_rect(r(pl::RIGHT.x + 5, pl::RIGHT.y, 1, 29), EDGE_DARK);
    c.fill_rect(r(pl::RIGHT.x + 19, pl::RIGHT.y, 1, 29), EDGE_DARK);
    // Scroll thumbs.
    raised(&mut c, pl::SCROLL_THUMB, 0x9aa8a2);
    pressed(&mut c, pl::SCROLL_THUMB_DOWN, mix(STEEL, AMBER, 60));
    for t in [pl::SCROLL_THUMB, pl::SCROLL_THUMB_DOWN] {
        for k in [6, 8, 10] {
            c.fill_rect(r(t.x + 2, t.y + k, 4, 1), STONE_LO);
        }
    }
    // Bottom pieces.
    for (p, seed) in [(pl::BOTTOM_LEFT, 9), (pl::BOTTOM_RIGHT, 10), (pl::BOTTOM, 11)] {
        stone(&mut c, p, STONE, seed);
        c.fill_rect(r(p.x, p.y, p.width, 1), STONE_HI);
        c.fill_rect(r(p.x, p.y + 37, p.width, 1), EDGE_DARK);
    }
    c.fill_rect(r(pl::BOTTOM_LEFT.x, pl::BOTTOM_LEFT.y, 1, 38), EDGE_LIGHT);
    c.fill_rect(r(pl::BOTTOM_RIGHT.x + 149, pl::BOTTOM_RIGHT.y, 1, 38), EDGE_DARK);
    for (k, label) in ["ADD", "REM", "SEL", "MISC"].iter().enumerate() {
        let (x, w) = pl::MENU_BUTTONS[k];
        let rr = r(pl::BOTTOM_LEFT.x + x, pl::BOTTOM_LEFT.y + 8, w, 18);
        raised(&mut c, rr, STEEL);
        text_centered(&mut c, label, rr.x + w as i32 / 2, rr.y + 7, LABEL);
    }
    let br = pl::BOTTOM_RIGHT;
    let list = r(br.x + 150 + pl::LIST_BUTTON.0 + 1, br.y + 8, pl::LIST_BUTTON.1, 18);
    raised(&mut c, list, STEEL);
    text_centered(&mut c, "LIST", list.x + 11, list.y + 7, LABEL);
    lcd(&mut c, r(br.x + 5, br.y + 8, 92, 11));
    lcd(&mut c, r(br.x + pl::MINI_TIME.0 - 2, br.y + 21, 33, 9));
    // Mini transport icons.
    for (k, (x, w)) in pl::MINI_BUTTONS.iter().enumerate() {
        let (x0, y0) = (br.x + x, br.y + pl::MINI_Y.0 + 1);
        let cx = x0 + *w as i32 / 2;
        match k {
            0 => {
                c.fill_rect(r(x0 + 1, y0, 1, 5), LABEL);
                for d in 0..3 {
                    c.fill_rect(r(x0 + 4 - d, y0 + d, 1, (5 - 2 * d) as u32), LABEL);
                }
            }
            1 => {
                for d in 0..3 {
                    c.fill_rect(r(cx - 1 + d, y0 + d, 1, (5 - 2 * d) as u32), LABEL);
                }
            }
            2 => {
                c.fill_rect(r(cx - 2, y0, 2, 5), LABEL);
                c.fill_rect(r(cx + 1, y0, 2, 5), LABEL);
            }
            3 => c.fill_rect(r(cx - 2, y0, 5, 5), LABEL),
            4 => {
                for d in 0..3 {
                    c.fill_rect(r(x0 + 2 + d, y0 + d, 1, (5 - 2 * d) as u32), LABEL);
                }
                c.fill_rect(r(x0 + 6, y0, 1, 5), LABEL);
            }
            _ => {
                for d in 0..3 {
                    c.fill_rect(r(cx - d, y0 + d, 2 * d as u32 + 1, 1), LABEL);
                }
                c.fill_rect(r(cx - 3, y0 + 4, 7, 1), LABEL);
            }
        }
    }
    // Resize grip.
    for d in 0..4 {
        for k in 0..=d {
            c.put(br.x + 146 - 3 * k, br.y + 34 - 3 * (d - k), STONE_HI);
        }
    }
    // Menu items: two lines each, up and pressed; and the bars beside them.
    const ITEMS: [&[(&str, &str)]; 5] = [
        &[("ADD", "URL"), ("ADD", "DIR"), ("ADD", "FILE")],
        &[("REM", "ALL"), ("CROP", ""), ("REM", "SEL"), ("REM", "MISC")],
        &[("INV", "SEL"), ("SEL", "ZERO"), ("SEL", "ALL")],
        &[("SORT", "LIST"), ("FILE", "INF"), ("MISC", "OPTS")],
        &[("NEW", "LIST"), ("SAVE", "LIST"), ("LOAD", "LIST")],
    ];
    for (m, items) in ITEMS.iter().enumerate() {
        let (x_up, x_down, _) = pl::MENUS[m];
        for (i, (a, b)) in items.iter().enumerate() {
            let y = pl::MENU_ITEM_Y + 19 * i as i32;
            for (x, down) in [(x_up, false), (x_down, true)] {
                let rr = r(x, y, 22, 18);
                if down {
                    pressed(&mut c, rr, mix(STEEL_DOWN, INK, 40));
                } else {
                    raised(&mut c, rr, STEEL);
                }
                let ink = if down { 0xe8fff4 } else { LABEL };
                if b.is_empty() {
                    text_centered(&mut c, a, x + 11, y + 7, ink);
                } else {
                    text_centered(&mut c, a, x + 11, y + 3, ink);
                    text_centered(&mut c, b, x + 11, y + 10, ink);
                }
            }
        }
        let bar_h = 19 * items.len() as u32 - 1;
        c.fill_rect(r(pl::MENU_BARS[m], pl::MENU_ITEM_Y, 3, bar_h), AMBER_DIM);
        c.fill_rect(r(pl::MENU_BARS[m] + 1, pl::MENU_ITEM_Y, 1, bar_h), AMBER);
    }
    c
}

fn eqmain() -> Canvas {
    let (w, h) = size::EQMAIN;
    let mut c = Canvas::new(w, h);
    {
        let b = c.bounds();
        stone(&mut c, b, STONE_LO, 12);
    }
    // Window background.
    let bg = eq::BACKGROUND;
    stone(&mut c, bg, STONE, 13);
    for y in 0..bg.height as i32 {
        c.blend_rect(r(0, y, bg.width, 1), 0x000000, (y as u32 * 50 / bg.height) as u8);
    }
    frame(&mut c, bg, EDGE_LIGHT, EDGE_DARK);
    // The slider field: a recessed panel with dB marks.
    lcd(&mut c, r(16, 34, 250, 76));
    for (label, y) in [("+12", 38), ("0", 66), ("-12", 95)] {
        text(&mut c, label, 50 + 4 * (3 - label.len() as i32), y, LABEL);
        for x in (66..266).step_by(6) {
            if !(36..=40).contains(&x) {
                c.put(x, y + 2, INK_DIM);
            }
        }
    }
    text_centered(&mut c, "PRE", eq::PREAMP.x + 7, 103, LABEL);
    for (k, label) in ["60", "170", "310", "600", "1K", "3K", "6K", "12K", "14K", "16K"].iter().enumerate() {
        text_centered(&mut c, label, eq::BAND_X + eq::BAND_STEP * k as i32 + 7, 103, LABEL);
    }
    lcd(&mut c, r(eq::GRAPH.x - 1, eq::GRAPH.y - 1, eq::GRAPH.width + 2, eq::GRAPH.height + 2));
    // Title bars.
    for (area, lit) in [(eq::TITLE_ACTIVE, true), (eq::TITLE_INACTIVE, false)] {
        stone(&mut c, area, if lit { mix(STONE, INK, 30) } else { STONE }, 14);
        frame(&mut c, area, EDGE_LIGHT, EDGE_DARK);
        let groove = if lit { mix(INK, STONE, 120) } else { STONE_HI };
        for (x0, x1) in [(8, 92), (182, 258)] {
            for k in [5, 8] {
                c.fill_rect(r(x0, area.y + k, (x1 - x0) as u32, 1), groove);
                c.fill_rect(r(x0, area.y + k + 1, (x1 - x0) as u32, 1), EDGE_DARK);
            }
        }
        title(&mut c, "EQUALIZER", 137, area.y + 3, if lit { 0xe8fff4 } else { LABEL });
        raised(&mut c, r(264, area.y + 3, 9, 9), STEEL);
        for d in 0..5 {
            c.put(266 + d, area.y + 5 + d, INK);
            c.put(270 - d, area.y + 5 + d, INK);
        }
    }
    // Close (on its own, for the title bar's corner), up and down.
    let (_, close) = eq::CLOSE;
    raised(&mut c, close.up, STEEL);
    pressed(&mut c, close.down, STEEL_DOWN);
    for (rr, ink) in [(close.up, INK), (close.down, AMBER)] {
        for d in 0..5 {
            c.put(rr.x + 2 + d, rr.y + 2 + d, ink);
            c.put(rr.x + 6 - d, rr.y + 2 + d, ink);
        }
    }
    // ON and AUTO.
    for ((_, s), label) in [(eq::ON, "ON"), (eq::AUTO, "AUTO")] {
        for (rr, on, down) in [(s.off, false, false), (s.off_down, false, true), (s.on, true, false), (s.on_down, true, true)] {
            if down {
                pressed(&mut c, rr, STEEL_DOWN);
            } else {
                raised(&mut c, rr, STEEL);
            }
            let d = i32::from(down);
            c.fill_rect(r(rr.x + 3 + d, rr.y + 3 + d, 3, 6), if on { AMBER } else { AMBER_DIM });
            text(&mut c, label, rr.x + 9 + d, rr.y + 4 + d, if on { 0xe8fff4 } else { LABEL });
        }
    }
    // PRESETS.
    let (_, p) = eq::PRESETS;
    raised(&mut c, p.up, STEEL);
    pressed(&mut c, p.down, STEEL_DOWN);
    text_centered(&mut c, "PRESETS", p.up.x + 22, p.up.y + 4, LABEL);
    text_centered(&mut c, "PRESETS", p.down.x + 23, p.down.y + 5, 0xe8fff4);
    // Slider frames: a groove filled from the bottom, mint to amber.
    let s = eq::SLIDER_SPRITES;
    for i in 0..28u32 {
        let f = s.track.rect(i);
        c.fill_rect(f, LCD);
        let groove = r(f.x + 5, f.y + 2, 4, f.height - 4);
        frame(&mut c, groove, EDGE_DARK, STONE_HI);
        let fill = (groove.height - 2) * i / 27;
        let col = mix(0x1f7a5c, if i > 13 { AMBER } else { INK }, i * 255 / 27);
        c.fill_rect(r(groove.x + 1, groove.y + groove.height as i32 - 1 - fill as i32, 2, fill), col);
    }
    for (rr, down) in [(s.thumb, false), (s.thumb_down, true)] {
        if down {
            pressed(&mut c, rr, mix(STEEL, AMBER, 60));
        } else {
            raised(&mut c, rr, 0x9aa8a2);
        }
        c.fill_rect(r(rr.x + 2, rr.y + 5, 7, 1), if down { AMBER } else { STONE_LO });
    }
    // The curve's background, line colors and preamp line.
    let g = eq::GRAPH_BACKGROUND;
    c.fill_rect(g, LCD);
    for x in (g.x..g.x + g.width as i32).step_by(3) {
        c.put(x, g.y + 9, LCD_GRID);
    }
    for k in 0..19u32 {
        let t = if k <= 9 { 255 - k * 25 } else { (k - 9) * 25 };
        c.put(eq::GRAPH_COLORS.x, eq::GRAPH_COLORS.y + k as i32, mix(INK, AMBER, t));
    }
    c.fill_rect(eq::PREAMP_LINE, INK_DIM);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheets_have_their_classic_sizes() {
        let s = skin();
        for (sheet, (w, h)) in [
            (&s.main, size::MAIN),
            (&s.titlebar, size::TITLEBAR),
            (&s.cbuttons, size::CBUTTONS),
            (&s.shufrep, size::SHUFREP),
            (&s.volume, size::VOLUME),
            (&s.balance, size::VOLUME),
            (&s.posbar, size::POSBAR),
            (&s.playpaus, size::PLAYPAUS),
            (&s.monoster, size::MONOSTER),
            (&s.text, size::TEXT),
            (&s.pledit, size::PLEDIT),
            (&s.eqmain, size::EQMAIN),
        ] {
            assert_eq!((sheet.width(), sheet.height()), (w, h));
        }
        assert_eq!((s.numbers.width(), s.numbers.height()), (108, 13));
    }

    #[test]
    fn every_text_cell_has_a_glyph() {
        for ch in layout::TEXT_LAYOUT.chars().filter(|c| !matches!(c, '\n' | '\0' | ' ')) {
            assert!(GLYPHS.iter().any(|(g, _)| *g == ch), "{ch:?}");
        }
    }

    /// The titles read as separate letters: scanning across the title, the
    /// columns holding text form one run per letter, with clear gaps.
    #[test]
    fn titles_are_separate_letters() {
        let s = skin();
        let ink = 0xe8fff4;
        let runs = |sheet: &Canvas, area: Rect| {
            let lit: Vec<bool> = (0..area.width).map(|x| (0..area.height).any(|y| sheet.get(area.x as u32 + x, area.y as u32 + y) == Some(ink))).collect();
            lit.windows(2).filter(|w| w[1] && !w[0]).count() + usize::from(lit[0])
        };
        assert_eq!(runs(&s.titlebar, main::TITLE_ACTIVE), "PHONOLITE".len());
        assert_eq!(runs(&s.eqmain, eq::TITLE_ACTIVE), "EQUALIZER".len());
        assert_eq!(runs(&s.pledit, pl::TOP_ACTIVE[1]), "PLAYLIST".len());
        // 7 rows tall.
        let rows = (0..14).filter(|&y| (0..275).any(|x| s.titlebar.get(27 + x, y) == Some(ink))).count();
        assert_eq!(rows, 7);
        assert_eq!(title_width("I"), 3);
        assert_eq!(title_width("MI"), 5 + TITLE_GAP + 3);
    }

    #[test]
    fn states_differ() {
        // Pressed and unpressed sprites must be distinguishable, or a skin
        // gives no feedback.
        let s = skin();
        let differ = |sheet: &Canvas, a: Rect, b: Rect| {
            (0..a.height).any(|y| (0..a.width).any(|x| sheet.get(a.x as u32 + x, a.y as u32 + y) != sheet.get(b.x as u32 + x, b.y as u32 + y)))
        };
        for (_, sp) in main::TRANSPORT {
            assert!(differ(&s.cbuttons, sp.up, sp.down));
        }
        for (_, t) in [main::SHUFFLE, main::REPEAT, main::EQ_TOGGLE, main::PL_TOGGLE] {
            assert!(differ(&s.shufrep, t.off, t.on) && differ(&s.shufrep, t.off, t.off_down) && differ(&s.shufrep, t.on, t.on_down));
        }
        assert!(differ(&s.playpaus, main::PLAYING, main::PAUSED) && differ(&s.playpaus, main::PAUSED, main::STOPPED));
        assert!(differ(&s.monoster, main::STEREO.1, main::STEREO.2));
        assert!(differ(&s.titlebar, main::TITLE_ACTIVE, main::TITLE_INACTIVE));
        let f = eq::SLIDER_SPRITES.track;
        assert!(differ(&s.eqmain, f.rect(0), f.rect(27)));
    }
}
