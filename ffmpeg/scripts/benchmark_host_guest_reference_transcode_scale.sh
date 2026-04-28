#!/bin/sh
#
# Scale-during-transcode benchmark — exercises swscale on the
# guest while the VT decoder + encoder are both active. This is
# the streaming-pipeline workflow: ingest 4K, scale to 1080p,
# re-encode for distribution.
#
# Why this matters that the no-scale transcode benchmark doesn't:
# `-f null -` transcode without a scale filter has FFmpeg's filter
# graph essentially passing pixel buffers through unchanged. The
# decoded CVPixelBuffer goes straight to the encoder without
# anyone reading its bytes on the guest CPU. If a guest-side
# bottleneck exists (shim's chunked-read memcpy, or a hot path
# in the CVPixelBuffer proxy), the no-scale benchmark hides it.
#
# Adding `scale=1920:1080` forces swscale to:
#   1. Lock the decoded CVPixelBuffer (triggers any lazy
#      pixel-fetch logic in the shim)
#   2. Read 12 MiB / frame at 4K from guest memory
#   3. Compute scaled output (~2.5 MiB / frame at 1080p NV12)
#   4. Write to the encode-input pool slot
#   5. Encode side reads the slot
#
# So scale-transcode pays the full pixel-fetch cost on every
# frame, just like real workloads do (display, transcode-with-
# format-conversion, etc.). If guest CPU is materially higher
# here than in the no-scale transcode benchmark, that's the
# real-world characteristic users see.
#
# Defaults to 4K input scaled to 1080p (the most common
# streaming workflow). Override TARGET_WIDTH / TARGET_HEIGHT for
# other scaling.
#
# Usage:
#   ffmpeg/scripts/benchmark_host_guest_reference_transcode_scale.sh
#   TARGET_WIDTH=1280 TARGET_HEIGHT=720 \
#     ffmpeg/scripts/benchmark_host_guest_reference_transcode_scale.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

# Default to the 4K source — the whole point of this benchmark
# is to exercise swscale on a substantial input.
REFERENCE_VIDEO="${REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/synthetic_4k_30fps_5s.mp4}"
TARGET_WIDTH="${TARGET_WIDTH:-1920}"
TARGET_HEIGHT="${TARGET_HEIGHT:-1080}"
# 60 frames at 4K is enough to amortize VM bringup without making
# the run take forever.
FRAME_LIMIT="${FRAME_LIMIT:-60}"

REFERENCE_VIDEO="${REFERENCE_VIDEO}" \
FRAME_LIMIT="${FRAME_LIMIT}" \
VF_FILTERS="${VF_FILTERS:-scale=${TARGET_WIDTH}:${TARGET_HEIGHT},format=nv12}" \
POOL_WIDTH="${POOL_WIDTH:-${TARGET_WIDTH}}" \
POOL_HEIGHT="${POOL_HEIGHT:-${TARGET_HEIGHT}}" \
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/ffmpeg-reference-transcode-scale-benchmark-$$}" \
VM_NAME="${VM_NAME:-vt-ferry-transcode-scale-bench-$$}" \
exec "${SCRIPT_DIR}/benchmark_host_guest_reference_transcode.sh" "$@"
