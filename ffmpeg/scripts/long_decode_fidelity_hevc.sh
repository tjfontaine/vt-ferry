#!/bin/sh
#
# HEVC variant of the long decode fidelity gate.
#
# Stages a host-side HEVC reference clip (via hevc_videotoolbox if
# none exists) and execs long_decode_fidelity.sh against it. Same
# tight PSNR / SSIM thresholds as the H.264 gate (PSNR>=80,
# SSIM>=0.999) — both decode paths share the same Apple
# implementation, so the bit-identical baseline holds across
# codecs. A regression here points specifically at HEVC
# parameter-set packing or the `from_hevc_parameter_sets` worker
# branch.
#
# The reference is staged once and cached. See
# v1_decode_perf_gate_hevc.sh for the equivalent staging logic on
# the perf side; this wrapper deliberately reuses the same cached
# file (HEVC_REFERENCE_VIDEO default path) so the perf and fidelity
# gates exercise the same source clip.
#
# Usage:
#   ffmpeg/scripts/long_decode_fidelity_hevc.sh
#   FRAME_COUNT=900 ffmpeg/scripts/long_decode_fidelity_hevc.sh

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

if [ ! -f "${HEVC_REFERENCE_VIDEO}" ]; then
  if [ ! -f "${SOURCE_REFERENCE_VIDEO}" ]; then
    die "neither HEVC_REFERENCE_VIDEO (${HEVC_REFERENCE_VIDEO}) nor \
SOURCE_REFERENCE_VIDEO (${SOURCE_REFERENCE_VIDEO}) exists; \
provide one or drop bbb_sunflower_1080p_30fps_normal.mp4 into \
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
exec "${SCRIPT_DIR}/long_decode_fidelity.sh" "$@"
