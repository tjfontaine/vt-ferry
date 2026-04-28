#!/bin/sh
#
# Run inside the vt-ferry build container (see ffmpeg/Dockerfile,
# `builder` stage). Compiles the guest shim cdylib for the
# native (Linux aarch64) target, stages it under
# `artifacts/ffmpeg-shim-libs/linux-debug/` with the libCore* /
# libVideoToolbox symlinks the patched FFmpeg expects, applies
# the vt-ferry patch to the cloned FFmpeg source, and configures
# + builds the patched FFmpeg.
#
# Usage (called from Dockerfile):
#   build_inside_container.sh <FFMPEG_REF>
#
# Outside a Docker container this script will still work as long
# as the host is aarch64 Linux with the same toolchain available;
# from macOS use `build_vt_ferry_image.sh` which drives the full
# image build.

set -eu

FFMPEG_REF="${1:-n8.1}"
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

echo "==> Building guest shim (native)"
sh ffmpeg/scripts/stage_guest_shim_libs.sh \
    debug \
    artifacts/ffmpeg-shim-libs/linux-debug \
    native

echo "==> Applying vt-ferry FFmpeg patch (${FFMPEG_REF})"
sh ffmpeg/scripts/prepare_linux_videotoolbox_tree.sh "${FFMPEG_REF}"

FFMPEG_BUILD_DIR="artifacts/ffmpeg-build/${FFMPEG_REF}-linux-debug"

echo "==> Mirroring patched source into ${FFMPEG_BUILD_DIR}"
rm -rf "${FFMPEG_BUILD_DIR}"
mkdir -p "$(dirname "${FFMPEG_BUILD_DIR}")"
cp -r "artifacts/ffmpeg-source/${FFMPEG_REF}/FFmpeg" "${FFMPEG_BUILD_DIR}"

echo "==> Configuring FFmpeg"
cd "${FFMPEG_BUILD_DIR}"
./configure \
    --enable-vt-ferry-videotoolbox-linux \
    --enable-videotoolbox \
    --disable-doc \
    --disable-debug \
    --extra-cflags="-I${REPO_ROOT}/crates/vt-ferry-shim/include" \
    --extra-ldflags="-L${REPO_ROOT}/artifacts/ffmpeg-shim-libs/linux-debug" \
    --extra-libs="-lVideoToolbox -lCoreVideo -lCoreMedia -lCoreFoundation"

echo "==> Building FFmpeg"
make -j"$(nproc)"

echo "==> ffmpeg + ffprobe ready in ${REPO_ROOT}/${FFMPEG_BUILD_DIR}/"
