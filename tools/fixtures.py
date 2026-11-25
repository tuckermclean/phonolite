#!/usr/bin/env python3
"""Regenerate tests/data/*.deflate and *.zip: deterministic sample data
compressed by Python's zlib (an independent DEFLATE implementation), so the
inflater is tested against a compressor that isn't ours.

The sample generator matches `sample()` in src/inflate.rs; only the
compressed side is committed. Run from the repository root:

    python3 tools/fixtures.py
"""
import io, os, zipfile, zlib

WORDS = ["skin", "llama", "phonolite", "the", "a", "of", "bitmap", "playlist", "equalizer", "volume", "\n", "seek"]

def sample(kind, n):
    x = 0x12345678
    def nxt():
        nonlocal x
        x = (x * 6364136223846793005 + 1442695040888963407) % 2**64
        return x >> 56
    out = bytearray()
    while len(out) < n:
        if kind == "text":
            out += WORDS[nxt() % len(WORDS)].encode() + b" "
        elif kind == "random":
            out.append(nxt())
        else:
            v, ln = nxt(), nxt() * 3
            out += bytes([v]) * ln
    return bytes(out[:n])

def deflate(data, level, strategy=zlib.Z_DEFAULT_STRATEGY):
    c = zlib.compressobj(level, zlib.DEFLATED, -15, 9, strategy)
    return c.compress(data) + c.flush()

d = "tests/data"
os.makedirs(d, exist_ok=True)
for name, kind, n, level, strat in [
    ("text-l9", "text", 40000, 9, zlib.Z_DEFAULT_STRATEGY),
    ("text-l0", "text", 10000, 0, zlib.Z_DEFAULT_STRATEGY),
    ("text-l1", "text", 40000, 1, zlib.Z_DEFAULT_STRATEGY),
    ("random-l6", "random", 5000, 6, zlib.Z_DEFAULT_STRATEGY),
    ("runs-l9", "runs", 70000, 9, zlib.Z_DEFAULT_STRATEGY),
    ("short-fixed", "text", 60, 9, zlib.Z_FIXED),
    ("empty", "text", 0, 9, zlib.Z_DEFAULT_STRATEGY),
]:
    open(f"{d}/{name}.deflate", "wb").write(deflate(sample(kind, n), level, strat))

# A skin-shaped archive: a folder inside, mixed-case names, both methods.
buf = io.BytesIO()
with zipfile.ZipFile(buf, "w") as z:
    z.writestr(zipfile.ZipInfo("MySkin/"), b"")
    z.writestr("MySkin/Main.BMP", sample("runs", 20000), compress_type=zipfile.ZIP_DEFLATED)
    z.writestr("MySkin/viscolor.txt", sample("text", 300), compress_type=zipfile.ZIP_STORED)
    z.writestr("readme.txt", b"made by tools/fixtures.py\n", compress_type=zipfile.ZIP_DEFLATED)
open(f"{d}/skin.zip", "wb").write(buf.getvalue())
