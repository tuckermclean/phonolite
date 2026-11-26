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
- **Windows**: player, equalizer and playlist docked in one shaped,
  borderless window, dragged as one; double size; always on top;
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
- **A built-in decoder** for when mpg123 is not installed (`--decoder
  auto`, the default, prefers mpg123): MPEG-1/2/2.5 Layers I–III, minimp3
  ported to safe Rust (CC0), its tables extracted by script. Within one
  LSB of mpg123 on twelve LAME/ffmpeg fixtures; corrupt input decodes to
  noise, never a panic. ID3v1/v2 tags, LAME/Xing length and seek table,
  linear resampling, the equalizer in its subbands. The playback tests run
  on both decoders.
- **Direct OSS output** (`/dev/dsp`), chosen when no sound server runs:
  the one module with `unsafe` (libc's `ioctl` for format, channels, rate,
  fragments and reset); a test keeps `unsafe` out of every other file.
- **The i486 build** (`tools/build-i486.sh`, Stele's target and recipe):
  423 KB, from 570 KB, under a 440 KB budget in CI (468 KB and a 480 KB
  budget with the built-in decoder and direct output). Cut: unwind tables
  (`-Cforce-unwind-tables=no`) and the unwinder (an empty `libunwind`
  stand-in instead of `libgcc_eh`), std's float formatting and parsing
  and musl's libm (replaced by `num`, tested against std), and libc's DNS
  resolver (protolith now resolves TCP display hosts itself).
