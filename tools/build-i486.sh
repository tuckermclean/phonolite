#!/bin/sh
# Build the 486 release binary the way Stele builds its own: inside the
# monolith-builder image (the i486 musl cross toolchain lives only there),
# for targets/i486-monolith-linux-musl.json (copied from Stele: cpu i486,
# no MMX/SSE, x87 float, static musl), with std rebuilt from source and
# panics aborting at once.
#
#   tools/build-i486.sh            # from the repository root; needs podman
#   STRIP=false tools/build-i486.sh  # keep symbols, to see what costs what
#
# The binary lands in target-i486/i486-monolith-linux-musl/release/phonolite.
# protolith is mounted from ../protolith (the path dependency).
set -eu
IMAGE=ghcr.io/tuckermclean/monolith-builder@sha256:c8978fe3b56cf6ec1714b1b878997c7082cbf9786caa2b8cbcdbd986169c0daf
root="$(cd "$(dirname "$0")/.." && pwd)"
if [ "${INSIDE_BUILDER:-}" != 1 ]; then
    exec podman run --rm -e INSIDE_BUILDER=1 -e STRIP="${STRIP:-true}" \
        -v "$root:/w/phonolite" -v "$root/../protolith:/w/protolith:ro" \
        -w /w/phonolite "$IMAGE" sh tools/build-i486.sh
fi
# std names libunwind at link time, but with panics aborting nothing
# unwinds: an empty archive satisfies the name and pulls in nothing.
# (Stele links libgcc_eh here; that drags in its frame-table machinery,
# ~13 KB, through the C startup code. If anything ever does need an
# unwinder, this link fails rather than silently growing.)
mkdir -p /tmp/unwindshim
rm -f /tmp/unwindshim/libunwind.a && ar rcs /tmp/unwindshim/libunwind.a
# -Cpanic=immediate-abort: no panic messages, no formatting machinery for
# them. -Cforce-unwind-tables=no: nothing unwinds, so no .eh_frame tables.
export RUSTFLAGS="-L /tmp/unwindshim -Zunstable-options -Cpanic=immediate-abort -Cforce-unwind-tables=no"
export CARGO_TARGET_I486_MONOLITH_LINUX_MUSL_LINKER=i486-linux-musl-gcc
CARGO_PROFILE_RELEASE_STRIP="$STRIP" CARGO_TARGET_DIR=target-i486 cargo +nightly-2026-07-15 build --release \
    --target targets/i486-monolith-linux-musl.json -Zbuild-std=std,panic_abort -Zjson-target-spec
bin=target-i486/i486-monolith-linux-musl/release/phonolite
ls -l "$bin"
# The budget (stripped). It was 570 KB before the size pass that set it at
# 440 KB; the built-in MPEG decoder and OSS output (45 KB) raised it to 480 KB;
# raise it only with a reason in the commit.
budget=480000
size=$(wc -c < "$bin")
if [ "$STRIP" = true ] && [ "$size" -gt "$budget" ]; then
    echo "phonolite is $size bytes, over the $budget-byte budget" >&2
    exit 1
fi
