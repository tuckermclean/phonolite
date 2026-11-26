//! The whole application against a real X server: each test starts its
//! own Xvfb (`-displayfd`), runs phonolite in-process on it with real
//! mpg123 and out123 (`-o sleep`: real-time output into the void), and
//! checks what reached the server by reading the window back.
//!
//! Skips without Xvfb or mpg123 unless `PHONOLITE_REQUIRE_XVFB` /
//! `PHONOLITE_REQUIRE_MPG123` are set (as in CI).

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use phonolite::app::{App, Options};
use phonolite::engine::Mpg123;
use phonolite::player::{Decoding, SinkCommand, Status};
use phonolite::testsignal;
use protolith::x11::{Connection, Event, Modifiers, MouseButton};
use protolith::Rect;

struct Xvfb {
    child: Child,
    display: String,
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn xvfb() -> Option<Xvfb> {
    let have_mpg123 = Command::new("mpg123").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    if !have_mpg123 {
        assert!(std::env::var_os("PHONOLITE_REQUIRE_MPG123").is_none(), "mpg123 is required");
        eprintln!("skipping: no mpg123");
        return None;
    }
    let spawned =
        Command::new("Xvfb").args(["-displayfd", "1", "-screen", "0", "800x1000x24", "-nolisten", "tcp"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            assert!(std::env::var_os("PHONOLITE_REQUIRE_XVFB").is_none(), "Xvfb is required: {e}");
            eprintln!("skipping: no Xvfb ({e})");
            return None;
        }
    };
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).expect("Xvfb display number");
    Some(Xvfb { child, display: format!(":{}", line.trim()) })
}

fn music(name: &str, n: usize, seconds: f64) -> Vec<PathBuf> {
    let d = std::env::temp_dir().join(format!("phonolite-xvfb-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    (0..n)
        .map(|i| {
            let p = d.join(format!("{i:02}.mp3"));
            let mut data = testsignal::id3v2(&[("TIT2", &format!("Song {i}")), ("TPE1", "Tester")]);
            data.extend(testsignal::tone(2 + 3 * i as u8, 0.8, seconds));
            std::fs::write(&p, data).unwrap();
            p
        })
        .collect()
}

fn app(x: &Xvfb, files: Vec<PathBuf>) -> App {
    let conn = Connection::open_display(&x.display, None).expect("connect");
    let opts = Options {
        skin: None,
        decoding: Decoding::Mpg123(Mpg123::default()),
        outputs: vec![phonolite::player::Output::Command(SinkCommand::out123(&["-o".into(), "sleep".into()], 44100))],
        fps: 30,
        files,
        config_dir: None,
    };
    App::new(conn, opts).expect("app")
}

fn run_until(app: &mut App, what: &str, mut f: impl FnMut(&mut App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !f(app) {
        assert!(Instant::now() < deadline, "timed out: {what}");
        app.step().expect("step");
    }
}

/// The window's pixels on the server equal the frame phonolite last
/// drew (scaled): every incremental update landed where it belongs.
fn assert_on_screen(app: &mut App) {
    let s = app.scale_factor();
    let f = app.frame().clone();
    // And the frame is what drawing everything afresh gives: what was
    // skipped as unchanged really was. Only what moves with the clock may
    // differ between the two draws: the time, the seek bar, the
    // visualizer, the title, and the playlist's mini clock.
    let fresh = app.render_all();
    let pl_bottom = f.height() as i32;
    let moving =
        [Rect::new(36, 26, 63, 13), Rect::new(16, 72, 248, 10), Rect::new(24, 43, 76, 16), Rect::new(111, 27, 154, 6), Rect::new(189, pl_bottom - 17, 33, 9)];
    let stale: Vec<(u32, u32)> = (0..f.height())
        .flat_map(|y| (0..f.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| fresh.get(x, y) != f.get(x, y) && !moving.iter().any(|r| r.contains(x as i32, y as i32)))
        .collect();
    assert!(stale.is_empty(), "stale pixels where nothing moves: {:?}", &stale[..stale.len().min(10)]);
    let got = app.conn.read_pixels(app.win, Rect::new(0, 0, f.width() * s, f.height() * s)).expect("read back");
    for y in 0..f.height() * s {
        for x in 0..f.width() * s {
            let want = f.get(x / s, y / s).unwrap();
            let have = got[(y * f.width() * s + x) as usize] & 0xffffff;
            assert_eq!(have, want, "pixel ({x}, {y}) at scale {s}");
        }
    }
}

fn click(app: &mut App, x: i32, y: i32) {
    let s = app.scale_factor() as i32;
    let (x, y) = (x * s + s / 2, y * s + s / 2);
    for pressed in [true, false] {
        app.handle(Event::Button {
            window: app.win,
            button: MouseButton::Left,
            pressed,
            x,
            y,
            root_x: x,
            root_y: y,
            modifiers: Modifiers::default(),
            time: 5000,
        });
    }
}

#[test]
fn plays_and_the_screen_keeps_up() {
    let Some(x) = xvfb() else { return };
    let mut a = app(&x, music("play", 2, 1.2));
    run_until(&mut a, "playing a while", |a| a.player.status() == Status::Playing && a.player.position() > 0.6);
    assert!(a.marquee().contains("Tester - Song 0"), "{}", a.marquee());
    // Dozens of incremental frames (clock, visualizer, title) went out.
    assert_on_screen(&mut a);
    // The next button (classic place: 108,88).
    click(&mut a, 115, 95);
    run_until(&mut a, "the second track", |a| a.marquee().contains("Song 1"));
    a.step().unwrap();
    assert_on_screen(&mut a);
    // Double size: the window doubles and the picture scales.
    let ctrl = Modifiers { ctrl: true, ..Modifiers::default() };
    let key = protolith::x11::Key::Character('d');
    a.handle(Event::Key { window: a.win, key, keysym: 0x64, keycode: 0, pressed: true, repeat: false, modifiers: ctrl });
    a.step().unwrap();
    assert_eq!(a.scale_factor(), 2);
    a.step().unwrap();
    assert_on_screen(&mut a);
    // Played out: the playlist ends, stopped.
    run_until(&mut a, "the end", |a| a.player.status() == Status::Stopped);
}

#[test]
fn hiding_panes_shrinks_the_window() {
    let Some(x) = xvfb() else { return };
    let mut a = app(&x, Vec::new());
    a.step().ok();
    let (_, h) = a.conn.window_size(a.win).unwrap();
    assert_eq!(h, 116 + 116 + 232);
    // The equalizer toggle (219,58), then the playlist's (242,58).
    click(&mut a, 225, 62);
    click(&mut a, 248, 62);
    a.step().ok();
    let deadline = Instant::now() + Duration::from_secs(5);
    while a.conn.window_size(a.win).unwrap().1 != 116 {
        assert!(Instant::now() < deadline, "window did not shrink");
        a.step().ok();
    }
    assert_on_screen(&mut a);
}

#[test]
fn a_run_of_bad_files_stops_instead_of_spinning() {
    let Some(x) = xvfb() else { return };
    // Nothing listens on port 9 here: every load fails at once.
    let urls: Vec<PathBuf> = (0..12).map(|i| PathBuf::from(format!("http://127.0.0.1:9/{i}.mp3"))).collect();
    let mut a = app(&x, urls);
    run_until(&mut a, "giving up", |a| a.marquee().contains("too many errors"));
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        a.step().unwrap();
        assert_eq!(a.player.status(), Status::Stopped, "and it stays stopped");
    }
    assert!(a.playlist.current().is_some_and(|c| c < 8), "gave up early: {:?}", a.playlist.current());
}

#[test]
fn region_txt_shapes_the_window() {
    let Some(x) = xvfb() else { return };
    let Ok(out) = Command::new("xwininfo").arg("-version").output() else {
        eprintln!("skipping: no xwininfo");
        return;
    };
    drop(out);
    let dir = std::env::temp_dir().join(format!("phonolite-xvfb-{}-region", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A skin of nothing but a region: a 200x100 triangle.
    std::fs::write(dir.join("region.txt"), "[Normal]\nNumPoints=3\nPointList=0,0, 200,0, 0,100\n").unwrap();
    let conn = Connection::open_display(&x.display, None).unwrap();
    let opts = Options {
        skin: Some(dir.clone()),
        decoding: Decoding::Mpg123(Mpg123::default()),
        outputs: vec![phonolite::player::Output::Command(SinkCommand::out123(&[], 44100))],
        fps: 30,
        files: Vec::new(),
        config_dir: None,
    };
    let mut a = App::new(conn, opts).unwrap();
    // Hide the equalizer and playlist: only the player's shape is left.
    click(&mut a, 225, 62);
    click(&mut a, 248, 62);
    a.step().ok();
    let info = Command::new("xwininfo").args(["-display", &x.display, "-shape", "-id", &format!("{:#x}", a.win.xid())]).output().unwrap();
    let text = String::from_utf8_lossy(&info.stdout);
    let extents = text.lines().find(|l| l.contains("Window shape extents")).unwrap_or_else(|| panic!("no shape: {text}"));
    // Pixel centers decide: the slanted edge already cuts the top row's
    // last pixel (its center, x = 199.5, is past the edge at x = 199).
    assert!(extents.contains("199x100+0+0"), "{extents}");
}
