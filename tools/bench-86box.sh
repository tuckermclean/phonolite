#!/bin/sh
# How fast does the built-in decoder run on period hardware? Boots The
# Monolith's i486 kernel in 86Box (cycle-timed emulation of real boards;
# times are the guest's own clock, so host load does not skew them) and
# decodes the MPEG fixtures there. Too slow for every commit (about two
# minutes per CPU): for when the question comes up.
#
#   MONOLITH_ISO=path/to/themonolith-*.iso tools/bench-86box.sh [CPU]...
#
# CPU: 486dx2-66 (default), 486dx4-100, 5x86-133, p24t-83 (Pentium
# OverDrive), or all. The board is an ASUS PCI/I-486SP3G (Award BIOS,
# 1995) with 32 MB; its CMOS settings (drive C detected, defaults saved)
# are tools/86box/486sp3g.nvr, made once in SETUP: the disk image must
# keep its geometry (32 MB: 65 cylinders, 16 heads, 63 sectors).
#
# Needs podman (the i486 builder image; a Debian image for the disk),
# Xvfb, and The Monolith ISO for its kernel and busybox initramfs.
# Downloads 86Box 6.0 and its ROM set into ~/.cache/86box (never into
# the repository).
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
: "${MONOLITH_ISO:?set MONOLITH_ISO to a Monolith ISO (its kernel and initramfs boot the guest)}"
cpus="${*:-486dx2-66}"
[ "$cpus" = all ] && cpus="486dx2-66 486dx4-100 5x86-133 p24t-83"
cache="$HOME/.cache/86box"
work=$(mktemp -d)
chmod 755 "$work"
xvfb_pid=
cleanup() {
    ps -eo pid,comm | awk '$2=="qt_thread"{print $1}' | xargs -r kill -9 2>/dev/null || true
    [ -n "$xvfb_pid" ] && kill "$xvfb_pid" 2>/dev/null || true
    rm -rf "$work"
}
trap cleanup EXIT

# 86Box and its ROMs.
mkdir -p "$cache"
if [ ! -x "$cache/squashfs-root/AppRun" ]; then
    curl -sfL -o "$cache/86Box.AppImage" https://github.com/86Box/86Box/releases/download/v6.0/86Box-Linux-x86_64-b9001.AppImage
    chmod +x "$cache/86Box.AppImage"
    (cd "$cache" && ./86Box.AppImage --appimage-extract >/dev/null)
fi
if [ ! -d "$cache/roms" ]; then
    curl -sfL -o "$cache/roms.zip" https://github.com/86Box/roms/archive/refs/tags/v6.0.zip
    python3 -c "import zipfile,sys; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" "$cache/roms.zip" "$cache"
    mv "$cache/roms-6.0" "$cache/roms" && rm "$cache/roms.zip"
fi

# The bench, for the i486.
IMAGE=ghcr.io/tuckermclean/monolith-builder@sha256:c8978fe3b56cf6ec1714b1b878997c7082cbf9786caa2b8cbcdbd986169c0daf
podman run --rm -v "$root:/w/phonolite" -v "$root/../protolith:/w/protolith:ro" -w /w/phonolite "$IMAGE" sh -c '
    mkdir -p /tmp/unwindshim && ar rcs /tmp/unwindshim/libunwind.a
    RUSTFLAGS="-L /tmp/unwindshim -Zunstable-options -Cpanic=immediate-abort -Cforce-unwind-tables=no" \
    CARGO_TARGET_I486_MONOLITH_LINUX_MUSL_LINKER=i486-linux-musl-gcc CARGO_TARGET_DIR=target-i486 \
    cargo +nightly-2026-07-15 build -q --release -p phonolite-mpeg --example bench \
        --target targets/i486-monolith-linux-musl.json -Zbuild-std=std,panic_abort -Zjson-target-spec'

# The guest disk: syslinux, the Monolith kernel, its initramfs plus ours;
# partition 2 (raw) takes the results.
mkdir -p "$work/overlay/bench-data"
cp "$root/target-i486/i486-monolith-linux-musl/release/examples/bench" "$root/tools/86box/bench-init" "$work/overlay/"
for f in l2-192-stereo.mp2 l3-128-joint.mp3 l3-320-noise.mp3 l3-64-mono.mp3 l3-mpeg2-22k.mp3; do
    cp "$root/tests/data/mpeg/$f" "$work/overlay/bench-data/"
done
cp "$MONOLITH_ISO" "$work/monolith.iso"
cat > "$work/mkdisk.sh" <<'DISK'
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null
apt-get install -y -qq syslinux syslinux-common mtools dosfstools fdisk cpio genisoimage >/dev/null 2>&1
cd /w
isoinfo -R -i monolith.iso -x /boot/vmlinuz > vmlinuz
isoinfo -R -i monolith.iso -x /boot/initrd.img > initrd.img
(cd overlay && find . | cpio -o -H newc 2>/dev/null) > overlay.cpio
# An appended archive must start on a 4-byte boundary.
cp initrd.img bench.img && truncate -s %512 bench.img && cat overlay.cpio >> bench.img
truncate -s 32M disk.img
printf 'label: dos\nstart=2048, size=55296, type=6, bootable\nstart=57344, type=da\n' | sfdisk -q disk.img
mkfs.fat -F 16 --offset 2048 -n BENCH disk.img 27648 >/dev/null 2>&1
printf 'DEFAULT bench\nPROMPT 0\nTIMEOUT 1\nLABEL bench\n  KERNEL vmlinuz\n  APPEND initrd=bench.img rdinit=/bench-init console=tty0 loglevel=4 bench_reps=20\n' > syslinux.cfg
for f in vmlinuz bench.img syslinux.cfg; do mcopy -i disk.img@@1M $f ::$f; done
syslinux --offset 1048576 --install disk.img
dd if=/usr/lib/syslinux/mbr/mbr.bin of=disk.img conv=notrunc bs=440 count=1 2>/dev/null
DISK
podman run --rm -v "$work:/w" debian:trixie sh /w/mkdisk.sh

# A display for 86Box's window.
Xvfb -displayfd 3 -screen 0 1024x768x24 -nolisten tcp 3>"$work/display" >/dev/null 2>&1 &
xvfb_pid=$!
while [ ! -s "$work/display" ]; do sleep 0.1; done
export DISPLAY=":$(cat "$work/display")"

for cpu in $cpus; do
    case $cpu in
        486dx2-66) set -- i486dx2 66666666 2 ;;
        486dx4-100) set -- am486dx4 100000000 3 ;;
        5x86-133) set -- am5x86 133333333 4 ;;
        p24t-83) set -- pentium_p24t 83333333 2.5 ;;
        *) echo "unknown CPU $cpu" >&2; exit 2 ;;
    esac
    vm="$work/vm-$cpu"
    mkdir -p "$vm/nvr"
    cp "$work/disk.img" "$vm/disk.img"
    cp "$root/tools/86box/486sp3g.nvr" "$vm/nvr/486sp3g.nvr"
    cat > "$vm/86box.cfg" <<CFG
[General]
vid_renderer = qt_software
confirm_exit = 0
confirm_reset = 0
confirm_save = 0

[Machine]
machine = 486sp3g
cpu_family = $1
cpu_speed = $2
cpu_multi = $3
cpu_use_dynarec = 1
fpu_type = internal
mem_size = 32768
time_sync = disabled

[Video]
gfxcard = et4000ax

[Storage controllers]
hdc_1 = internal

[Hard disks]
hdd_01_fn = disk.img
hdd_01_ide_channel = 0:0
hdd_01_parameters = 63, 16, 65, 0, ide
CFG
    (cd "$vm" && "$cache/squashfs-root/AppRun" -N -R "$cache/roms" -P "$vm" >/dev/null 2>&1 &)
    result() { dd if="$vm/disk.img" bs=512 skip=57344 count=64 2>/dev/null | tr -d '\0'; }
    t=0
    until result | grep -q "== END"; do
        sleep 5
        t=$((t + 5))
        [ $t -lt 900 ] || { echo "$cpu: no results after 15 minutes" >&2; exit 1; }
    done
    ps -eo pid,comm | awk '$2=="qt_thread"{print $1}' | xargs -r kill -9
    echo "== $cpu"
    result | awk '
        /^model name/ { sub(/^model name[ \t]*: /, ""); print "   " $0 }
        /^== / && !/cpu|END/ { name = $2 }
        /real time/ { printf "   %-20s %6.2fx real time  (%3.0f%% of the CPU)\n", name, $(NF-2), 100 / $(NF-2) }'
done
