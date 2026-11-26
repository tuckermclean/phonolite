#!/usr/bin/env python3
"""Regenerate src/mpeg/tables.rs from minimp3.h (CC0; see third_party/minimp3).

    python3 tools/mpeg-tables.py path/to/minimp3.h > src/mpeg/tables.rs

Tables are copied as written in the C source, never retyped; arrays the C
pads with zeros are padded here; g_deq_L12, which the C builds with a macro
of float divisions, is computed in float32 the same way.
"""
import hashlib, re, struct, sys

src = open(sys.argv[1]).read()
out = [
    "//! Tables of the MPEG audio decoder, extracted mechanically from minimp3",
    "//! (<https://github.com/lieff/minimp3>, public domain under CC0 1.0; see",
    "//! third_party/minimp3/). Source `minimp3.h` sha256:",
    "//! " + hashlib.sha256(src.encode()).hexdigest() + ".",
    "//! Regenerate with tools/mpeg-tables.py; do not edit by hand.",
    "",
    "#![allow(clippy::excessive_precision, clippy::unreadable_literal)]",
    "",
]

def body(name):
    m = re.search(r"\b" + re.escape(name) + r"\s*(\[[^=]*\])\s*=\s*\{(.*?)\};", src, re.S)
    assert m, name
    return m.group(2)

def nums(text):
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.findall(r"-?[0-9][0-9.e+-]*f?", text.replace("{", " ").replace("}", " "))

def lit(v, ty):
    v = v.rstrip("f")
    return v + ".0" if ty == "f32" and "." not in v and "e" not in v else v

def emit1(rust, ty, values):
    out.append(f"pub(crate) static {rust}: [{ty}; {len(values)}] = [{', '.join(lit(v, ty) for v in values)}];")

def emit2(rust, ty, name, rows, width):
    groups = re.findall(r"\{([^{}]*)\}", body(name))
    assert len(groups) == rows, name
    out.append(f"pub(crate) static {rust}: [[{ty}; {width}]; {rows}] = [")
    for g in groups:
        v = [lit(x, ty) for x in nums(g)]
        v += ["0"] * (width - len(v))
        assert len(v) == width, name
        out.append("    [" + ", ".join(v) + "],")
    out.append("];")

def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]

v = nums(body("halfrate"))
assert len(v) == 90
out.append("pub(crate) static HALFRATE: [[[u8; 15]; 3]; 2] = [")
for ver in range(2):
    out.append("    [" + ", ".join("[" + ", ".join(v[ver * 45 + l * 15 : ver * 45 + l * 15 + 15]) + "]" for l in range(3)) + "],")
out.append("];")
emit1("BITALLOC_CODE_TAB", "u8", nums(body("g_bitalloc_code_tab")))
deq = [repr(f32(f32(c) / x)) for x in [3, 7, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767, 65535, 3, 5, 9] for c in (9.53674316e-07, 7.56931807e-07, 6.00777173e-07)]
emit1("DEQ_L12", "f32", deq)
emit2("SCF_LONG", "u8", "g_scf_long", 8, 23)
emit2("SCF_SHORT", "u8", "g_scf_short", 8, 40)
emit2("SCF_MIXED", "u8", "g_scf_mixed", 8, 40)
emit1("SCFC_DECODE", "u8", nums(body("g_scfc_decode")))
emit1("MOD", "u8", nums(body("g_mod")))
emit2("SCF_PARTITIONS", "u8", "g_scf_partitions", 3, 28)
emit1("PREAMP", "u8", nums(body("g_preamp")))
emit1("EXPFRAC", "f32", nums(body("g_expfrac")))
emit1("POW43", "f32", nums(body("g_pow43")))
emit1("HUFF_TABS", "i16", nums(body("tabs")))
emit1("TAB32", "u8", nums(body("tab32")))
emit1("TAB33", "u8", nums(body("tab33")))
emit1("TABINDEX", "i16", nums(body("tabindex")))
emit1("LINBITS", "u8", nums(body("g_linbits")))
emit1("PAN", "f32", nums(body("g_pan")))
emit2("AA", "f32", "g_aa", 2, 8)
emit1("TWID9", "f32", nums(body("g_twid9")))
emit1("TWID3", "f32", nums(body("g_twid3")))
emit2("MDCT_WINDOW", "f32", "g_mdct_window", 2, 18)
emit1("SEC", "f32", nums(body("g_sec")))
emit1("SYNTH_WIN", "f32", nums(body("g_win")))
print("\n".join(out))
