//! Reading ZIP archives (`.wsz` skins are ZIP files): the central
//! directory, stored and deflated members, CRC-checked.

use std::fmt;

use crate::inflate::{crc32, inflate, InflateError};

/// The most a single member may expand to (a skin bitmap is well under a
/// megabyte; this stops a "zip bomb" from eating memory).
pub const MAX_MEMBER: usize = 32 << 20;

/// Why an archive or member could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZipError {
    /// Not a ZIP archive (no end-of-central-directory record).
    NotZip,
    /// Damaged (the reason says how).
    Invalid(&'static str),
    /// A feature skins never use: ZIP64, encryption, other compression.
    Unsupported(&'static str),
    /// The member's data does not inflate.
    Inflate(InflateError),
}

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZipError::NotZip => f.write_str("not a ZIP archive"),
            ZipError::Invalid(why) => write!(f, "damaged ZIP archive: {why}"),
            ZipError::Unsupported(what) => write!(f, "unsupported ZIP feature: {what}"),
            ZipError::Inflate(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for ZipError {}

/// One member of an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The path inside the archive, `/`-separated.
    pub name: String,
    method: u16,
    flags: u16,
    crc: u32,
    compressed: usize,
    size: usize,
    offset: usize,
}

impl Member {
    /// A directory entry.
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }

    /// The last path component.
    pub fn file_name(&self) -> &str {
        self.name.trim_end_matches('/').rsplit(['/', '\\']).next().unwrap_or("")
    }
}

/// A parsed archive over its bytes.
#[derive(Debug)]
pub struct Archive<'a> {
    data: &'a [u8],
    /// The members, in central-directory order.
    pub members: Vec<Member>,
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

impl<'a> Archive<'a> {
    /// Read the central directory.
    pub fn parse(data: &'a [u8]) -> Result<Archive<'a>, ZipError> {
        // The end record is 22 bytes plus a comment of up to 64 KiB.
        let lowest = data.len().saturating_sub(22 + 0xFFFF);
        let eocd = (lowest..=data.len().saturating_sub(22)).rev().find(|&i| u32_at(data, i) == Some(0x0605_4b50)).ok_or(ZipError::NotZip)?;
        let bad = ZipError::Invalid("truncated directory");
        let count = usize::from(u16_at(data, eocd + 10).ok_or(bad.clone())?);
        let dir_offset = u32_at(data, eocd + 16).ok_or(bad.clone())?;
        if dir_offset == u32::MAX || count == 0xFFFF {
            return Err(ZipError::Unsupported("ZIP64"));
        }
        let mut members = Vec::with_capacity(count);
        let mut p = dir_offset as usize;
        for _ in 0..count {
            if u32_at(data, p) != Some(0x0201_4b50) {
                return Err(ZipError::Invalid("bad central directory entry"));
            }
            let field = |o: usize| u32_at(data, p + o).ok_or(bad.clone());
            let half = |o: usize| u16_at(data, p + o).ok_or(bad.clone());
            let (flags, method) = (half(8)?, half(10)?);
            let (crc, compressed, size) = (field(16)?, field(20)?, field(24)?);
            let (name_len, extra_len, comment_len) = (usize::from(half(28)?), usize::from(half(30)?), usize::from(half(32)?));
            let offset = field(42)?;
            let name = data.get(p + 46..p + 46 + name_len).ok_or(bad.clone())?;
            members.push(Member {
                name: String::from_utf8_lossy(name).into_owned(),
                method,
                flags,
                crc,
                compressed: compressed as usize,
                size: size as usize,
                offset: offset as usize,
            });
            p += 46 + name_len + extra_len + comment_len;
        }
        Ok(Archive { data, members })
    }

    /// The first file (not directory) whose last path component matches
    /// `name`, ignoring ASCII case — skins are made on Windows, where
    /// `Main.BMP` and `main.bmp` are one file, and are often zipped inside
    /// a folder.
    pub fn find(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|m| !m.is_dir() && m.file_name().eq_ignore_ascii_case(name))
    }

    /// A member's contents.
    pub fn read(&self, m: &Member) -> Result<Vec<u8>, ZipError> {
        if m.flags & 1 != 0 {
            return Err(ZipError::Unsupported("encryption"));
        }
        if m.size > MAX_MEMBER {
            return Err(ZipError::Inflate(InflateError::TooLarge));
        }
        let bad = ZipError::Invalid("truncated member");
        if u32_at(self.data, m.offset) != Some(0x0403_4b50) {
            return Err(ZipError::Invalid("bad local header"));
        }
        // The local header's name/extra lengths can differ from the directory's.
        let start = m.offset
            + 30
            + usize::from(u16_at(self.data, m.offset + 26).ok_or(bad.clone())?)
            + usize::from(u16_at(self.data, m.offset + 28).ok_or(bad.clone())?);
        let body = self.data.get(start..start + m.compressed).ok_or(bad)?;
        let out = match m.method {
            0 => body.to_vec(),
            8 => inflate(body, m.size).map_err(ZipError::Inflate)?.0,
            _ => return Err(ZipError::Unsupported("a compression method other than deflate")),
        };
        if out.len() != m.size {
            return Err(ZipError::Invalid("size mismatch"));
        }
        if crc32(&out) != m.crc {
            return Err(ZipError::Invalid("CRC mismatch"));
        }
        Ok(out)
    }
}

/// Build a ZIP of stored (uncompressed) members — for tests, and for
/// anyone wanting to pack a skin without a zip tool.
pub fn store(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut dir = Vec::new();
    for (name, data) in files {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let header = |sig: u32| {
            let mut h = sig.to_le_bytes().to_vec();
            if sig == 0x0201_4b50 {
                h.extend(20u16.to_le_bytes()); // made by
            }
            h.extend(20u16.to_le_bytes()); // needed
            h.extend(0u16.to_le_bytes()); // flags
            h.extend(0u16.to_le_bytes()); // stored
            h.extend(0u32.to_le_bytes()); // time, date
            h.extend(crc.to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes()); // extra
            h
        };
        out.extend(header(0x0403_4b50));
        out.extend(name.as_bytes());
        out.extend(*data);
        let mut c = header(0x0201_4b50);
        c.extend([0; 8]); // comment len, disk, internal attrs, external attrs (first 2)
        c.extend([0; 2]);
        c.extend(offset.to_le_bytes());
        c.extend(name.as_bytes());
        dir.extend(c);
    }
    let dir_offset = out.len() as u32;
    out.extend(&dir);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((dir.len() as u32).to_le_bytes());
    out.extend(dir_offset.to_le_bytes());
    out.extend([0; 2]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inflate::tests::sample;

    #[test]
    fn reads_a_python_made_archive() {
        let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/skin.zip")).unwrap();
        let z = Archive::parse(&data).unwrap();
        let names: Vec<&str> = z.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["MySkin/", "MySkin/Main.BMP", "MySkin/viscolor.txt", "readme.txt"]);
        assert!(z.find("myskin").is_none(), "directories are not files");
        assert_eq!(z.read(z.find("main.bmp").unwrap()).unwrap(), sample("runs", 20000), "deflated");
        assert_eq!(z.read(z.find("VISCOLOR.TXT").unwrap()).unwrap(), sample("text", 300), "stored");
        assert_eq!(z.read(z.find("readme.txt").unwrap()).unwrap(), b"made by tools/fixtures.py\n");
        assert!(z.find("pledit.txt").is_none());
    }

    #[test]
    fn our_stored_archives_read_back() {
        let z = store(&[("a/B.txt", b"hello"), ("c", b"")]);
        let a = Archive::parse(&z).unwrap();
        assert_eq!(a.read(a.find("b.TXT").unwrap()).unwrap(), b"hello");
        assert_eq!(a.read(a.find("c").unwrap()).unwrap(), b"");
        assert_eq!(a.members[0].file_name(), "B.txt");
    }

    #[test]
    fn damage_is_detected() {
        assert_eq!(Archive::parse(b"not a zip").unwrap_err(), ZipError::NotZip);
        assert_eq!(Archive::parse(b"").unwrap_err(), ZipError::NotZip);
        let mut z = store(&[("x", b"hello")]);
        z[30 + 1 + 1] ^= 0xff; // a byte of the data
        let a = Archive::parse(&z).unwrap();
        assert_eq!(a.read(&a.members[0]), Err(ZipError::Invalid("CRC mismatch")));
        let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/skin.zip")).unwrap();
        for cut in 0..data.len() {
            if let Ok(a) = Archive::parse(&data[..cut]) {
                for m in &a.members {
                    let _ = a.read(m);
                }
            }
        }
        for i in 0..data.len() {
            let mut bad = data.clone();
            bad[i] ^= 0x55;
            if let Ok(a) = Archive::parse(&bad) {
                for m in &a.members {
                    let _ = a.read(m);
                }
            }
        }
    }
}
