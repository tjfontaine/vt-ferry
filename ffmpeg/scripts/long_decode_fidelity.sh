#!/bin/sh
#
# Phase 11 long-decode fidelity gate.
#
# Decode-side analogue to long_encode_fidelity.sh. Drives
# compare_host_guest_videotoolbox_decode.sh against the bundled
# 1080p reference clip and asserts the host VideoToolbox decode
# and guest VideoToolbox decode (over vsock + chunked READ_BUFFER)
# produce frame-by-frame matching NV12 output.
#
# Catches:
#   - per-plane stride math drift in vtf_copy_pixel_buffer_planes
#   - chunk-boundary off-by-ones in OP_READ_BUFFER (pool path for
#     >720p frames)
#   - parameter-set extraction or formatting drift (a missed byte
#     would corrupt every frame's first slice header → dramatic
#     PSNR drop)
#
# A perfect run produces PSNR=inf / SSIM=1.0 — the two decode
# paths should be bit-identical because both use the same Apple
# VideoToolbox implementation; the shim is a transparent wire
# bridge. The defaults (PSNR>=80, SSIM>=0.999) are deliberately
# tighter than the encode fidelity gate (PSNR>=40, SSIM>=0.99)
# because there's no encoder rate-control noise to absorb — any
# real deviation is a transport-layer bug.
#
# Defaults aim for "fast enough to run on every PR": 300 frames
# of the 1080p Big Buck Bunny clip (≈ 10 s @ 30 fps).
#
# Usage:
#   ffmpeg/scripts/long_decode_fidelity.sh
#   FRAME_COUNT=900 ffmpeg/scripts/long_decode_fidelity.sh
#   MIN_SSIM_ALL=1.0 ffmpeg/scripts/long_decode_fidelity.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

REFERENCE_VIDEO="${REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4}"
FRAME_COUNT="${FRAME_COUNT:-300}"

# Tighter than encode fidelity because decode is supposed to be
# bit-identical between host and guest paths (same Apple
# implementation, just routed through vsock). Override on the
# command line if a relaxed floor is appropriate (e.g. for a
# soak that runs across many GOP boundaries on a fragile clip).
MIN_PSNR_AVERAGE="${MIN_PSNR_AVERAGE:-80}"
MIN_PSNR_MIN="${MIN_PSNR_MIN:-50}"
MIN_SSIM_ALL="${MIN_SSIM_ALL:-0.999}"

if [ ! -f "${REFERENCE_VIDEO}" ]; then
    echo "ERROR: REFERENCE_VIDEO not found: ${REFERENCE_VIDEO}" >&2
    echo "" >&2
    echo "Drop the Big Buck Bunny 1080p sample into" >&2
    echo "${REPO_ROOT}/artifacts/reference-videos/" >&2
    echo "or set REFERENCE_VIDEO=/path/to/clip.mp4." >&2
    exit 1
fi

REFERENCE_VIDEO="${REFERENCE_VIDEO}" \
FRAME_COUNT="${FRAME_COUNT}" \
MIN_PSNR_AVERAGE="${MIN_PSNR_AVERAGE}" \
MIN_PSNR_MIN="${MIN_PSNR_MIN}" \
MIN_SSIM_ALL="${MIN_SSIM_ALL}" \
exec "${REPO_ROOT}/ffmpeg/scripts/compare_host_guest_videotoolbox_decode.sh" "$@"
