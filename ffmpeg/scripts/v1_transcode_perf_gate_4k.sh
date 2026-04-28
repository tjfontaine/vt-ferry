#!/bin/sh
#
# 4K variant of v1_transcode_perf_gate.sh.
#
# 4K transcode is dominated by encoder + decoder hardware throughput.
# Both host VT and guest VT keep up with realtime by a comfortable
# margin (~1.7x observed) but neither matches the libx264 software
# fps numbers — that's expected, since libx264 chews ~8 cores at
# 4K and VT uses ~1 core's worth of fixed-function silicon.
#
# Same three thresholds as the 1080p gate, but realtime floor
# loosened to 1.0x (the bar at 4K is "must keep up with realtime"
# rather than "must be 2x realtime"). CPU efficiency cap stays
# at 0.5 — the value-of-hardware story is even sharper at 4K
# because libx264 4K is more expensive than 1080p.
#
# Defaults:
#   * REFERENCE_VIDEO  synthetic 4K clip in artifacts/reference-videos/
#   * FRAME_LIMIT      60 (2 seconds of 4K source)
#   * MIN_GUEST_TRANSCODE_REALTIME      1.0
#   * MAX_GUEST_VS_HOST_TRANSCODE_RATIO  2.0 (same as 1080p; the
#                                            real-VT-decode floor
#                                            applies at 4K too,
#                                            observed ~1.6x)
#   * MAX_GUEST_TRANSCODE_CPU_RATIO      0.5 (same as 1080p)

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

REFERENCE_VIDEO="${REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/synthetic_4k_30fps_5s.mp4}"
FRAME_LIMIT="${FRAME_LIMIT:-60}"
MIN_GUEST_TRANSCODE_REALTIME="${MIN_GUEST_TRANSCODE_REALTIME:-1.0}"
MAX_GUEST_VS_HOST_TRANSCODE_RATIO="${MAX_GUEST_VS_HOST_TRANSCODE_RATIO:-2.0}"
MAX_GUEST_TRANSCODE_CPU_RATIO="${MAX_GUEST_TRANSCODE_CPU_RATIO:-0.5}"

if [ ! -f "${REFERENCE_VIDEO}" ]; then
    echo "ERROR: REFERENCE_VIDEO not found: ${REFERENCE_VIDEO}" >&2
    echo "" >&2
    echo "Generate the synthetic 4K clip with:" >&2
    echo "  ffmpeg -y -f lavfi -i testsrc2=size=3840x2160:rate=30:duration=5 \\" >&2
    echo "         -c:v libx264 -preset ultrafast -pix_fmt yuv420p \\" >&2
    echo "         artifacts/reference-videos/synthetic_4k_30fps_5s.mp4" >&2
    echo "or set REFERENCE_VIDEO=/path/to/4k_clip.mp4." >&2
    exit 1
fi

REFERENCE_VIDEO="${REFERENCE_VIDEO}" \
FRAME_LIMIT="${FRAME_LIMIT}" \
MIN_GUEST_TRANSCODE_REALTIME="${MIN_GUEST_TRANSCODE_REALTIME}" \
MAX_GUEST_VS_HOST_TRANSCODE_RATIO="${MAX_GUEST_VS_HOST_TRANSCODE_RATIO}" \
MAX_GUEST_TRANSCODE_CPU_RATIO="${MAX_GUEST_TRANSCODE_CPU_RATIO}" \
exec "${SCRIPT_DIR}/v1_transcode_perf_gate.sh" "$@"
