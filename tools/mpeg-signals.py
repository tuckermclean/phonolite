#!/usr/bin/env python3
"""Write the source signals for tools/mpeg-fixtures.sh as WAV files into
the directory given: chords with vibrato (music), clicks on silence (short
blocks), a sweep with noise (wide spectrum), mono bursts, and low-rate
versions for MPEG-2 and 2.5. All generated; deterministic."""
import math, random, struct, sys

out = sys.argv[1]

def wav(name, rate, chans, frames):
    data = b"".join(struct.pack("<" + "h" * chans, *[max(-32768, min(32767, int(v * 32767))) for v in f]) for f in frames)
    hdr = b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt " + struct.pack("<IHHIIHH", 16, 1, chans, rate, rate * chans * 2, chans * 2, 16) + b"data" + struct.pack("<I", len(data))
    open(f"{out}/{name}", "wb").write(hdr + data)

R = 44100
n = int(R * 0.5)

def music(i, r=R):
    t = i / r
    left = sum(0.12 * math.sin(2 * math.pi * f * t * (1 + 0.003 * math.sin(2 * math.pi * 5 * t))) for f in (220, 277.2, 329.6, 440, 880))
    right = sum(0.12 * math.sin(2 * math.pi * f * t) for f in (196, 246.9, 293.7, 587.3, 1174.7))
    return (left, right)

wav("music.wav", R, 2, [music(i) for i in range(n)])
rnd = random.Random(7)

def clicks(i):
    k = i % 4410
    v = (rnd.random() * 2 - 1) * 0.8 * math.exp(-k / 60) if k < 400 else 0.0
    return (v, -v * 0.5)

wav("clicks.wav", R, 2, [clicks(i) for i in range(n)])

def noise(i):
    t = i / R
    s = 0.3 * math.sin(2 * math.pi * (100 + 8000 * t) * t)
    return (s + 0.2 * (rnd.random() * 2 - 1), s * 0.7 + 0.2 * (rnd.random() * 2 - 1))

wav("noise.wav", R, 2, [noise(i) for i in range(n)])
wav("mono.wav", R, 1, [((0.5 * math.sin(2 * math.pi * 300 * i / R) * (1 if (i // 2205) % 2 == 0 else 0.2)),) for i in range(n)])
wav("music22.wav", 22050, 2, [music(i, 22050) for i in range(11025)])
wav("music8.wav", 8000, 1, [(music(i, 8000)[0],) for i in range(4000)])
