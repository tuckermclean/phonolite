# Working on phonolite — AGENTS.md

How to work on this program, for AI agents and humans alike. Short on
purpose; doc comments and `README.md` say *what* it is.

## The non-negotiables

1. **One dependency: protolith.** It draws, takes input, and provides the
   widgets. Decoding is mpg123's when it is installed, else the built-in
   decoder's (`mpeg/`, the `phonolite-mpeg` crate: minimp3 ported, built
   for speed); everything else (inflate,
   ZIP, FFT, M3U/PLS) is here in std-only Rust. **`unsafe` lives only in
   `src/oss.rs`** — libc's `ioctl` with the OSS requests that take one
   `int`, because setting a sound device's format has no safe route. The
   crate denies `unsafe_code` and a test fails if the word appears in any
   other file; widening that needs the user's say.
2. **It runs on a Pentium; a 486DX2 is the stretch goal** — like Stele
   and protolith, which exist for that hardware. The build that counts is
   `tools/build-i486.sh` (Stele's i486 musl target, std rebuilt, panics
   aborting, no unwind tables; podman and the monolith-builder image), held
   to a size budget there. Judge size on it, never on x86_64. One decoder
   process, never two (a second mpg123 for the visualizer was rejected for
   this). The pump does one pass over the samples. Panes redraw only when
   what they show changed, and only changed pixels are sent. Measure CPU
   (`/proc/<pid>/stat` ticks over a fixed playback window) before and after
   anything that runs per frame or per sample.
3. **MSRV 1.75, stable Rust.** Test on both.
4. **Test first; never weaken a test to pass.** Mutation-check new
   assertions (break the code, watch the test fail, restore).
5. **Docs never outrun the tests.** README/CHANGELOG claims are things a
   test checks, updated in the same commit.
6. **No copied skin art.** The built-in skin is generated in code. A
   user's classic skin (e.g. `~/Downloads/base-2.91.wsz`) is reference
   material for checking layouts: never commit it or anything extracted
   from it. Test fixtures are generated (`tools/fixtures.py`, `tools/mpeg-*`,
   `testsignal`).

## The gate (run before every commit)

```sh
cargo fmt --all --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
PHONOLITE_REQUIRE_MPG123=1 PHONOLITE_REQUIRE_XVFB=1 cargo test --workspace
PHONOLITE_REQUIRE_MPG123=1 PHONOLITE_REQUIRE_XVFB=1 cargo +1.75 test --workspace
for i in $(seq 1 10); do cargo test -q --workspace -- --test-threads=16 || break; done
tools/build-i486.sh    # the target build, and its size budget
```

The stress loop matters: many tests run real mpg123/out123 processes in
real time, and races show only under load. Needs `mpg123` (which brings
`out123`) and `xvfb`. protolith is a path dependency (`../protolith`).

## Test tiers

1. Pure: the remote protocol, EQ mapping, playlist, inflate/ZIP (against
   Python zlib fixtures), skin parsers and geometry, FFT (against a naive
   DFT), the audio pump (in-memory sources and sinks: order, pacing,
   pause/replay, gapless, flush).
2. The decoder: against mpg123's decode of `tests/data/mpeg/` (LAME and
   ffmpeg encodes of generated signals, made in a throwaway Alpine
   container; within 1 LSB, aligned for the LAME Info frame), corrupt
   input never panics. The player's playback tests run on both decoders.
3. Real processes: mpg123 decoding generated MPEG-1 Layer I tones
   (`mpeg/src/testsignal.rs` — a tone per subband is a known frequency), into
   `out123 -O file` (to measure what came out) or `out123 -o sleep`
   (real-time output with no sound card).
4. Panes with synthetic protolith events (`WindowId::from_xid`).
5. Xvfb end to end (`tests/xvfb.rs`): the app in-process on a real server;
   the window read back must equal the last frame, and the last frame must
   equal a from-scratch render (catches stale caching).

6. Period hardware, by hand when the question comes up (not CI):
   `tools/bench-86box.sh [CPU|all]` boots The Monolith's i486 kernel in
   86Box (an ASUS 486SP3G, Award BIOS, its CMOS in `tools/86box/`) and
   times the decoder there, in the guest's own clock. Needs
   `MONOLITH_ISO`, podman, Xvfb; about two minutes per CPU.

## Hard-won lessons

- mpg123 replies on stderr (`--remote-err`) so stdout can carry audio; it
  prints `@P 3` then `@P 0` at a track's end, `@P 0` alone on STOP.
- ID3v1 fields in `@I ID3:` are fixed width in *bytes* after UTF-8
  conversion, not characters.
- A pipe holds ~0.37 s at 44.1 kHz stereo and can't be shrunk without
  `fcntl`. Hence: per-track decoder processes (flush = kill), the sink
  killed on pause with the unheard tail replayed, and writes paced to
  `LEAD` ahead of the clock.
- At the natural end, close the sink's input and let it drain — killing it
  cuts off the last buffer.
- Without a window manager a newly mapped window takes the focus: a popup
  must not close on the main window's focus-out alone.
- Terminus (the 191-glyph subset) has no `…` or `•`.
- Xvfb resets when its last client disconnects: a test that reconnects
  (a restart) must hold another connection open meanwhile.
- Window placement: run `tools/check-windows.sh` after touching
  `src/app/snap.rs` or how windows are created, moved or shown — Xvfb
  tests have no window manager, and each real one differs. In it, give
  the WM a few seconds to start (openbox's first start announces itself
  long before it handles clicks), read positions with `xwininfo`
  (`xdotool getwindowgeometry` counts a reparenting WM's title twice),
  and kill what each run started (`timeout` alone leaves Xvfb behind).
- 86Box: a fresh CMOS stops POST ("press F1"); the board's disk type
  must be in CMOS (made once in SETUP, kept as an `.nvr` file), so the
  disk image keeps its geometry. Its process is named `qt_thread`, not
  86Box; it ignores SIGTERM; it writes disk images through, so the host
  can poll the results partition while it runs. The Monolith rc1
  initramfs's busybox has no `dd` or `sed`.
- The Monolith rc1 kernel unpacks at 16 MB (`CONFIG_PHYSICAL_START`):
  it needs about 26 MB of RAM to boot; the 1993 AMI board (ali1429) never
  started it at all. Bench boards need 32 MB.
- `read_pixels` reads protolith's backing pixmap, so it works on unmapped
  windows; ask the server (`GetWindowAttributes`) whether one is shown.
- PulseAudio: protocol details come from its `native-common.h`,
  `tagstruct.c` and `protocol-native.c`; the fake server in `pulse.rs` only
  encodes our reading of them, so `tools/check-pulse.sh` (real PulseAudio
  and PipeWire in containers) is the proof — run it after touching
  `pulse.rs`.
- Size, on the i486 build: `{}`/`{:.N}` on an `f64`, or `str::parse::<f64>`,
  pulls in ~24 KB of std's float conversion — use `num::fixed` and
  `num::parse_decimal`. `f64::ln`/`powf`/`sin`/`cos`/`log10` pull ~17 KB of
  musl's libm — use `num::{ln, exp, pow, log10, sin_cos}`. Find what pulls
  a symbol in by walking callers in `objdump -d -C` of a `STRIP=false`
  build. Left in on purpose: musl's `printf` (16 KB), which std's thread
  spawning reaches through `pthread_setname_np` — avoiding it would need a
  `#[no_mangle]` override, i.e. `unsafe`.

## Housekeeping

- Commits: imperative subject, a body that says why, ending with the
  attribution lines the session asks for.
- CHANGELOG.md is updated in the same commit as the change.
