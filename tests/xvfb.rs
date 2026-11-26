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

use phonolite::app::{App, Options, Pane};
use phonolite::engine::Mpg123;
use phonolite::player::{Decoding, SinkCommand, Status};
use phonolite::testsignal;
use protolith::x11::proto::request::{self, MapState};
use protolith::x11::{Connection, Event, Modifiers, MouseButton, MouseButtons};
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
    app_with(x, files, None)
}

fn app_with(x: &Xvfb, files: Vec<PathBuf>, config_dir: Option<PathBuf>) -> App {
    let conn = Connection::open_display(&x.display, None).expect("connect");
    let opts = Options {
        skin: None,
        decoding: Decoding::Mpg123(Mpg123::default()),
        outputs: vec![phonolite::player::Output::Command(SinkCommand::out123(&["-o".into(), "sleep".into()], 44100))],
        fps: 30,
        files,
        config_dir,
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

/// Each shown window's pixels on the server equal the frame phonolite
/// last drew for it (scaled): every incremental update landed where it
/// belongs.
fn assert_on_screen(app: &mut App) {
    let s = app.scale_factor();
    let shown: Vec<Pane> = Pane::ALL.into_iter().filter(|p| app.is_shown(*p)).collect();
    for pane in shown {
        let f = app.frame(pane).clone();
        // And the frame is what drawing it afresh gives: what was skipped
        // as unchanged really was. Only what moves with the clock may
        // differ between the two draws: the time, the seek bar, the
        // visualizer, the title, and the playlist's mini clock.
        let fresh = app.render_all(pane);
        let moving = match pane {
            Pane::Main => vec![Rect::new(36, 26, 63, 13), Rect::new(16, 72, 248, 10), Rect::new(24, 43, 76, 16), Rect::new(111, 27, 154, 6)],
            Pane::Eq => vec![],
            Pane::Pl => vec![Rect::new(189, f.height() as i32 - 17, 33, 9)],
        };
        let stale: Vec<(u32, u32)> = (0..f.height())
            .flat_map(|y| (0..f.width()).map(move |x| (x, y)))
            .filter(|&(x, y)| fresh.get(x, y) != f.get(x, y) && !moving.iter().any(|r| r.contains(x as i32, y as i32)))
            .collect();
        assert!(stale.is_empty(), "{pane:?}: stale pixels where nothing moves: {:?}", &stale[..stale.len().min(10)]);
        let got = app.conn.read_pixels(app.window(pane), Rect::new(0, 0, f.width() * s, f.height() * s)).expect("read back");
        for y in 0..f.height() * s {
            for x in 0..f.width() * s {
                let want = f.get(x / s, y / s).unwrap();
                let have = got[(y * f.width() * s + x) as usize] & 0xffffff;
                assert_eq!(have, want, "{pane:?}: pixel ({x}, {y}) at scale {s}");
            }
        }
    }
}

fn button(app: &mut App, pane: Pane, pressed: bool, x: i32, y: i32) {
    let b = app.bounds(pane);
    app.handle(Event::Button {
        window: app.window(pane),
        button: MouseButton::Left,
        pressed,
        x,
        y,
        root_x: b.x + x,
        root_y: b.y + y,
        modifiers: Modifiers::default(),
        time: 5000,
    });
}

/// Whether the X server shows a window.
fn map_state(app: &mut App, p: Pane) -> MapState {
    let xid = app.window(p).xid();
    let reply = app.conn.client().round_trip(&request::get_window_attributes(xid)).unwrap();
    request::parse_window_attributes(&reply).unwrap().map_state
}

/// A click at 1x coordinates in the main window.
fn click(app: &mut App, x: i32, y: i32) {
    let s = app.scale_factor() as i32;
    let (x, y) = (x * s + s / 2, y * s + s / 2);
    button(app, Pane::Main, true, x, y);
    button(app, Pane::Main, false, x, y);
}

/// Press at `at` (1x) in a window, move the pointer by `by` (screen
/// pixels) in a few steps, release; then where every window is, as the
/// X server has it.
fn drag(app: &mut App, pane: Pane, at: (i32, i32), by: (i32, i32)) -> [(i32, i32); 3] {
    let s = app.scale_factor() as i32;
    let (x, y) = (at.0 * s, at.1 * s);
    let start = app.bounds(pane);
    let (rx, ry) = (start.x + x, start.y + y);
    button(app, pane, true, x, y);
    for k in 1..=4 {
        let (root_x, root_y) = (rx + by.0 * k / 4, ry + by.1 * k / 4);
        // Window coordinates are the window's at the time (it moves).
        let now = app.bounds(pane);
        let window = app.window(pane);
        app.handle(Event::Motion {
            window,
            x: root_x - now.x,
            y: root_y - now.y,
            root_x,
            root_y,
            buttons: MouseButtons { left: true, ..MouseButtons::default() },
            modifiers: Modifiers::default(),
        });
    }
    let now = app.bounds(pane);
    let window = app.window(pane);
    app.handle(Event::Button {
        window,
        button: MouseButton::Left,
        pressed: false,
        x: rx + by.0 - now.x,
        y: ry + by.1 - now.y,
        root_x: rx + by.0,
        root_y: ry + by.1,
        modifiers: Modifiers::default(),
        time: 5100,
    });
    app.step().unwrap();
    Pane::ALL.map(|p| {
        let w = app.window(p);
        app.conn.window_position(w).unwrap()
    })
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
fn hiding_windows_unmaps_them() {
    let Some(x) = xvfb() else { return };
    let mut a = app(&x, Vec::new());
    a.step().ok();
    assert_on_screen(&mut a);
    // The equalizer toggle (219,58), then the playlist's (242,58).
    click(&mut a, 225, 62);
    click(&mut a, 248, 62);
    a.step().ok();
    assert!(!a.is_shown(Pane::Eq) && !a.is_shown(Pane::Pl));
    for p in [Pane::Eq, Pane::Pl] {
        assert_eq!(map_state(&mut a, p), MapState::Unmapped, "{p:?}");
    }
    assert_eq!(a.conn.window_size(a.win).unwrap(), (275, 116), "the main window is as it was");
    assert_on_screen(&mut a);
    // Back again, drawn.
    click(&mut a, 225, 62);
    a.step().ok();
    assert_eq!(map_state(&mut a, Pane::Eq), MapState::Viewable);
    assert_on_screen(&mut a);
}

#[test]
fn windows_snap_off_and_back_on_and_docked_ones_follow_the_main_window() {
    let Some(x) = xvfb() else { return };
    let mut a = app(&x, Vec::new());
    a.step().ok();
    // First run: stacked, docked, in the middle of the 800x1000 screen.
    let (mx, my) = ((800 - 275) / 2, (1000 - 464) / 2);
    let start = Pane::ALL.map(|p| {
        let w = a.window(p);
        a.conn.window_position(w).unwrap()
    });
    assert_eq!(start, [(mx, my), (mx, my + 116), (mx, my + 232)]);
    // The equalizer's title bar pulls it out of the stack, away from
    // every edge: it goes exactly where it is dragged.
    let at = drag(&mut a, Pane::Eq, (150, 6), (-200, 300));
    assert_eq!(at, [(mx, my), (mx - 200, my + 416), (mx, my + 232)]);
    // Back, to within a few pixels of main's right side: it snaps there,
    // tops aligned.
    let at = drag(&mut a, Pane::Eq, (150, 6), (200 + 275 + 6, -416 + 3));
    assert_eq!(at[1], (mx + 275, my), "docked beside the main window");
    // The main window carries the equalizer docked to it; the playlist,
    // whose dock (the equalizer's bottom) left, stays.
    let at = drag(&mut a, Pane::Main, (150, 6), (-100, -50));
    assert_eq!(at, [(mx - 100, my - 50), (mx + 175, my - 50), (mx, my + 232)]);
    // The playlist alone, toward the screen's left edge: 5 px from it, it
    // snaps to it.
    let at = drag(&mut a, Pane::Pl, (100, 10), (-mx + 5, 0));
    assert_eq!(at[2], (0, my + 232));
    // Under the main window again (a little off): snapped on, aligned.
    let at = drag(&mut a, Pane::Pl, (100, 10), (mx - 100 + 4, -232 + 116 - 50 + 6));
    assert_eq!(at[2], (mx - 100, my + 66));
    // Now all three move together.
    let at = drag(&mut a, Pane::Main, (150, 6), (30, 40));
    assert_eq!(at, [(mx - 70, my - 10), (mx + 205, my - 10), (mx - 70, my + 106)]);
    // Double size keeps the docked windows docked, at twice the offsets.
    let ctrl = Modifiers { ctrl: true, ..Modifiers::default() };
    let key = protolith::x11::Key::Character('d');
    a.handle(Event::Key { window: a.win, key, keysym: 0x64, keycode: 0, pressed: true, repeat: false, modifiers: ctrl });
    a.step().unwrap();
    let at = Pane::ALL.map(|p| {
        let w = a.window(p);
        a.conn.window_position(w).unwrap()
    });
    assert_eq!(at, [(mx - 70, my - 10), (mx - 70 + 550, my - 10), (mx - 70, my - 10 + 232)]);
    a.step().unwrap();
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

#[test]
fn window_places_survive_a_restart() {
    let Some(x) = xvfb() else { return };
    let dir = std::env::temp_dir().join(format!("phonolite-xvfb-{}-places", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // The X server resets when its last client leaves: stay connected.
    let _keep = Connection::open_display(&x.display, None).expect("connect");
    let mut a = app_with(&x, Vec::new(), Some(dir.clone()));
    a.step().ok();
    let moved = drag(&mut a, Pane::Pl, (100, 10), (300, 100));
    drop(a); // saves
    let mut b = app_with(&x, Vec::new(), Some(dir.clone()));
    b.step().ok();
    let at = Pane::ALL.map(|p| {
        let w = b.window(p);
        b.conn.window_position(w).unwrap()
    });
    assert_eq!(at, moved);
    assert_on_screen(&mut b);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_window_placed_elsewhere_at_the_start_is_put_back() {
    let Some(x) = xvfb() else { return };
    let mut a = app(&x, Vec::new());
    a.step().ok();
    let eq = a.window(Pane::Eq);
    let want = a.conn.window_position(eq).unwrap();
    // As openbox does with transient windows: another client moves it.
    let mut other = protolith::x11::Client::connect(&protolith::x11::DisplayName::parse(&x.display).unwrap(), None).unwrap();
    other.send(&request::configure_window(eq.xid(), &request::ConfigureWindow { x: Some(0), y: Some(0), ..Default::default() })).unwrap();
    other.sync().unwrap();
    assert_eq!(a.conn.window_position(eq).unwrap(), (0, 0));
    run_until(&mut a, "the equalizer put back", |a| a.conn.window_position(eq).unwrap() == want);
    // Once settled, a move by someone else (the user, through the window
    // manager) stands.
    let deadline = Instant::now() + Duration::from_millis(2500);
    while Instant::now() < deadline {
        a.step().unwrap();
    }
    other.send(&request::configure_window(eq.xid(), &request::ConfigureWindow { x: Some(5), y: Some(6), ..Default::default() })).unwrap();
    other.sync().unwrap();
    // (Idle, a step waits up to a second for an event.)
    for _ in 0..2 {
        a.step().unwrap();
    }
    assert_eq!(a.conn.window_position(eq).unwrap(), (5, 6));
}
