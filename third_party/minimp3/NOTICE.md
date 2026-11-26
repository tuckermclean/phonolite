# minimp3

phonolite's built-in MPEG audio decoder (`src/mpeg/`) is a port to safe Rust
of minimp3 by Lion (lieff), https://github.com/lieff/minimp3, which its
author dedicated to the public domain under CC0 1.0 (`LICENSE` here).

Ported from `minimp3.h`, sha256
`eb4b44812cfe77a358aa1820462cc877473a8d97206009ebecd18b60a123875a`,
fetched 2026-11-25. The SIMD paths were left out (a 486 has none); the
tables in `src/mpeg/tables.rs` were extracted from it by script
(`tools/mpeg-tables.py`).
