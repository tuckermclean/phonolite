# Changelog

## 0.1.0 — unreleased

First version: a classic-skinned mpg123 frontend on protolith.

- **Audio**: one mpg123 per track in remote mode (`-R --remote-err`),
  decoding to a pipe; a pump passes the samples to `out123` (or any sink
  command), applying volume and balance, pacing writes 0.3 s ahead of the
  clock, replaying the unheard tail after a pause, flushing on seek or
  skip, and queuing tracks back to back with no gap. The equalizer is
  mpg123's (10 bands mapped onto its 32 subbands, plus preamp). Decode
  rate selectable (`--rate`). Tags from mpg123 (ID3v1 fixed-width in bytes,
  ID3v2, ICY). A failed file moves the playlist on; five in a row stop.
- **Skins**: `.wsz` (ZIP: stored and deflated members, CRC-checked;
  inflate written here, tested against Python's zlib) or folders, file
  names in any case, one folder deep; per-file fallback to the built-in
  skin (`balance.bmp` to the skin's own `volume.bmp`); `nums_ex.bmp`;
  `viscolor.txt`, `pledit.txt`, `region.txt` (polygons rasterized by pixel
  centers into the window's SHAPE). Geometry measured on real sheets.
- **Built-in skin**: every sheet generated in code (volcanic stone, mint
  LCD, amber lamps), including a 3x5 font for the whole text sheet and
  a 5x7 one, spaced and shadowed, for the window titles.
- **Windows**: player, equalizer and playlist, docked; double size; always on top;
  minimize; popup menus (override-redirect); dialogs (file browser, skin
  picker, playlist load/save, URL, jump to file) from protolith widgets.
- **Visualizer**: 19-bar spectrum (512-point FFT, falling bars, held
  peaks) or oscilloscope, in `viscolor.txt` colors.
- **Playlist**: M3U/M3U8/PLS read and write, folder scanning, shuffle (a
  fresh order per lap, never the same track twice in a row), repeat,
  sort/reverse/randomize/crop, stable entry ids across edits.
- **Frugal drawing**: panes redraw only when what they show changed (most
  frames only the visualizer and title), and only changed pixels go to
  the server. Measured: 1.65 % of a core at 30 fps, 0.95 % at
  `--rate 22050 --fps 10`.
- Settings and playlist kept in `$XDG_CONFIG_HOME/phonolite`.
- **Detachable windows that snap**: the player, equalizer and playlist
  are three windows (the latter two transient for the player). Drag the
  equalizer or playlist by its title bar to pull it off; an edge within
  10 px of another window's (beside, above, below, or aligned with it) or
  of the screen's snaps to it; the player carries whatever is docked to
  it, transitively (`src/app/snap.rs`). phonolite moves the windows
  itself, following the pointer — a window manager's move cannot snap to
  the other windows. Positions are saved (`main_pos`, `eq_pos`,
  `pl_pos`); a saved place off the current screen falls back to the
  stack in the middle. Double size scales docked offsets; resizing the
  playlist moves what is docked under it. For two seconds after a window
  is mapped phonolite puts it back if the window manager placed it
  elsewhere (openbox centers transient windows on their owner; Fluxbox
  may ignore the position asked for). Tests: snapping and docking rules;
  on Xvfb, pulling off, snapping beside, carrying, the screen edge,
  double size, a restart, and a window moved by another client while
  settling (put back) and after (left), against the server's idea of
  where each window is. `tools/check-windows.sh` drives the real program
  with a real pointer (xdotool) with no window manager and under twm,
  openbox, IceWM, Fluxbox and xfwm4 in a container: 18 of 18 runs right.
  It found two protolith bugs (positions were the frame's; resizing
  dropped the position hint, which hung twm), fixed there.
- **Its own PulseAudio client** (`src/pulse.rs`): the native protocol in
  safe Rust over the socket (`$PULSE_SERVER`, else
  `$XDG_RUNTIME_DIR/pulse/native`) — protocol 13, no shared memory, audio
  sent only as the server requests it, cut off by hanging up, drained at
  the end. First choice whenever such a server runs, so PulseAudio,
  PipeWire's Pulse layer and WSLg play with nothing installed. Checked by
  `tools/check-pulse.sh`: a tone played into real PulseAudio and PipeWire
  1.4 (no cookie) arrives at 440 Hz at the level sent; unit tests check the
  encodings byte for byte and flow control against a server that enforces
  its credit. +16 KB on the i486 build.
- **The decoder is its own crate**, `phonolite-mpeg` (`mpeg/`), built at
  opt-level 3 while the rest stays at "z": 406x real time on 320 kbit/s
  stereo, from 283x, for 12 KB on the i486 build. `cargo run --release -p
  phonolite-mpeg --example bench -- FILE` measures it.
- **A built-in decoder** for when mpg123 is not installed (`--decoder
  auto`, the default, prefers mpg123): MPEG-1/2/2.5 Layers I–III, minimp3
  ported to safe Rust (CC0), its tables extracted by script. Within one
  LSB of mpg123 on twelve LAME/ffmpeg fixtures; corrupt input decodes to
  noise, never a panic. ID3v1/v2 tags, LAME/Xing length and seek table,
  linear resampling, the equalizer in its subbands. The playback tests run
  on both decoders.
- **An output that fails hands over to the next** (from what was last
  heard), so one broken route — say `aplay` whose default ALSA device
  will not open — no longer means silence; when none works, the error
  lists each one's reason. Messages also go to stderr in full (the title
  shows a few seconds of a scrolling fragment). A sound server is also
  recognized by `$PULSE_SERVER`.
- **More ways out**: `pacat` (PulseAudio, PipeWire's Pulse layer) and
  `aplay` (alsa-utils) as sinks, found on `PATH`; mixing outputs before
  the exclusive `/dev/dsp`; with nothing to play through, the error says
  what to install. Both argument lists were checked playing into real
  programs (ALSA's null device, a PulseAudio null sink).
- **Direct OSS output** (`/dev/dsp`), the last resort and the bare machine's:
  the one module with `unsafe` (libc's `ioctl` for format, channels, rate,
  fragments and reset); a test keeps `unsafe` out of every other file.
- **The i486 build** (`tools/build-i486.sh`, Stele's target and recipe):
  423 KB, from 570 KB, under a 440 KB budget in CI (468 KB and a 480 KB
  budget with the built-in decoder and direct output). Cut: unwind tables
  (`-Cforce-unwind-tables=no`) and the unwinder (an empty `libunwind`
  stand-in instead of `libgcc_eh`), std's float formatting and parsing
  and musl's libm (replaced by `num`, tested against std), and libc's DNS
  resolver (protolith now resolves TCP display hosts itself).
