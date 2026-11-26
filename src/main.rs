//! phonolite: a skinnable MP3 player for X11, a frontend to mpg123.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use phonolite::app::{App, Options};
use phonolite::engine::Mpg123;
use phonolite::player::{Decoding, Output, SinkCommand};

const HELP: &str = "\
phonolite — a skinnable MP3 player for X11, a frontend to mpg123

usage: phonolite [options] [file | folder | playlist | URL]...

  -s, --skin PATH      wear a classic skin (.wsz, or a folder of its files)
  -o, --output MODULE  out123 output module: alsa, pulse, oss, jack, ...
      --sink COMMAND   another sound sink (a command reading raw 16-bit
                       44.1 kHz stereo on stdin, e.g. \"aplay -q -f cd\")
      --mpg123 PATH    the mpg123 program to run
      --decoder WHICH  auto (default: mpg123 if it runs, else built in),
                       mpg123, or builtin
      --dsp DEVICE     play straight to an OSS device (e.g. /dev/dsp)
                       rather than choosing an output
      --fps N          animation frames per second (default 30)
      --rate HZ        decode at this sample rate (default 44100); on a
                       slow machine --rate 22050 --fps 10 halves the work
                       (a --sink must then expect that rate)
      --no-config      neither load nor save settings and playlist
  -h, --help           this text
  -V, --version        the version

Keys: z x c v b  previous, play, pause, stop, next;  l  add files;
j  jump to file;  arrows  seek and volume;  s r  shuffle, repeat;
Ctrl+D  double size;  Ctrl+A  always on top;  Alt+G Alt+E  equalizer,
playlist;  Ctrl+Q  quit.  Right-click for the menu.
";

fn parse() -> Result<Options, String> {
    let mut mpg123 = Mpg123::default();
    let mut decoder = String::from("auto");
    let mut dsp: Option<PathBuf> = None;
    let mut opts = Options {
        skin: None,
        decoding: Decoding::Builtin { rate: phonolite::audio::RATE },
        sink: Output::Oss("/dev/dsp".into()),
        fps: 30,
        files: Vec::new(),
        config_dir: phonolite::config::dir(),
    };
    let mut args = std::env::args_os().skip(1);
    let mut output: Option<String> = None;
    let mut sink: Option<String> = None;
    while let Some(a) = args.next() {
        let s = a.to_string_lossy().into_owned();
        let mut value = |name: &str| args.next().map(|v| v.to_string_lossy().into_owned()).ok_or(format!("{name} needs a value"));
        match s.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("phonolite {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-s" | "--skin" => opts.skin = Some(PathBuf::from(value(&s)?)),
            "-o" | "--output" => output = Some(value(&s)?),
            "--sink" => sink = Some(value(&s)?),
            "--mpg123" => mpg123.program = value(&s)?,
            "--decoder" => decoder = value(&s)?,
            "--dsp" => dsp = Some(PathBuf::from(value(&s)?)),
            "--fps" => opts.fps = value(&s)?.parse().map_err(|_| "--fps needs a number".to_string())?,
            "--rate" => mpg123.rate = value(&s)?.parse().ok().filter(|r| (8000..=192000).contains(r)).ok_or("--rate needs a sample rate in Hz")?,
            "--no-config" => opts.config_dir = None,
            "--" => opts.files.extend(args.by_ref().map(PathBuf::from)),
            _ if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option {s} (try --help)")),
            _ => opts.files.push(PathBuf::from(a)),
        }
    }
    let rate = mpg123.rate;
    opts.decoding = match decoder.as_str() {
        "auto" => Decoding::prefer_mpg123(mpg123),
        "mpg123" => Decoding::Mpg123(mpg123),
        "builtin" | "built-in" => Decoding::Builtin { rate },
        other => return Err(format!("--decoder takes auto, mpg123 or builtin, not {other}")),
    };
    opts.sink = if let Some(cmd) = sink {
        let mut words = cmd.split_whitespace().map(String::from);
        let program = words.next().ok_or("--sink needs a command")?;
        Output::Command(SinkCommand { program, args: words.collect() })
    } else if let Some(d) = dsp {
        Output::Oss(d)
    } else if let Some(o) = output {
        Output::Command(SinkCommand::out123(&["-o".into(), o], rate))
    } else {
        Output::choose(rate)
    };
    Ok(opts)
}

fn main() -> ExitCode {
    let opts = match parse() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("phonolite: {e}");
            return ExitCode::from(2);
        }
    };
    let conn = match protolith::x11::Connection::open() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("phonolite: cannot open the display: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut app = match App::new(conn, opts) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("phonolite: {e}");
            return ExitCode::FAILURE;
        }
    };
    while !app.quit {
        if let Err(e) = app.step() {
            eprintln!("phonolite: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
