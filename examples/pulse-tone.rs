//! Play one second of a 440 Hz tone through phonolite's own PulseAudio
//! client (to check it against a real server).
//!
//!     cargo run --example pulse-tone [-- /path/to/pulse/native]

use std::io::Write;

fn main() -> std::io::Result<()> {
    let path = match std::env::args().nth(1) {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            let server = std::env::var("PULSE_SERVER").ok();
            let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
            phonolite::pulse::server_path(server.as_deref(), runtime.as_deref()).expect("no PulseAudio server named")
        }
    };
    let (control, mut w) = phonolite::pulse::open(&path, 44100)?;
    let mut pcm = Vec::with_capacity(44100 * 4);
    for i in 0..44100u32 {
        let v = (8000.0 * (2.0 * std::f64::consts::PI * 440.0 * f64::from(i) / 44100.0).sin()) as i16;
        pcm.extend(v.to_le_bytes());
        pcm.extend(v.to_le_bytes());
    }
    w.write_all(&pcm)?;
    drop(w);
    control.finish();
    // finish() drains in the background; give it the buffer's length.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    eprintln!("played 1 s through {}", path.display());
    Ok(())
}
