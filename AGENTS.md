# Working on phonolite — AGENTS.md

How to work on this program, for AI agents and humans alike. Short on
purpose; doc comments and `README.md` say *what* it is.

## The non-negotiables

1. **One dependency: protolith.** It draws, takes input, and provides the
   widgets. Audio is mpg123's (a separate program); everything else
   (inflate, ZIP, FFT, M3U/PLS) is here in std-only Rust. No `unsafe`
   (`#![forbid(unsafe_code)]`).
2. **It must be able to run on a 486.** One decoder process, never two
   (an earlier design ran a second mpg123 for the visualizer — rejected for
   exactly this). The pump does one pass over the samples. Panes redraw
   only when what they show changed, and only changed pixels are sent.
   Measure CPU (`/proc/<pid>/stat` ticks over a fixed playback window)
   before and after anything that runs per frame or per sample.
3. **MSRV 1.75, stable Rust.** Test on both.
4. **Test first; never weaken a test to pass.** Mutation-check new
   assertions (break the code, watch the test fail, restore).
5. **Docs never outrun the tests.** README/CHANGELOG claims are things a
   test checks, updated in the same commit.
6. **No copied skin art.** The built-in skin is generated in code. A
   user's classic skin (e.g. `~/Downloads/base-2.91.wsz`) is reference
   material for checking layouts: never commit it or anything extracted
   from it. Test fixtures are generated (`tools/fixtures.py`, `testsignal`).

## The gate (run before every commit)

```sh
cargo fmt --check
RUSTFLAGS="-D warnings" cargo clippy --all-targets
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
PHONOLITE_REQUIRE_MPG123=1 PHONOLITE_REQUIRE_XVFB=1 cargo test
PHONOLITE_REQUIRE_MPG123=1 PHONOLITE_REQUIRE_XVFB=1 cargo +1.75 test
for i in $(seq 1 10); do cargo test -q -- --test-threads=16 || break; done
```

The last line matters: many tests run real mpg123/out123 processes in real
time, and races show only under load. Needs `mpg123` (which brings
`out123`) and `xvfb`. protolith is a path dependency (`../protolith`).

## Test tiers

1. Pure: the remote protocol, EQ mapping, playlist, inflate/ZIP (against
   Python zlib fixtures), skin parsers and geometry, FFT (against a naive
   DFT), the audio pump (in-memory sources and sinks: order, pacing,
   pause/replay, gapless, flush).
2. Real processes: mpg123 decoding generated MPEG-1 Layer I tones
   (`src/testsignal.rs` — a tone per subband is a known frequency), into
   `out123 -O file` (to measure what came out) or `out123 -o sleep`
   (real-time output with no sound card).
3. Panes with synthetic protolith events (`WindowId::from_xid`).
4. Xvfb end to end (`tests/xvfb.rs`): the app in-process on a real server;
   the window read back must equal the last frame, and the last frame must
   equal a from-scratch render (catches stale caching).

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

## Housekeeping

- Commits: imperative subject, a body that says why, ending with the
  attribution lines the session asks for.
- CHANGELOG.md is updated in the same commit as the change.
