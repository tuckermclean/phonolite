#!/bin/sh
# Check phonolite's own PulseAudio client against real servers, in
# throwaway Debian containers (podman): PulseAudio, and PipeWire's Pulse
# layer (with WirePlumber, and no cookie). Each plays a 1 s 440 Hz tone
# (examples/pulse-tone.rs) into a null sink while its monitor is recorded;
# the recording must hold the tone at the level sent.
#
#   tools/check-pulse.sh
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
work=$(mktemp -d)
chmod 755 "$work"  # the container's unprivileged user runs the player from it
trap 'rm -rf "$work"' EXIT
cargo build -q --manifest-path "$root/Cargo.toml" --example pulse-tone
cp "$root/target/debug/examples/pulse-tone" "$work/"
for server in pulseaudio pipewire; do
    if [ $server = pulseaudio ]; then
        packages="pulseaudio pulseaudio-utils"
        start='pulseaudio --daemonize -n --exit-idle-time=-1 --load=module-native-protocol-unix --load="module-null-sink sink_name=out"; sleep 1'
    else
        packages="pipewire pipewire-pulse wireplumber pulseaudio-utils dbus-daemon"
        start='dbus-daemon --session --address=unix:path=$XDG_RUNTIME_DIR/bus --fork >/dev/null; export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus; pipewire >/dev/null 2>&1 & sleep 1; wireplumber >/dev/null 2>&1 & sleep 1; pipewire-pulse >/dev/null 2>&1 & sleep 2; pactl load-module module-null-sink sink_name=out >/dev/null; sleep 1; rm -f ~/.config/pulse/cookie'
    fi
    cat > "$work/run.sh" <<SCRIPT
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null && apt-get install -y -qq $packages >/dev/null 2>&1
useradd -m u
cat > /home/u/play.sh <<'INNER'
export XDG_RUNTIME_DIR=/tmp/rt; mkdir -p \$XDG_RUNTIME_DIR; chmod 700 \$XDG_RUNTIME_DIR
$start
pactl set-default-sink out
parec -d out.monitor --format=s16le --rate=44100 --channels=2 --raw > /tmp/rec.raw & REC=\$!
sleep 0.5; /work/pulse-tone; sleep 0.5; kill \$REC
INNER
su u -s /bin/sh /home/u/play.sh
cp /tmp/rec.raw /work/rec-$server.raw
SCRIPT
    podman run --rm -v "$work:/work" debian:trixie sh /work/run.sh
    python3 - "$work/rec-$server.raw" "$server" <<'PY'
import cmath, math, struct, sys
d = open(sys.argv[1], "rb").read()
left = struct.unpack("<%dh" % (len(d) // 2), d)[0::2]
loud = [i for i, v in enumerate(left) if abs(v) > 1000]
assert loud, f"{sys.argv[2]}: silence"
seg = left[loud[0] + 4410 : loud[0] + 8820]
power = lambda f: abs(sum(x * cmath.exp(-2j * math.pi * f * i / 44100) for i, x in enumerate(seg)))
peak = max(map(abs, left))
assert max(range(300, 600, 10), key=power) == 440 and 7900 <= peak <= 8100, (sys.argv[2], peak)
print(f"{sys.argv[2]}: the tone arrived (440 Hz, peak {peak})")
PY
done
