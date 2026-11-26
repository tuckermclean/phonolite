//! Decode a file N times and report the speed as a multiple of real time.
//!
//!     cargo run --release -p phonolite-mpeg --example bench -- FILE [N]

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("a file");
    let reps: usize = args.next().map_or(100, |n| n.parse().expect("a count"));
    let data = std::fs::read(&path).expect("readable");
    let start = std::time::Instant::now();
    let (mut seconds, mut hz) = (0.0, 0);
    for _ in 0..reps {
        let (pcm, info) = phonolite_mpeg::decode_all(&data);
        hz = info.hz;
        seconds += pcm.len() as f64 / info.channels.max(1) as f64 / f64::from(info.hz.max(1));
    }
    let took = start.elapsed().as_secs_f64();
    println!("{seconds:.1} s of {hz} Hz audio in {took:.3} s: {:.2}x real time", seconds / took);
}
