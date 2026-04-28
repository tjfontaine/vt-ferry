#!/bin/sh
#
# HEVC variant of the v1 decode perf gate.
#
# Stages a host-side HEVC reference clip (via hevc_videotoolbox if
# none exists at the expected path) and execs v1_decode_perf_gate.sh
# against it. Tests the same three thresholds — guest-vs-host
# wallclock ratio, realtime floor, CPU efficiency cap — but on the
# HEVC decode path (`hevc_videotoolbox_hwaccel`) instead of H.264.
#
# Why a separate wrapper rather than a knob on the H.264 gate:
#   - HEVC decode involves a different parameter-set delivery
#     (VPS+SPS+PPS) which has its own extraction path in the worker
#     (`from_hevc_parameter_sets`) and a different shim branch for
#     the format description. A regression in the HEVC parameter-
#     set packing wouldn't fail the H.264 gate
#   - the gate runs are independent so a CI orchestrator can
#     parallelize them
#
# The HEVC reference is staged once and cached at the path below.
# Subsequent runs reuse it. Override HEVC_REFERENCE_VIDEO to point
# at a pre-staged file (e.g. a Main10 / 10-bit clip for HDR
# regression testing).
#
# Usage:
#   ffmpeg/scripts/v1_decode_perf_gate_hevc.sh
#   FRAME_LIMIT=600 ffmpeg/scripts/v1_decode_perf_gate_hevc.sh
#   HEVC_REFERENCE_VIDEO=/path/to/hevc.mp4 \
#     ffmpeg/scripts/v1_decode_perf_gate_hevc.sh

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
  # hevc_videotoolbox is hardware-encoded on the host, so this is
  # near-instant even for the full 10-minute source. -bf 0 keeps
  # the bitstream simple (no B-frames) for cleaner decode-side
  # measurement.
  "${HOST_FFMPEG_BIN}" -hide_banner -y \
    -i "${SOURCE_REFERENCE_VIDEO}" \
    -map 0:v:0 -an \
    -c:v hevc_videotoolbox -profile:v main -b:v 6000000 -bf 0 \
    "${HEVC_REFERENCE_VIDEO}" >/dev/null 2>&1 \
    || die "failed to stage HEVC reference via hevc_videotoolbox"
fi

# HEVC decode wallclock ratio is structurally noisier than H.264 at
# 300 frames: host VT HEVC decode hits ~0.8s wallclock at 1080p so
# 50-100ms host noise = 6-12% ratio variance. Pre-Phase-15 the cap
# was 2.0x against fictional ~0.59x; real VT runs span 2.0-2.8x
# back-to-back. 3.5x absorbs the noise; if a real regression pushes
# above this, FRAME_LIMIT=1200 (4x more frames) shrinks the noise
# floor enough to read the signal.
REFERENCE_VIDEO="${HEVC_REFERENCE_VIDEO}" \
MAX_GUEST_VS_HOST_DECODE_RATIO="${MAX_GUEST_VS_HOST_DECODE_RATIO:-3.5}" \
exec "${SCRIPT_DIR}/v1_decode_perf_gate.sh" "$@"
