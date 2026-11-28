# SPEC: phonolite

## What this is
phonolite is a skinnable MP3 player for X11 that wears classic 2.x skins (`.wsz` archives or folders), drawn
with protolith alone, and small and frugal enough for a Pentium, with a 486DX2 as the stretch goal. It decodes
through mpg123 when that is installed and with its own decoder (the `phonolite-mpeg` crate) when it is not, and it
plays through a PulseAudio or PipeWire server, `out123`, `pacat`, `aplay` or `/dev/dsp`.

It is not a toolkit and links no C library. Not planned: native ALSA output (it would need far more `unsafe`), the
video window, the mini-browser, `gen.bmp` windows, and any copied skin art.

## Principles
These are AGENTS.md's non-negotiables; every change respects them.
1. **One dependency: protolith.** Everything else (decoding, inflate, ZIP, FFT, M3U/PLS, the Pulse protocol) is
   std-only Rust here. `unsafe` lives only in `src/oss.rs`; widening that needs the owner's say.
2. **It runs on a Pentium; a 486DX2 is the stretch goal.** Size is judged on the i486 build
   (`tools/build-i486.sh`, held to its size budget), never on x86_64. One decoder process, one pass over the
   samples; panes redraw only what changed. CPU is measured before and after anything that runs per frame or per
   sample.
3. **MSRV 1.75 and stable Rust**, tested on both.
4. **Test first; never weaken a test to pass.** New assertions are mutation-checked (break the code, watch the
   test fail, restore).
5. **Docs never outrun the tests.** A README or CHANGELOG claim is something a test checks, updated in the same
   commit.
6. **No copied skin art.** The built-in skin is generated in code; test fixtures are generated
   (`tools/fixtures.py`, `tools/mpeg-*`, `testsignal`).
7. **By-hand checks after touching their code**: `tools/check-windows.sh` after changes to `src/app/snap.rs` or to
   how windows are created, moved or shown; `tools/check-pulse.sh` after changes to `src/pulse.rs`.

## State
Done at adoption: everything README.md and CHANGELOG.md described as working. The suite then passed (125 tests,
none failing), so the gate started with an empty `known_red`. The built-in decoder's intensity stereo is ported,
but no test pinned it.

Planned, in this order:
1. Issue #1, idle memory. Its memory budget test comes first; then its phonolite-only proposals in the order 3 (no
   canvases for closed windows), 4 (0.4 s of audio history), 1 (one canvas per window). Proposals 2 and 6 wait
   for protolith's issue #1 (part B, indexed images; part A, the upload fix). Proposal 5 (double size scaled at
   present time) comes last.
2. Half-rate decoding for the 486, as mpg123's `-2` does.
3. On demand: trimming the encoder's delay and padding, window-shade mode, multi-select and drag to reorder in the
   playlist.
4. A small docs change: `Cargo.toml`'s comment names the decoder's folder as `src/mpeg/` (it is `mpeg/`), and the
   package description still calls phonolite "a frontend to mpg123".

README's other "Not (yet) there" points stay documented limits: the equalizer's shared low subband, the playlist
drawn in Terminus, the display following the sink rather than the card's latency, linear resampling, OSS-only
direct output.

## Architecture
A Cargo workspace of two crates. `phonolite` (`src/`) is a library with a binary on top (`src/main.rs`); every
module is public so it can be tested on its own. `phonolite-mpeg` (`mpeg/`) is the built-in decoder, minimp3
ported to safe Rust and built at `opt-level = 3` while the rest is built for size. protolith is a path dependency
(`../protolith`).
- Decoding and audio: `remote` (mpg123's remote protocol), `engine` (the decoder process), `builtin` (the built-in
  decoder behind the same interface), `audio` (the pump to the sink: volume, balance, pacing, replay after pause,
  gapless), `player` (playback on top), `eq` (10 bands onto 32 subbands), `oss` and `pulse` (direct output).
- Formats: `inflate` and `zip` (reading `.wsz`), `playlist` (entries, shuffle, repeat, M3U/PLS), `config`
  (settings kept between runs), `num` (fixed-point formatting and maths that keep std's float code and libm out
  of the i486 build).
- Skins: `skin` (parsers, geometry, the built-in skin generated in code) and `vis` (spectrum and oscilloscope).
- The app: `app` (the three windows, snapping, panes, dialogs, popup menus).
- Tests: inline `#[cfg(test)]` modules, `mpeg/src/tests.rs`, `tests/xvfb.rs` (end to end on Xvfb), generated
  fixtures in `tests/data/`. Build and checks: `tools/`, `targets/`, `.github/workflows/ci.yml`.

`Cargo.lock` and `third_party/` are out of scope (`docs/graph.yaml` gives the reasons).

## The regime
`docs/graph.yaml` holds the gate (the test command, the pattern of its failure lines, the failures it already
has), the paths out of scope, and the nodes; while the adoption runs it also lists the areas still to be mapped
into nodes. A node is built with `/noodlebot:node <ID>`; AGENTS.md's "Building a node" says how.

## Node spec template
A node spec `nodes/<ID>-<name>.md` has the headings Purpose, Tests, Changes, Gate and Risks, a header row
`| Track | Deps | Unlocks | Size |`, an **Owns:** line, and criteria `- **<ID>-AC-<n>** — … *Verify:* …`.

## Open decisions
- **Where new tests go.** Default: in `tests/<topic>.rs` (every module is public) or `mpeg/src/tests.rs`, which
  the role guard treats as test paths (`.noodlebot/roles.yaml`). The inline `#[cfg(test)]` modules stay as they
  are, and an implementer never weakens them.
- **Which protolith a gate run builds against.** Default: a checkout of protolith's main branch at `../protolith`,
  as CI does.

## Conventions
- Branches `node/<ID>-<name>` for a node and `spec/<topic>` for a spec change.
- Sizes: S (under a day), M (two or three days), L (more; split it if it can be split).
- A node is done when its gate criteria pass, CI's `gate` jobs (stable and 1.75) and its `i486` job pass on its
  PR, and the PR is merged.
- Commits: an imperative subject and a body that says why; CHANGELOG.md is updated in the same commit as the
  change it describes.
