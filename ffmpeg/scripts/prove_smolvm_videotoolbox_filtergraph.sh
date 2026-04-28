#!/bin/sh
#
# Filter-graph variant of the smolvm VideoToolbox proof.
#
# The plain encode smoke runs a minimal `format=<pix>` filter so the
# pipeline reduces to "decode source -> swscale to NV12 -> VT encode".
# Real FFmpeg jobs typically chain more — scale, fps conversion, crop,
# overlay. This wrapper sets PRE_FORMAT_FILTERS to a representative
# scale+fps chain so the smoke validates that the shim tolerates:
#
#   * source dimensions != encoder dimensions (swscale picks up after
#     decode and hands a different size to the VT encoder)
#   * source frame rate != encoder frame rate (fps filter drops/dups
#     frames; the shim must still recycle pool buffers in lock-step)
#
# Defaults pick a 720p / 24fps target from a synthetic 480p / 30fps
# source, both moving in different directions (scale up + fps down)
# to exercise both swscale paths.
#
# Override knobs (env or CLI):
#   FRAME_SIZE        source size (default 480x270)
#   FRAME_RATE        source rate (default 30)
#   DURATION          source duration in seconds (default 1)
#   TARGET_SIZE       encoder size (default 1280x720)
#   TARGET_FPS        encoder fps (default 24)
#   GUEST_CODEC       encoder (default h264_videotoolbox)
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_filtergraph.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"

FRAME_SIZE="${FRAME_SIZE:-480x270}"
FRAME_RATE="${FRAME_RATE:-30}"
DURATION="${DURATION:-1}"
TARGET_SIZE="${TARGET_SIZE:-1280x720}"
TARGET_FPS="${TARGET_FPS:-24}"
TARGET_W="${TARGET_SIZE%x*}"
TARGET_H="${TARGET_SIZE#*x}"

FRAME_SIZE="${FRAME_SIZE}" \
FRAME_RATE="${FRAME_RATE}" \
DURATION="${DURATION}" \
PRE_FORMAT_FILTERS="${PRE_FORMAT_FILTERS:-scale=${TARGET_W}:${TARGET_H},fps=${TARGET_FPS}}" \
EXPECTED_OUTPUT_WIDTH="${EXPECTED_OUTPUT_WIDTH:-${TARGET_W}}" \
EXPECTED_OUTPUT_HEIGHT="${EXPECTED_OUTPUT_HEIGHT:-${TARGET_H}}" \
GUEST_CODEC="${GUEST_CODEC:-h264_videotoolbox}" \
EXPECTED_CODEC_NAME="${EXPECTED_CODEC_NAME:-h264}" \
exec "${SCRIPT_DIR}/prove_smolvm_videotoolbox_encode.sh" "$@"
