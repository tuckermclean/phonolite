//! # phonolite
//!
//! A skinnable MP3 player for X11: a frontend to mpg123 that wears classic
//! 2.x skins (`.wsz`), drawn with [protolith](https://github.com/tuckermclean/protolith).
//!
//! The binary is the product; the library exists so every piece can be
//! tested on its own:
//!
//! - [`remote`] / [`engine`] / [`audio`] / [`player`] — mpg123's remote
//!   protocol, the decoder process, the pump that carries its samples to
//!   the sound sink, and playback on top. One decoder, one pass over the
//!   audio: it has to run on a 486.
//! - [`eq`] — the 10-band equalizer mapped onto mpg123's 32 subbands.
//! - [`playlist`] — entries, shuffle/repeat, M3U and PLS.
//! - [`zip`] / [`inflate`] — reading `.wsz` skin archives.
//! - [`skin`] — classic skins, their geometry, and the built-in skin.
//! - [`vis`] — the spectrum analyzer and oscilloscope, fed by the samples
//!   on their way to the sink.
//! - [`app`] / [`config`] — the windows and the settings kept between runs.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms, unreachable_pub)]

pub mod app;
pub mod audio;
pub mod config;
pub mod engine;
pub mod eq;
pub mod inflate;
pub mod num;
pub mod player;
pub mod playlist;
pub mod remote;
pub mod skin;
#[doc(hidden)]
pub mod testsignal;
pub mod vis;
pub mod zip;
