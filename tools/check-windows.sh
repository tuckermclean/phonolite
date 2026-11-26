#!/bin/sh
# Check phonolite's detachable, snapping windows with a real pointer
# (xdotool, through XTEST) on Xvfb, with no window manager and under
# twm, openbox, IceWM, Fluxbox and xfwm4, in a throwaway Debian container
# (podman). Each run: the windows start stacked and docked; the
# equalizer is pulled away by its title bar, then dropped 5 px from the
# main window's right side (it must snap beside it, tops aligned); the
# main window is dragged and must carry the equalizer, leaving the
# playlist. Positions are read from the X server (xwininfo).
#
#   tools/check-windows.sh [ROUNDS]      (WMS="twm openbox" to pick)
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
rounds=${1:-3}
wms=${WMS:-none twm openbox icewm fluxbox xfwm4}
work=$(mktemp -d)
chmod 755 "$work"
trap 'rm -rf "$work"' EXIT
cargo build -q --release --manifest-path "$root/Cargo.toml"
cp "$root/target/release/phonolite" "$work/"
cat > "$work/drag.sh" <<'DRAG'
#!/bin/sh
# One run under window manager $1 ("none" for none) on display $2.
set -u
export DISPLAY=$2
Xvfb $2 -screen 0 1024x768x24 -nolisten tcp >/dev/null 2>&1 & pids=$!
trap 'kill $pids 2>/dev/null' EXIT
sleep 1
if [ "$1" != none ]; then
    $1 >/dev/null 2>&1 & pids="$pids $!"
    # Ready when it says so (EWMH); twm says nothing, so give it time.
    # And then some: openbox's first start in a fresh container announces
    # itself well before it handles clicks on windows it manages.
    for i in $(seq 1 15); do xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q "window id" && break; sleep 0.2; done
    sleep 3
fi
HOME=$(mktemp -d) /work/phonolite --no-config >/dev/null 2>&1 & pids="$pids $!"
sleep 3
pos() {
    xwininfo -name "$1" | awk '/Absolute upper-left X/ { x = $4 } /Absolute upper-left Y/ { y = $4 } END { print x "," y }'
}
all() { echo "$(pos phonolite) $(pos 'phonolite equalizer') $(pos 'phonolite playlist')"; }
# Press at ($4,$5) in window $1, move by ($2,$3) in steps, release. (The
# equalizer, at 74,468, moved by 580,-312, lands 5 px right of the main
# window and 4 px low: inside the 10 px snap.)
drag() {
    p=$(pos "$1"); x=${p%,*}; y=${p#*,}
    xdotool mousemove $((x + $4)) $((y + $5)) mousedown 1
    for k in 1 2 3 4 5 6; do xdotool mousemove $((x + $4 + $2 * k / 6)) $((y + $5 + $3 * k / 6)); sleep 0.05; done
    xdotool mouseup 1
    sleep 0.4
}
check() {
    got=$(all)
    [ "$got" = "$2" ] || { echo "$wm: $1: windows at $got, wanted $2"; exit 1; }
}
wm=$1
check "first run: stacked and docked in the middle" "374,152 374,268 374,384"
drag 'phonolite equalizer' -300 200 150 6
check "the equalizer pulled off" "374,152 74,468 374,384"
drag 'phonolite equalizer' 580 -312 150 6
check "the equalizer snapped beside the main window" "374,152 649,152 374,384"
drag phonolite -150 -100 150 6
check "the main window carried the equalizer" "224,52 499,52 374,384"
echo "$wm: ok"
DRAG
cat > "$work/run.sh" <<RUN
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null
apt-get install -y -qq xvfb xdotool x11-utils twm openbox icewm fluxbox xfwm4 >/dev/null 2>&1
n=10
for round in \$(seq 1 $rounds); do
    for wm in $wms; do
        n=\$((n + 1))
        timeout 60 sh /work/drag.sh \$wm :\$n || { echo "\$wm: FAILED (round \$round)"; exit 1; }
    done
done
RUN
podman run --rm -v "$work:/work" debian:trixie sh /work/run.sh
