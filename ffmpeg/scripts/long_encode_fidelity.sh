#!/bin/sh
#
# Phase 8 long-encode fidelity gate.
#
# Drives compare_host_guest_videotoolbox_smoke.sh against a real
# reference clip for a long-enough run that any drift between the
# host and guest VideoToolbox encode paths would show up (encoder
# state warmup, GOP boundaries, rate-control feedback, etc.). The
# 30-frame smoke proved bit-identical decode (PSNR=inf, SSIM=1.0);
# this script extends to the full configured frame budget and
# enforces PSNR/SSIM thresholds so a regression that *almost*
# matches the reference still fails CI.
#
# Defaults aim for "fast enough to run on every PR": 300 frames of
# the 1080p Big Buck Bunny clip (≈ 10 s of encode @ 30 fps). Override
# FRAME_COUNT for a longer soak. Override the MIN_* thresholds to
# raise or lower the gate.
#
# Usage:
#   ffmpeg/scripts/long_encode_fidelity.sh
#   FRAME_COUNT=900 ffmpeg/scripts/long_encode_fidelity.sh
#   MIN_PSNR_AVERAGE=45 ffmpeg/scripts/long_encode_fidelity.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

REFERENCE_VIDEO="${REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4}"
FRAME_COUNT="${FRAME_COUNT:-300}"

# Threshold defaults. The 30-frame smoke produces PSNR=inf / SSIM=1.0
# (bit-identical encode) — these gates are deliberately *looser* so
# they only fail on a real regression, not on a measurement-floor
# fluctuation in a longer clip. Override on the command line if a
# stricter floor is appropriate for your run.
MIN_PSNR_AVERAGE="${MIN_PSNR_AVERAGE:-40}"
MIN_PSNR_MIN="${MIN_PSNR_MIN:-30}"
MIN_SSIM_ALL="${MIN_SSIM_ALL:-0.99}"

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
exec "${REPO_ROOT}/ffmpeg/scripts/compare_host_guest_videotoolbox_smoke.sh" "$@"
