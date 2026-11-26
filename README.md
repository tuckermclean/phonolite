# phonolite

A skinnable MP3 player for X11 that wears classic 2.x skins (`.wsz`),
drawn with [protolith](https://github.com/tuckermclean/protolith). It
plays through [mpg123](https://mpg123.de) when that is installed and
decodes MPEG audio itself when it is not; it plays through `out123`, or
straight to the sound device. No toolkit, no C libraries, one Rust
dependency, and `unsafe` in exactly one small module (the sound device's
`ioctl`s). Small and frugal enough to aim at a 486.

```sh
phonolite ~/Music                      # a folder, files, .m3u/.pls, or URLs
phonolite --skin ~/skins/Classic.wsz   # wear a skin (or a folder of its files)
phonolite --rate 22050 --fps 10 ...    # a slow machine: half the work
```

Nothing else is required: without mpg123 it decodes with its built-in
decoder, and it plays straight to a PulseAudio or PipeWire server, or
through whichever of `out123`, `pacat` or `aplay` is installed, or straight
to `/dev/dsp` (OSS; on ALSA systems the kernel's OSS
emulation, `CONFIG_SND_PCM_OSS`). With none of those, it says what to
install.

## What it does

- **Three windows that snap together**: the player (shaped by the skin),
  the 10-band equalizer and the resizable playlist, borderless, stacked
  and docked at first. Drag the equalizer or playlist by its title bar to
  pull it off; bring an edge within 10 pixels of another window's —
  below, above or beside it — or of the screen's, and it snaps there.
  Dragging the player carries every window docked to it, directly or
  through another. Where they are is remembered. Each can be hidden;
  double size (Ctrl+D) scales it all and keeps docked windows docked;
  always on top (Ctrl+A); minimize.
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
mpg123 -R ─────┐                               ┌──▶ out123 ──▶ sound card
 (or the        ├──raw PCM──▶ phonolite pump ──┤
 built-in       │             (volume, balance,└──▶ /dev/dsp (OSS)
 decoder) ──────┘              visualizer tap)
```

**Decoding.** mpg123 when it runs (`--decoder auto`, the default), else the
built-in decoder (the `phonolite-mpeg` crate in `mpeg/`): MPEG-1/2/2.5 Layers I, II and III, a port
to safe Rust of [minimp3](https://github.com/lieff/minimp3) (public domain,
CC0; `third_party/minimp3/`). On every test file — LAME and ffmpeg encodes
at MPEG-1/2/2.5, mono, stereo, joint stereo, CBR and VBR, Layers II and III
— its output is within one least significant bit of mpg123's. It reads
ID3v1/v2 tags and the LAME/Xing header (length, seek table), resamples
linearly to the session rate, and applies the equalizer in its subbands
just as mpg123 does. It plays files; streams (`http://…`) need mpg123.

**Output.** When a PulseAudio-speaking server runs — PulseAudio, PipeWire
(through its Pulse layer, as on most current desktops) or WSLg — phonolite
talks to it itself (`src/pulse.rs`: the native protocol in safe Rust over
the socket, as protolith speaks X11), with nothing else installed.
Otherwise the first of these that is installed: `out123` (with mpg123);
`pacat` when a server runs; `aplay` (alsa-utils).
All of them mix with other programs — through the server, or alsa-lib's
dmix where it is configured — so they come before `/dev/dsp`, which is
exclusive (dmix lives in alsa-lib, and OSS bypasses it): the last resort,
and the way on a bare machine. If one fails — `aplay` that cannot open
its device, a server that refuses — phonolite says why on stderr and goes
on through the next, from what was last heard; if none works, the error
lists what each said. `--dsp`, `-o` and `--sink` choose explicitly. PipeWire's own `pw-cat` is not used: it reads only files with
a header, not a raw stream; its Pulse layer serves `pacat`.

One decoder, one pass over the samples: the visualizer reads the samples
on their way to the sink instead of decoding the file a second time. The
equalizer is mpg123's own (it scales the decoder's 32 subbands — free).
Writes to the sink are paced 0.3 s ahead of the clock, so volume changes
are heard at once; pausing cuts the sink off and replays what had not been
heard; seeking or skipping starts a fresh decoder, so nothing stale plays.
Tracks that run into each other are queued back to back with no gap.

## Cost

It is meant for a Pentium, with a 486DX2 as the stretch goal. Built for
that (`tools/build-i486.sh`: 32-bit static musl for an i486, std rebuilt
from source, panics aborting), phonolite is **505 KB**, of which the
built-in decoder (built for speed) and direct OSS and PulseAudio output are
about 78 KB; a program that does nothing, built the same way, is 58 KB (KB
here: 1000 bytes). CI holds it under a 520 KB budget. A size pass found, and removed, 147 KB: unwind tables and
the unwinder (nothing unwinds), std's float formatting and parsing, libm,
and libc's DNS resolver (via protolith).

The built-in decoder (its own crate, built at `opt-level = 3` while the
rest is built for size) decodes 320 kbit/s stereo at 406× real time on one
modern core — the same as mpg123's portable C path (mpg123 with AVX: 2×
faster). On period hardware, emulated cycle by cycle in 86Box
(`tools/bench-86box.sh`), as a multiple of real time:

| CPU | MP3 128 kbit/s joint stereo | MP3 320 kbit/s | MP3 64 kbit/s mono | MP3 22 kHz (MPEG-2) | MP2 192 kbit/s |
|---|---|---|---|---|---|
| 486DX2-66 | 0.55× | 0.53× | 0.68× | 1.11× | 0.66× |
| Am486DX4-100 | 0.84× | 0.80× | 1.03× | 1.68× | 0.99× |
| Am5x86-133 | 1.12× | 1.07× | 1.38× | 2.24× | 1.33× |
| Pentium OverDrive 83 | 2.10× | 1.94× | 2.41× | 4.24× | 2.16× |

So the built-in decoder plays 44.1 kHz MP3 from an Am5x86-133 up (with
little to spare) and comfortably on a Pentium; a 486DX2-66 keeps up only
with 22 kHz files. Under 1× the music stutters.

CPU, measured on x86_64 with the visualizer on over 20 s of playback (as a
share of one modern core; the test audio is MPEG Layer I, cheaper to decode
than MP3, so mpg123's share is understated):

| profile | phonolite | mpg123 | out123 |
|---|---|---|---|
| default (44.1 kHz, 30 fps) | 1.65 % | 0.95 % | 0.15 % |
| `--rate 22050 --fps 10` | 0.95 % | 0.55 % | 0.05 % |

phonolite redraws only what changed (most frames: the visualizer and the
scrolling title) and sends only changed pixels to the X server. (A plain
x86_64 `cargo build --release`, prebuilt std and all, is 798 KB.)

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
    --decoder WHICH  auto (mpg123 if it runs, else built in), mpg123, builtin
    --dsp DEVICE     play straight to an OSS device (e.g. /dev/dsp)
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
- The shade button hides the equalizer and playlist rather than rolling
  them up into window-shade mode. Hiding a window leaves a gap; the ones
  docked beyond it no longer follow the player until snapped back.
- The playlist is drawn in Terminus, not the font `pledit.txt` names.
- No mini-browser, video window, `gen.bmp` windows or skin cursors.
- A 486DX2 or DX4 cannot decode 44.1 kHz MP3 in real time with the
  built-in decoder (see Cost); half-rate decoding, as mpg123's `-2` does,
  is the likely remedy and is not done yet.
- Displayed time and the visualizer follow what has been sent to the sink,
  not the sound card's own latency.
- The built-in decoder does not trim the encoder's delay and padding (LAME
  records them; mpg123 removes them), so its track changes carry a few
  milliseconds of extra silence; it resamples linearly; its intensity
  stereo is ported but untested (LAME never writes it).
- Direct output is OSS only, and exclusive. Native ALSA would need far
  more `unsafe`; dmix needs alsa-lib. Its `ioctl`s are tested against a
  fake device and a non-device file only — no sound card was at hand.

## Development

See [AGENTS.md](AGENTS.md): the rules, the test gate, the test tiers.
protolith is a path dependency (`../protolith`).

## License

MIT or Apache-2.0, at your option. The binary embeds Terminus Font glyphs
(via protolith's `font-terminus-12`), which are OFL-1.1.
