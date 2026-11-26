//! Settings kept between runs: `phonolite.conf` (`key=value` lines) and
//! the playlist as `playlist.m3u`, in `$XDG_CONFIG_HOME/phonolite` (or
//! `~/.config/phonolite`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::eq::Equalizer;
use crate::num::{fixed, parse_decimal};
use crate::vis::Mode;

/// Everything remembered.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Volume slider, 0..=100.
    pub volume: u32,
    /// Balance, -100..=100.
    pub balance: i32,
    /// Shuffle.
    pub shuffle: bool,
    /// Repeat.
    pub repeat: bool,
    /// Equalizer curve.
    pub eq: Equalizer,
    /// The EQ window's AUTO: keep the curve on between runs (off: start flat).
    pub eq_auto: bool,
    /// Equalizer window shown.
    pub show_eq: bool,
    /// Playlist window shown.
    pub show_pl: bool,
    /// Playlist window height.
    pub pl_height: u32,
    /// Double size.
    pub double: bool,
    /// Always on top.
    pub on_top: bool,
    /// Visualizer.
    pub vis: Mode,
    /// Remaining time instead of elapsed.
    pub remaining: bool,
    /// Skin file or folder; none for built-in.
    pub skin: Option<String>,
    /// Where the file browser was last.
    pub last_dir: Option<String>,
    /// The current playlist entry.
    pub current: Option<usize>,
    /// Where the main, equalizer and playlist windows were on the screen.
    pub positions: [Option<(i32, i32)>; 3],
}

impl Default for Config {
    fn default() -> Config {
        Config {
            volume: 75,
            balance: 0,
            shuffle: false,
            repeat: false,
            eq: Equalizer::default(),
            eq_auto: true,
            show_eq: true,
            show_pl: true,
            pl_height: crate::skin::layout::pl::DEFAULT_HEIGHT,
            double: false,
            on_top: false,
            vis: Mode::Spectrum,
            remaining: false,
            skin: None,
            last_dir: None,
            current: None,
            positions: [None; 3],
        }
    }
}

/// The settings folder.
pub fn dir() -> Option<PathBuf> {
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|x| !x.is_empty()) {
        return Some(PathBuf::from(x).join("phonolite"));
    }
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(|h| PathBuf::from(h).join(".config/phonolite"))
}

const POSITION_KEYS: [&str; 3] = ["main_pos", "eq_pos", "pl_pos"];

fn flag(b: bool) -> &'static str {
    if b {
        "1"
    } else {
        "0"
    }
}

impl Config {
    /// As text.
    pub fn to_text(&self) -> String {
        let mut s = String::from("# phonolite settings\n");
        let _ = writeln!(s, "volume={}", self.volume);
        let _ = writeln!(s, "balance={}", self.balance);
        let _ = writeln!(s, "shuffle={}", flag(self.shuffle));
        let _ = writeln!(s, "repeat={}", flag(self.repeat));
        let _ = writeln!(s, "eq_on={}", flag(self.eq.on));
        let _ = writeln!(s, "eq_preamp={}", fixed(self.eq.preamp, 2));
        let _ = writeln!(s, "eq_bands={}", self.eq.bands.iter().map(|b| fixed(*b, 2)).collect::<Vec<_>>().join(","));
        let _ = writeln!(s, "eq_auto={}", flag(self.eq_auto));
        let _ = writeln!(s, "show_eq={}", flag(self.show_eq));
        let _ = writeln!(s, "show_pl={}", flag(self.show_pl));
        let _ = writeln!(s, "pl_height={}", self.pl_height);
        let _ = writeln!(s, "double={}", flag(self.double));
        let _ = writeln!(s, "on_top={}", flag(self.on_top));
        let _ = writeln!(
            s,
            "vis={}",
            match self.vis {
                Mode::Spectrum => "spectrum",
                Mode::Scope => "scope",
                Mode::Off => "off",
            }
        );
        let _ = writeln!(s, "remaining={}", flag(self.remaining));
        if let Some(k) = &self.skin {
            let _ = writeln!(s, "skin={k}");
        }
        if let Some(d) = &self.last_dir {
            let _ = writeln!(s, "last_dir={d}");
        }
        if let Some(c) = self.current {
            let _ = writeln!(s, "current={c}");
        }
        for (key, pos) in POSITION_KEYS.iter().zip(self.positions) {
            if let Some((x, y)) = pos {
                let _ = writeln!(s, "{key}={x},{y}");
            }
        }
        s
    }

    /// From text; unknown keys and bad values are ignored.
    pub fn from_text(text: &str) -> Config {
        let mut c = Config::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let b = v == "1" || v.eq_ignore_ascii_case("true");
            match k {
                "volume" => c.volume = v.parse::<u32>().map_or(c.volume, |x| x.min(100)),
                "balance" => c.balance = v.parse::<i32>().map_or(c.balance, |x| x.clamp(-100, 100)),
                "shuffle" => c.shuffle = b,
                "repeat" => c.repeat = b,
                "eq_on" => c.eq.on = b,
                "eq_preamp" => c.eq.preamp = parse_decimal(v).map_or(c.eq.preamp, |x| x.clamp(-12.0, 12.0)),
                "eq_bands" => {
                    let vals: Vec<f64> = v.split(',').filter_map(parse_decimal).collect();
                    if vals.len() == 10 {
                        for (slot, x) in c.eq.bands.iter_mut().zip(vals) {
                            *slot = x.clamp(-12.0, 12.0);
                        }
                    }
                }
                "eq_auto" => c.eq_auto = b,
                "show_eq" => c.show_eq = b,
                "show_pl" => c.show_pl = b,
                "pl_height" => c.pl_height = v.parse().unwrap_or(c.pl_height),
                "double" => c.double = b,
                "on_top" => c.on_top = b,
                "vis" => {
                    c.vis = match v {
                        "scope" => Mode::Scope,
                        "off" => Mode::Off,
                        _ => Mode::Spectrum,
                    }
                }
                "remaining" => c.remaining = b,
                "skin" if !v.is_empty() => c.skin = Some(v.to_string()),
                "last_dir" if !v.is_empty() => c.last_dir = Some(v.to_string()),
                "current" => c.current = v.parse().ok(),
                _ if POSITION_KEYS.contains(&k) => {
                    let i = POSITION_KEYS.iter().position(|p| *p == k).expect("contained");
                    c.positions[i] = v.split_once(',').and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)));
                }
                _ => {}
            }
        }
        c
    }

    /// Read `phonolite.conf` from `dir` (defaults if absent).
    pub fn load(dir: &Path) -> Config {
        std::fs::read_to_string(dir.join("phonolite.conf")).map(|t| Config::from_text(&t)).unwrap_or_default()
    }

    /// Write `phonolite.conf` to `dir`, creating it.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join("phonolite.conf.new");
        std::fs::write(&tmp, self.to_text())?;
        std::fs::rename(tmp, dir.join("phonolite.conf"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let c = Config {
            volume: 42,
            balance: -30,
            shuffle: true,
            repeat: true,
            eq: Equalizer { on: true, preamp: -3.5, bands: [1.0, -2.0, 3.0, 0.0, 0.5, 12.0, -12.0, 4.0, 2.0, 1.0] },
            eq_auto: false,
            show_eq: false,
            show_pl: true,
            pl_height: 290,
            double: true,
            on_top: true,
            vis: Mode::Scope,
            remaining: true,
            skin: Some("/skins/x y.wsz".into()),
            last_dir: Some("/music".into()),
            current: Some(7),
            positions: [Some((10, -20)), None, Some((300, 400))],
        };
        assert_eq!(Config::from_text(&c.to_text()), c);
        assert_eq!(Config::from_text(""), Config::default());
        let odd = Config::from_text("volume=900\nbalance=x\neq_bands=1,2\nnonsense\nvis=weird\neq_pos=1,x\n");
        assert_eq!((odd.volume, odd.balance, odd.eq.bands, odd.vis), (100, 0, [0.0; 10], Mode::Spectrum));
        assert_eq!(odd.positions, [None; 3]);
    }

    #[test]
    fn save_and_load() {
        let d = std::env::temp_dir().join(format!("phonolite-conf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(Config::load(&d), Config::default());
        let c = Config { volume: 3, ..Config::default() };
        c.save(&d.join("sub")).unwrap();
        assert_eq!(Config::load(&d.join("sub")), c);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
