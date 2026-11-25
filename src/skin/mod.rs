//! Skins: the classic 2.x format (a `.wsz` ZIP, or the same files in a
//! folder) and phonolite's own built-in one, generated in that format.
//!
//! A skin that lacks a file falls back to the built-in skin's version of
//! it — except `balance.bmp`, which falls back to the skin's own
//! `volume.bmp`, as the format intends.

pub mod builtin;
pub mod layout;

use std::path::Path;

use protolith::canvas::{bmp, Canvas};

use crate::zip::Archive;

/// Playlist colors, from `pledit.txt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaylistColors {
    /// Track text.
    pub normal: u32,
    /// The current track's text.
    pub current: u32,
    /// Background.
    pub normal_bg: u32,
    /// Selected rows' background.
    pub selected_bg: u32,
}

/// A loaded skin: every sheet, decoded.
#[derive(Clone)]
pub struct Skin {
    /// Where it came from ("built-in", or a file name).
    pub name: String,
    /// main.bmp.
    pub main: Canvas,
    /// titlebar.bmp.
    pub titlebar: Canvas,
    /// cbuttons.bmp.
    pub cbuttons: Canvas,
    /// shufrep.bmp.
    pub shufrep: Canvas,
    /// volume.bmp.
    pub volume: Canvas,
    /// balance.bmp.
    pub balance: Canvas,
    /// posbar.bmp.
    pub posbar: Canvas,
    /// playpaus.bmp.
    pub playpaus: Canvas,
    /// monoster.bmp.
    pub monoster: Canvas,
    /// nums_ex.bmp if present (it has a minus), else numbers.bmp.
    pub numbers: Canvas,
    /// `numbers` is nums_ex.bmp.
    pub numbers_ex: bool,
    /// text.bmp.
    pub text: Canvas,
    /// pledit.bmp.
    pub pledit: Canvas,
    /// eqmain.bmp.
    pub eqmain: Canvas,
    /// viscolor.txt: background, grid dots, 16 spectrum colors top to
    /// bottom, 5 oscilloscope colors, peak dots.
    pub viscolor: [u32; 24],
    /// pledit.txt.
    pub playlist: PlaylistColors,
    /// region.txt's `[Normal]` polygons: the main window's shape.
    pub region: Option<Vec<Vec<(i32, i32)>>>,
    /// Files that were missing or unreadable and came from the built-in skin.
    pub fallbacks: Vec<String>,
}

enum Files<'a> {
    Dir(Vec<std::path::PathBuf>),
    Zip(Archive<'a>),
}

impl Files<'_> {
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        match self {
            Files::Dir(paths) => {
                let p = paths.iter().find(|p| p.file_name().is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(name)))?;
                std::fs::read(p).ok()
            }
            Files::Zip(z) => z.read(z.find(name)?).ok(),
        }
    }
}

/// Load a skin from a `.wsz`/`.zip` file or a folder of its files.
pub fn load(path: &Path) -> Result<Skin, String> {
    let name = path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned());
    if path.is_dir() {
        let paths = collect_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(from_files(&Files::Dir(paths), name));
    }
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let zip = Archive::parse(&data).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(from_files(&Files::Zip(zip), name))
}

/// A folder's files, and those one level down (skins unpacked with their
/// own folder inside).
fn collect_dir(dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            if let Ok(inner) = std::fs::read_dir(&p) {
                out.extend(inner.filter_map(|e| e.ok().map(|e| e.path())));
            }
        } else {
            out.push(p);
        }
    }
    // Files at the top win over ones in subfolders.
    out.sort_by_key(|p| p.components().count());
    Ok(out)
}

fn from_files(files: &Files<'_>, name: String) -> Skin {
    let base = builtin::skin();
    let mut fallbacks = Vec::new();
    let mut sheet = |file: &str, fallback: &Canvas| -> Canvas {
        match files.read(file).map(|d| bmp::decode(&d)) {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                fallbacks.push(format!("{file} ({e})"));
                fallback.clone()
            }
            None => {
                fallbacks.push(file.to_string());
                fallback.clone()
            }
        }
    };
    let volume = sheet("volume.bmp", &base.volume);
    let (numbers, numbers_ex) = match files.read("nums_ex.bmp").map(|d| bmp::decode(&d)) {
        Some(Ok(c)) => (c, true),
        _ => (sheet("numbers.bmp", &base.numbers), false),
    };
    let mut skin = Skin {
        name,
        main: sheet("main.bmp", &base.main),
        titlebar: sheet("titlebar.bmp", &base.titlebar),
        cbuttons: sheet("cbuttons.bmp", &base.cbuttons),
        shufrep: sheet("shufrep.bmp", &base.shufrep),
        balance: match files.read("balance.bmp").map(|d| bmp::decode(&d)) {
            Some(Ok(c)) => c,
            _ => volume.clone(),
        },
        volume,
        posbar: sheet("posbar.bmp", &base.posbar),
        playpaus: sheet("playpaus.bmp", &base.playpaus),
        monoster: sheet("monoster.bmp", &base.monoster),
        numbers,
        numbers_ex,
        text: sheet("text.bmp", &base.text),
        pledit: sheet("pledit.bmp", &base.pledit),
        eqmain: sheet("eqmain.bmp", &base.eqmain),
        viscolor: base.viscolor,
        playlist: base.playlist,
        region: None,
        fallbacks: Vec::new(),
    };
    match files.read("viscolor.txt") {
        Some(t) => skin.viscolor = parse_viscolor(&String::from_utf8_lossy(&t), &base.viscolor),
        None => fallbacks.push("viscolor.txt".into()),
    }
    match files.read("pledit.txt") {
        Some(t) => skin.playlist = parse_pledit(&String::from_utf8_lossy(&t), base.playlist),
        None => fallbacks.push("pledit.txt".into()),
    }
    if let Some(t) = files.read("region.txt") {
        skin.region = parse_region(&String::from_utf8_lossy(&t), "Normal");
    }
    skin.fallbacks = fallbacks;
    skin
}

/// Parse `viscolor.txt`: one `r,g,b` per line (anything after the third
/// number ignored), 24 lines; missing or bad lines keep `defaults`.
pub fn parse_viscolor(text: &str, defaults: &[u32; 24]) -> [u32; 24] {
    let mut out = *defaults;
    let mut i = 0;
    for line in text.lines() {
        if i == 24 {
            break;
        }
        let nums: Vec<u32> = line.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).take(3).filter_map(|s| s.parse().ok()).collect();
        // Comments-only lines ("// color 0 = black" carries a digit!) are
        // recognized by not starting with a number.
        if nums.len() < 3 || !line.trim_start().starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        out[i] = (nums[0].min(255) << 16) | (nums[1].min(255) << 8) | nums[2].min(255);
        i += 1;
    }
    out
}

/// Parse `pledit.txt`'s `[Text]` colors (`#RRGGBB`, `#` optional).
pub fn parse_pledit(text: &str, defaults: PlaylistColors) -> PlaylistColors {
    let mut c = defaults;
    for (key, value) in ini_section(text, "Text") {
        let hex = value.trim().trim_start_matches('#');
        let Some(v) = hex.get(..6).and_then(|h| u32::from_str_radix(h, 16).ok()) else { continue };
        match key.to_ascii_lowercase().as_str() {
            "normal" => c.normal = v,
            "current" => c.current = v,
            "normalbg" => c.normal_bg = v,
            "selectedbg" => c.selected_bg = v,
            _ => {}
        }
    }
    c
}

/// Parse a `region.txt` section: `NumPoints` (a list of polygon sizes) and
/// `PointList` (x,y pairs, comma or space separated). `None` if the
/// section is absent or inconsistent.
pub fn parse_region(text: &str, section: &str) -> Option<Vec<Vec<(i32, i32)>>> {
    let entries = ini_section(text, section);
    let get = |k: &str| entries.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone());
    let numbers =
        |s: String| s.split(|c: char| c == ',' || c.is_whitespace()).filter(|t| !t.is_empty()).map(|t| t.parse::<i32>().ok()).collect::<Option<Vec<i32>>>();
    let counts = numbers(get("NumPoints")?)?;
    let points = numbers(get("PointList")?)?;
    if points.len() % 2 != 0 || counts.iter().any(|&c| c < 3) || counts.iter().map(|&c| c as usize * 2).sum::<usize>() != points.len() {
        return None;
    }
    let mut out = Vec::new();
    let mut it = points.chunks(2).map(|p| (p[0], p[1]));
    for c in counts {
        out.push(it.by_ref().take(c as usize).collect());
    }
    Some(out)
}

/// The `key=value` lines of an INI section (case-insensitive name; `;`
/// comments).
fn ini_section(text: &str, section: &str) -> Vec<(String, String)> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with(';') || line.starts_with("//") || line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            // Only the first section of a name counts (Winamp's rule).
            if inside {
                break;
            }
            inside = name.trim().eq_ignore_ascii_case(section);
            continue;
        }
        if inside {
            if let Some((k, v)) = line.split_once('=') {
                out.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
    }
    out
}

/// Rasterize polygons into a `width`×`height` mask: a pixel is in when its
/// center is inside any polygon (even-odd within each), the way the
/// format's "outside pixel coordinates" mean it.
pub fn region_mask(polygons: &[Vec<(i32, i32)>], width: u32, height: u32) -> Vec<bool> {
    let mut mask = vec![false; (width * height) as usize];
    for poly in polygons {
        for y in 0..height {
            let cy = f64::from(y) + 0.5;
            let mut xs: Vec<f64> = Vec::new();
            for i in 0..poly.len() {
                let (x0, y0) = (f64::from(poly[i].0), f64::from(poly[i].1));
                let (x1, y1) = (f64::from(poly[(i + 1) % poly.len()].0), f64::from(poly[(i + 1) % poly.len()].1));
                if (y0 <= cy) != (y1 <= cy) {
                    xs.push(x0 + (cy - y0) / (y1 - y0) * (x1 - x0));
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
            for pair in xs.chunks(2) {
                let [a, b] = pair else { continue };
                for x in 0..width {
                    let cx = f64::from(x) + 0.5;
                    if cx >= *a && cx < *b {
                        mask[(y * width + x) as usize] = true;
                    }
                }
            }
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip;

    /// A 24-bit BMP of one color (for skin-loading tests).
    pub(crate) fn solid_bmp(w: u32, h: u32, rgb: u32) -> Vec<u8> {
        let row = (w * 3).div_ceil(4) * 4;
        let mut d = b"BM".to_vec();
        d.extend((54 + row * h).to_le_bytes());
        d.extend([0; 4]);
        d.extend(54u32.to_le_bytes());
        d.extend(40u32.to_le_bytes());
        d.extend(w.to_le_bytes());
        d.extend(h.to_le_bytes());
        d.extend(1u16.to_le_bytes());
        d.extend(24u16.to_le_bytes());
        d.extend([0; 24]);
        for _ in 0..h {
            for _ in 0..w {
                d.extend([rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8]);
            }
            d.extend(std::iter::repeat(0).take((row - w * 3) as usize));
        }
        d
    }

    #[test]
    fn viscolor_lines() {
        // In the style of real files: comments after the numbers, odd spacing.
        let text = "0,0,0, // color 0 = black\n24,33,41, // grey\n\n// a comment line with 3 4 5 numbers\n150, 150, 150, // peak\n300,1,2\nbad\n";
        let d = [0x111111; 24];
        let c = parse_viscolor(text, &d);
        assert_eq!(&c[..4], &[0, 0x182129, 0x969696, 0xff0102]);
        assert_eq!(c[4], 0x111111, "missing lines keep the default");
    }

    #[test]
    fn pledit_colors() {
        let text = "[Text]\nNormal=#00FF00\nCurrent=FFFFFF\nNormalBG=#000000\nSelectedBG=#0000C6\nFont=Arial\n[Other]\nNormal=#123456\n";
        let d = PlaylistColors { normal: 1, current: 2, normal_bg: 3, selected_bg: 4 };
        assert_eq!(parse_pledit(text, d), PlaylistColors { normal: 0x00ff00, current: 0xffffff, normal_bg: 0, selected_bg: 0xc6 });
        assert_eq!(parse_pledit("Normal=#00FF00", d), d, "outside the section");
    }

    #[test]
    fn region_polygons_and_mask() {
        let text = "; comment\n[Normal]\nNumPoints=4, 4\nPointList=0,0, 4,0, 4,2, 0,2,  1,2 3,2 3,4 1,4\n[Normal]\nNumPoints=3\nPointList=0,0,1,1,0,1\n";
        let p = parse_region(text, "normal").unwrap();
        assert_eq!(p, vec![vec![(0, 0), (4, 0), (4, 2), (0, 2)], vec![(1, 2), (3, 2), (3, 4), (1, 4)]]);
        let m = region_mask(&p, 5, 4);
        let rows: Vec<String> = m.chunks(5).map(|r| r.iter().map(|&b| if b { '#' } else { '.' }).collect()).collect();
        assert_eq!(rows, ["####.", "####.", ".##..", ".##.."]);
        assert_eq!(parse_region(text, "WindowShade"), None);
        assert_eq!(parse_region("[Normal]\nNumPoints=4\nPointList=0,0,1,0,1,1\n", "Normal"), None, "too few points");
        assert_eq!(parse_region("[Normal]\nNumPoints=2\nPointList=0,0,1,0\n", "Normal"), None, "not a polygon");
        // A triangle: diagonal edge, pixel centers decide.
        let m = region_mask(&[vec![(0, 0), (4, 0), (0, 4)]], 4, 4);
        let rows: Vec<String> = m.chunks(4).map(|r| r.iter().map(|&b| if b { '#' } else { '.' }).collect()).collect();
        assert_eq!(rows, ["###.", "##..", "#...", "...."]);
    }

    #[test]
    fn loads_a_zip_with_fallbacks_and_nums_ex() {
        let main = solid_bmp(275, 116, 0x123456);
        let nums = solid_bmp(108, 13, 0x00ff00);
        let vol = solid_bmp(68, 433, 0x445566);
        let z = zip::store(&[
            ("Skin/MAIN.BMP", &main),
            ("Skin/nums_ex.bmp", &nums),
            ("Skin/Volume.bmp", &vol),
            ("Skin/text.bmp", b"garbage"),
            ("Skin/viscolor.txt", b"1,2,3\n"),
            ("Skin/region.txt", b"[Normal]\nNumPoints=4\nPointList=0,0,275,0,275,116,0,116\n"),
        ]);
        let dir = std::env::temp_dir().join(format!("phonolite-skin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Test Skin.wsz");
        std::fs::write(&path, z).unwrap();
        let s = load(&path).unwrap();
        assert_eq!(s.name, "Test Skin");
        assert_eq!(s.main.get(10, 10), Some(0x123456));
        assert!(s.numbers_ex);
        assert_eq!(s.numbers.width(), 108);
        assert_eq!(s.balance.get(0, 0), Some(0x445566), "balance falls back to the skin's own volume");
        let builtin = builtin::skin();
        assert_eq!(s.cbuttons.pixels(), builtin.cbuttons.pixels(), "missing: built-in");
        assert_eq!(s.text.pixels(), builtin.text.pixels(), "undecodable: built-in");
        assert!(s.fallbacks.iter().any(|f| f.starts_with("text.bmp (")), "{:?}", s.fallbacks);
        assert!(s.fallbacks.contains(&"cbuttons.bmp".to_string()));
        assert!(s.fallbacks.contains(&"pledit.txt".to_string()));
        assert!(!s.fallbacks.iter().any(|f| f.starts_with("balance")), "not a fallback to built-in");
        assert_eq!(s.viscolor[0], 0x010203);
        assert_eq!(s.viscolor[1], builtin.viscolor[1]);
        assert_eq!(s.region.as_ref().unwrap()[0].len(), 4);

        // The same files in a folder (with a nested folder, as unzipped).
        let folder = dir.join("unpacked");
        std::fs::create_dir_all(folder.join("inner")).unwrap();
        std::fs::write(folder.join("inner/main.bmp"), &main).unwrap();
        let s = load(&folder).unwrap();
        assert_eq!(s.main.get(0, 0), Some(0x123456));
        assert!(!s.numbers_ex);
        assert!(load(&dir.join("missing.wsz")).is_err());
        std::fs::write(dir.join("not.wsz"), b"nope").unwrap();
        assert!(load(&dir.join("not.wsz")).err().unwrap().contains("not a ZIP"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
