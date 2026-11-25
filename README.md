# phonolite

A skinnable MP3 player for X11 — a frontend to
[mpg123](https://mpg123.de) that wears classic 2.x skins (`.wsz`), drawn
with [protolith](https://github.com/tuckermclean/protolith). No toolkit, no
C libraries, no `unsafe`; one Rust dependency. Small and frugal enough to
aim at a 486.

```sh
phonolite ~/Music                      # a folder, files, .m3u/.pls, or URLs
phonolite --skin ~/skins/Classic.wsz   # wear a skin (or a folder of its files)
phonolite --rate 22050 --fps 10 ...    # a slow machine: half the work
```

Needs `mpg123` at run time (it brings `out123`, which plays the sound).

## What it does

- **Three docked panes in one shaped, borderless window**: the player, the
  10-band equalizer, and the resizable playlist — stacked as classic
  players lay them out, dragged as one. Each can be hidden; double size
  (Ctrl+D) scales it all; always on top (Ctrl+A); minimize.
- **Classic skins**: `.wsz` archives (ZIP, read by phonolite's own inflate)
  or folders; any file a skin lacks comes from the built-in skin.
  `viscolor.txt`, `pledit.txt` (playlist colors) and `region.txt` (the
  window's shape) are honored; text and digits come from the skin's
  sprite fonts.
- **A built-in skin** of its own, generated in code: speckled volcanic
  stone, a mint LCD, amber lamps.
- **Playback**: play/pause/stop/previous/next, seek bar, volume, balance,
  shuffle, repeat, gapless track changes, streams (`http://…`), elapsed or
  remaining time, tags read by mpg123 (ID3v1/v2, ICY stream titles).
- **Equalizer**: ON, preamp, ten bands, a response curve, built-in presets.
- **Visualizer**: a 19-bar spectrum analyzer with falling peaks, or an
  oscilloscope, in the skin's colors; click it to switch.
- **Playlist**: double-click to play, Delete to remove, scroll bar, sort,
  reverse, randomize, crop, M3U/PLS load and save, jump-to-file (J).
- **Dialogs and menus** built from protolith's widgets: a file browser,
  a skin picker, a URL prompt, popup menus (right-click anywhere).
- Settings and the playlist persist in `~/.config/phonolite/`.

## How the audio works

```
mpg123 -R  ──raw PCM──▶  phonolite pump  ──▶  out123 ──▶ sound card
(decodes,                (volume, balance,
 EQ in its subbands)      visualizer tap)
```

One decoder, one pass over the samples: the visualizer reads the samples
on their way to the sink instead of decoding the file a second time. The
equalizer is mpg123's own (it scales the decoder's 32 subbands — free).
Writes to the sink are paced 0.3 s ahead of the clock, so volume changes
are heard at once; pausing cuts the sink off and replays what had not been
heard; seeking or skipping starts a fresh decoder, so nothing stale plays.
Tracks that run into each other are queued back to back with no gap.

## Cost

Measured with the visualizer on, over 20 s of playback (CPU time as a share
of one modern core; the test audio is MPEG Layer I, cheaper to decode than
MP3, so mpg123's share is understated):

| profile | phonolite | mpg123 | out123 |
|---|---|---|---|
| default (44.1 kHz, 30 fps) | 1.65 % | 0.95 % | 0.15 % |
| `--rate 22050 --fps 10` | 0.95 % | 0.55 % | 0.05 % |

phonolite redraws only what changed (most frames: the visualizer and the
scrolling title) and sends only changed pixels to the X server. The release
binary is about 815 KB (opt-level z, LTO, stripped).

## Keys

`z x c v b` previous, play, pause, stop, next · `l` add files · `j` jump
to file · ←/→ seek 5 s · ↑/↓ volume · `s`/`r` shuffle/repeat · Ctrl+D double
size · Ctrl+A always on top · Alt+G / Alt+E equalizer / playlist · Ctrl+Q
quit · media keys · Escape closes menus. With the playlist focused, the
arrows, Enter and Delete act on it.

## Options

```
-s, --skin PATH      wear a classic skin (.wsz, or a folder of its files)
-o, --output MODULE  out123 output module: alsa, pulse, oss, jack, ...
    --sink COMMAND   another sink reading raw 16-bit stereo on stdin
    --mpg123 PATH    the mpg123 program to run
    --fps N          animation frames per second (default 30)
    --rate HZ        decode at this sample rate (default 44100)
    --no-config      neither load nor save settings and playlist
```

## Not (yet) there

- The equalizer is mapped onto mpg123's 32 equal-width subbands, so the
  60, 170, 310 and 600 Hz sliders share the lowest one (0–689 Hz) and act
  together; changes are heard after the decoder's pipe (~0.4 s).
- The playlist selects one entry at a time; no drag to reorder, no
  drag-and-drop from file managers.
- The panes stay docked; the shade button hides the equalizer and playlist
  rather than rolling the player up into window-shade mode.
- The playlist is drawn in Terminus, not the font `pledit.txt` names.
- No mini-browser, video window, `gen.bmp` windows or skin cursors.
- Displayed time and the visualizer follow what has been sent to the sink,
  not the sound card's own latency.

## Development

See [AGENTS.md](AGENTS.md): the rules, the test gate, the test tiers.
protolith is a path dependency (`../protolith`).

## License

MIT or Apache-2.0, at your option. The binary embeds Terminus Font glyphs
(via protolith's `font-terminus-12`), which are OFL-1.1.
