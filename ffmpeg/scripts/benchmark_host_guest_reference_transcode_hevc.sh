#!/bin/sh
#
# HEVC variant of benchmark_host_guest_reference_transcode.sh.
#
# Stages a hevc_videotoolbox-encoded reference clip (cached at
# the path the v1_decode_perf_gate_hevc.sh stages, so the two
# benchmarks share the same source clip) and runs the transcode
# benchmark with libx265 + hevc_videotoolbox as the encoders.
# Decode side is codec-agnostic — `-hwaccel videotoolbox` picks
# `hevc_videotoolbox_hwaccel` from the HEVC bitstream's codec_id.
#
# Why a separate HEVC wrapper rather than a knob on the H.264
# benchmark: the encode-side parameter space differs slightly
# (e.g. main10 / 10-bit profiles for HEVC). Keeping HEVC in its
# own wrapper makes the codec-specific defaults explicit while
# the underlying benchmark stays one source of truth.
#
# Usage:
#   ffmpeg/scripts/benchmark_host_guest_reference_transcode_hevc.sh
#   FRAME_LIMIT=120 \
#     ffmpeg/scripts/benchmark_host_guest_reference_transcode_hevc.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

HOST_FFMPEG_BIN="${HOST_FFMPEG_BIN:-ffmpeg}"
SOURCE_REFERENCE_VIDEO="${SOURCE_REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4}"
HEVC_REFERENCE_VIDEO="${HEVC_REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_hevc.mp4}"

die() {
  echo "ERROR: $*" >&2
  exit 1
}

command -v "${HOST_FFMPEG_BIN}" >/dev/null 2>&1 || die "missing host ffmpeg"

# Stage HEVC reference if absent. Mirrors v1_decode_perf_gate_hevc.sh
# so that gate's cache path is reused — staging happens once across
# both benchmarks.
if [ ! -f "${HEVC_REFERENCE_VIDEO}" ]; then
  if [ ! -f "${SOURCE_REFERENCE_VIDEO}" ]; then
    die "neither HEVC_REFERENCE_VIDEO (${HEVC_REFERENCE_VIDEO}) nor \
SOURCE_REFERENCE_VIDEO (${SOURCE_REFERENCE_VIDEO}) exists; provide \
one or drop bbb_sunflower_1080p_30fps_normal.mp4 into \
${REPO_ROOT}/artifacts/reference-videos/"
  fi
  echo "host: staging HEVC reference at ${HEVC_REFERENCE_VIDEO} (one-time)"
  "${HOST_FFMPEG_BIN}" -hide_banner -y \
    -i "${SOURCE_REFERENCE_VIDEO}" \
    -map 0:v:0 -an \
    -c:v hevc_videotoolbox -profile:v main -b:v 6000000 -bf 0 \
    "${HEVC_REFERENCE_VIDEO}" >/dev/null 2>&1 \
    || die "failed to stage HEVC reference via hevc_videotoolbox"
fi

REFERENCE_VIDEO="${HEVC_REFERENCE_VIDEO}" \
SOFTWARE_CODEC="${SOFTWARE_CODEC:-libx265}" \
VIDEOTOOLBOX_CODEC="${VIDEOTOOLBOX_CODEC:-hevc_videotoolbox}" \
exec "${SCRIPT_DIR}/benchmark_host_guest_reference_transcode.sh" "$@"
