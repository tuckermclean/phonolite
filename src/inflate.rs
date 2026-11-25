//! DEFLATE decompression (RFC 1951), for `.wsz` skins (ZIP archives).
//!
//! Canonical-Huffman decoding one bit at a time, after Mark Adler's
//! `puff.c`: small and obviously correct rather than fast — a skin is a
//! few hundred kilobytes.

use std::fmt;

/// Why a stream could not be inflated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InflateError {
    /// The input ended mid-stream.
    Truncated,
    /// The stream is malformed (the reason says how).
    Invalid(&'static str),
    /// The output would exceed the caller's limit.
    TooLarge,
}

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InflateError::Truncated => f.write_str("compressed data is truncated"),
            InflateError::Invalid(why) => write!(f, "compressed data is invalid: {why}"),
            InflateError::TooLarge => f.write_str("compressed data expands past the size limit"),
        }
    }
}

impl std::error::Error for InflateError {}

const MAX_BITS: usize = 15;

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit_buf: u32,
    bit_count: u32,
}

impl Bits<'_> {
    fn bits(&mut self, need: u32) -> Result<u32, InflateError> {
        let mut val = self.bit_buf;
        while self.bit_count < need {
            let b = *self.data.get(self.pos).ok_or(InflateError::Truncated)?;
            self.pos += 1;
            val |= u32::from(b) << self.bit_count;
            self.bit_count += 8;
        }
        self.bit_buf = if need >= 32 { 0 } else { val >> need };
        self.bit_count -= need;
        Ok(if need >= 32 { val } else { val & ((1 << need) - 1) })
    }

    fn align(&mut self) {
        self.bit_buf = 0;
        self.bit_count = 0;
    }
}

struct Huffman {
    count: [u16; MAX_BITS + 1],
    symbol: Vec<u16>,
}

impl Huffman {
    /// Build from code lengths. Incomplete codes are allowed only where
    /// RFC 1951 allows them (a single distance code); over-subscribed
    /// ones never are.
    fn new(lengths: &[u8]) -> Result<Huffman, InflateError> {
        let mut count = [0u16; MAX_BITS + 1];
        for &l in lengths {
            count[usize::from(l)] += 1;
        }
        if usize::from(count[0]) == lengths.len() {
            return Ok(Huffman { count, symbol: Vec::new() });
        }
        let mut left: i32 = 1;
        for &c in &count[1..] {
            left = left * 2 - i32::from(c);
            if left < 0 {
                return Err(InflateError::Invalid("over-subscribed Huffman code"));
            }
        }
        let mut offs = [0u16; MAX_BITS + 2];
        for len in 1..=MAX_BITS {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[usize::from(offs[usize::from(l)])] = sym as u16;
                offs[usize::from(l)] += 1;
            }
        }
        Ok(Huffman { count, symbol })
    }

    fn decode(&self, b: &mut Bits<'_>) -> Result<u16, InflateError> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..=MAX_BITS {
            code |= b.bits(1)? as i32;
            let count = i32::from(self.count[len]);
            if code - count < first {
                return Ok(self.symbol[(index + (code - first)) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(InflateError::Invalid("no such Huffman code"))
    }
}

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] =
    [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// Inflate a raw DEFLATE stream (no zlib or gzip wrapper), refusing to
/// produce more than `limit` bytes. Returns the output and the number of
/// input bytes consumed.
pub fn inflate(data: &[u8], limit: usize) -> Result<(Vec<u8>, usize), InflateError> {
    let mut b = Bits { data, pos: 0, bit_buf: 0, bit_count: 0 };
    let mut out = Vec::new();
    loop {
        let last = b.bits(1)? == 1;
        match b.bits(2)? {
            0 => stored(&mut b, &mut out, limit)?,
            1 => {
                let (lit, dist) = fixed_tables();
                codes(&mut b, &mut out, &lit, &dist, limit)?;
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut b)?;
                codes(&mut b, &mut out, &lit, &dist, limit)?;
            }
            _ => return Err(InflateError::Invalid("block type 3")),
        }
        if last {
            return Ok((out, b.pos));
        }
    }
}

fn stored(b: &mut Bits<'_>, out: &mut Vec<u8>, limit: usize) -> Result<(), InflateError> {
    b.align();
    let header = b.data.get(b.pos..b.pos + 4).ok_or(InflateError::Truncated)?;
    let len = u16::from_le_bytes([header[0], header[1]]);
    let nlen = u16::from_le_bytes([header[2], header[3]]);
    if len != !nlen {
        return Err(InflateError::Invalid("stored block length check"));
    }
    b.pos += 4;
    let body = b.data.get(b.pos..b.pos + usize::from(len)).ok_or(InflateError::Truncated)?;
    if out.len() + body.len() > limit {
        return Err(InflateError::TooLarge);
    }
    out.extend_from_slice(body);
    b.pos += usize::from(len);
    Ok(())
}

fn fixed_tables() -> (Huffman, Huffman) {
    let mut l = [0u8; 288];
    l[..144].fill(8);
    l[144..256].fill(9);
    l[256..280].fill(7);
    l[280..].fill(8);
    (Huffman::new(&l).expect("fixed code is complete"), Huffman::new(&[5; 30]).expect("fixed code is valid"))
}

fn dynamic_tables(b: &mut Bits<'_>) -> Result<(Huffman, Huffman), InflateError> {
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let nlen = b.bits(5)? as usize + 257;
    let ndist = b.bits(5)? as usize + 1;
    let ncode = b.bits(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(InflateError::Invalid("too many length or distance codes"));
    }
    let mut lengths = [0u8; 320];
    for &o in &ORDER[..ncode] {
        lengths[o] = b.bits(3)? as u8;
    }
    let lencode = Huffman::new(&lengths[..19])?;
    if lencode.symbol.is_empty() || !complete(&lencode) {
        return Err(InflateError::Invalid("incomplete code-length code"));
    }
    let mut i = 0;
    while i < nlen + ndist {
        let sym = lencode.decode(b)?;
        if sym < 16 {
            lengths[i] = sym as u8;
            i += 1;
            continue;
        }
        let (value, repeat) = match sym {
            16 => {
                if i == 0 {
                    return Err(InflateError::Invalid("repeat with no previous length"));
                }
                (lengths[i - 1], 3 + b.bits(2)? as usize)
            }
            17 => (0, 3 + b.bits(3)? as usize),
            _ => (0, 11 + b.bits(7)? as usize),
        };
        if i + repeat > nlen + ndist {
            return Err(InflateError::Invalid("code lengths overrun"));
        }
        lengths[i..i + repeat].fill(value);
        i += repeat;
    }
    if lengths[256] == 0 {
        return Err(InflateError::Invalid("no end-of-block code"));
    }
    let lit = Huffman::new(&lengths[..nlen])?;
    if !complete(&lit) && lit.count[1..].iter().map(|&c| u32::from(c)).sum::<u32>() != 1 {
        return Err(InflateError::Invalid("incomplete literal/length code"));
    }
    let dist = Huffman::new(&lengths[nlen..nlen + ndist])?;
    let used: u32 = dist.count[1..].iter().map(|&c| u32::from(c)).sum();
    if used > 1 && !complete(&dist) {
        return Err(InflateError::Invalid("incomplete distance code"));
    }
    Ok((lit, dist))
}

fn complete(h: &Huffman) -> bool {
    let mut left: i32 = 1;
    for &c in &h.count[1..] {
        left = left * 2 - i32::from(c);
    }
    left == 0
}

fn codes(b: &mut Bits<'_>, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman, limit: usize) -> Result<(), InflateError> {
    loop {
        let sym = lit.decode(b)?;
        match sym {
            0..=255 => {
                if out.len() >= limit {
                    return Err(InflateError::TooLarge);
                }
                out.push(sym as u8);
            }
            256 => return Ok(()),
            257..=285 => {
                let s = usize::from(sym - 257);
                let len = usize::from(LEN_BASE[s]) + b.bits(u32::from(LEN_EXTRA[s]))? as usize;
                let d = usize::from(dist.decode(b)?);
                if d >= 30 {
                    return Err(InflateError::Invalid("distance code 30 or 31"));
                }
                let distance = usize::from(DIST_BASE[d]) + b.bits(u32::from(DIST_EXTRA[d]))? as usize;
                if distance > out.len() {
                    return Err(InflateError::Invalid("distance before the start"));
                }
                if out.len() + len > limit {
                    return Err(InflateError::TooLarge);
                }
                let start = out.len() - distance;
                for k in 0..len {
                    out.push(out[start + k]);
                }
            }
            _ => return Err(InflateError::Invalid("length code 286 or 287")),
        }
    }
}

/// CRC-32 (IEEE, as in ZIP and PNG).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The deterministic sample data `tools/fixtures.py` compressed with
    /// Python's zlib, regenerated here so only the compressed side is in
    /// the repository.
    pub(crate) fn sample(kind: &str, n: usize) -> Vec<u8> {
        let mut x: u64 = 0x1234_5678;
        let mut next = move || {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (x >> 56) as u8
        };
        const WORDS: [&str; 12] = ["skin", "llama", "phonolite", "the", "a", "of", "bitmap", "playlist", "equalizer", "volume", "\n", "seek"];
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            match kind {
                "text" => {
                    out.extend(WORDS[usize::from(next()) % WORDS.len()].as_bytes());
                    out.push(b' ');
                }
                "random" => out.push(next()),
                _ => {
                    let (v, len) = (next(), usize::from(next()) * 3);
                    out.extend(std::iter::repeat(v).take(len));
                }
            }
        }
        out.truncate(n);
        out
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn zlib_streams_of_every_block_type() {
        // name, kind, size: see tools/fixtures.py.
        for (name, kind, n) in [
            ("text-l9.deflate", "text", 40000),
            ("text-l0.deflate", "text", 10000),
            ("text-l1.deflate", "text", 40000),
            ("random-l6.deflate", "random", 5000),
            ("runs-l9.deflate", "runs", 70000),
            ("short-fixed.deflate", "text", 60),
            ("empty.deflate", "text", 0),
        ] {
            let data = fixture(name);
            let (out, used) = inflate(&data, 1 << 20).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(out, sample(kind, n), "{name}");
            assert_eq!(used, data.len(), "{name}: consumed all");
        }
    }

    #[test]
    fn block_types_are_what_the_fixtures_claim() {
        // Bits 1-2 of the first byte: 0 stored, 1 fixed, 2 dynamic.
        assert_eq!((fixture("text-l0.deflate")[0] >> 1) & 3, 0);
        assert_eq!((fixture("short-fixed.deflate")[0] >> 1) & 3, 1);
        assert_eq!((fixture("text-l9.deflate")[0] >> 1) & 3, 2);
    }

    #[test]
    fn damage_is_an_error_never_a_panic() {
        let good = fixture("text-l9.deflate");
        for cut in (0..good.len()).step_by(97) {
            assert!(inflate(&good[..cut], 1 << 20).is_err(), "cut at {cut}");
        }
        let mut x = 7u32;
        for _ in 0..300 {
            let mut bad = good.clone();
            for _ in 0..4 {
                x = x.wrapping_mul(1103515245).wrapping_add(12345);
                let i = (x as usize >> 8) % bad.len();
                bad[i] ^= 1 << (x % 8);
            }
            let _ = inflate(&bad, 1 << 20);
        }
        assert_eq!(inflate(&[0b111], 10), Err(InflateError::Invalid("block type 3")));
        assert_eq!(inflate(&[1, 5, 0, 0xfb, 0xff], 10), Err(InflateError::Invalid("stored block length check")));
        assert_eq!(inflate(&fixture("runs-l9.deflate"), 1000), Err(InflateError::TooLarge));
        // A fixed block whose first code is a match: distance 1 with no output.
        // Bits: BFINAL 1, BTYPE 01, then length code 257 (7 bits 0000001), distance code 0 (5 bits 00000).
        assert_eq!(inflate(&[0b0000_0011, 0b0000_0010, 0], 10), Err(InflateError::Invalid("distance before the start")));
    }

    #[test]
    fn crc() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
