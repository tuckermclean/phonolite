#!/bin/sh
# Regenerate tests/data/mpeg/: signals made here (tools/mpeg-signals.py),
# encoded by LAME 3.100 and ffmpeg's MP2 encoder in a throwaway Alpine
# container (nothing is installed on the host). Run from the repository root:
#
#   tools/mpeg-fixtures.sh
#
# The encoders are not bit-exact across versions, so regenerated files may
# differ; the tests compare against mpg123's decode of whatever is there.
set -eu
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
python3 -I tools/mpeg-signals.py "$work"
cat > "$work/encode.sh" <<'SCRIPT'
set -e
apk add -q lame ffmpeg >/dev/null
cd /enc
lame -S -b 128 -m s  music.wav   l3-128-stereo.mp3
lame -S -b 128 -m j  music.wav   l3-128-joint.mp3
lame -S -b 320 -m j  noise.wav   l3-320-noise.mp3
lame -S -b 112 -m j  clicks.wav  l3-112-clicks.mp3
lame -S -V 4         noise.wav   l3-vbr-noise.mp3
lame -S -b 64  -m m  mono.wav    l3-64-mono.mp3
lame -S -b 48  -m j  music22.wav l3-mpeg2-22k.mp3
lame -S -b 16  -m m  music8.wav  l3-mpeg25-8k.mp3
lame -S -t -b 128 -m j music.wav l3-128-notag.mp3
ffmpeg -loglevel error -y -i music.wav   -c:a mp2 -b:a 192k l2-192-stereo.mp2
ffmpeg -loglevel error -y -i mono.wav    -c:a mp2 -b:a 64k  l2-64-mono.mp2
ffmpeg -loglevel error -y -i music22.wav -c:a mp2 -b:a 64k  l2-mpeg2-22k.mp2
SCRIPT
podman run --rm -v "$work:/enc" docker.io/library/alpine:3.20 sh /enc/encode.sh
mkdir -p tests/data/mpeg
cp "$work"/*.mp3 "$work"/*.mp2 tests/data/mpeg/
