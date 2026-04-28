#!/bin/sh
#
# 10-bit (P010 / HEVC Main10) variant of the smolvm VideoToolbox proof.
#
# Drives prove_smolvm_videotoolbox_encode.sh with hevc_videotoolbox,
# `-profile:v main10`, and PIXEL_FORMAT=0x78343230 ('x420', the
# video-range 10-bit 4:2:0 FourCC). FFmpeg's swscale will stage frames
# as p010le, the guest shim wraps them through CVPixelBuffer planar
# bytes (Y + interleaved CbCr at 2 bytes/sample), the host worker
# hands the layout to VTCompressionSession, and ffprobe must
# round-trip back to `codec=hevc, profile=Main 10`.
#
# Same broker / vsock plumbing as the 8-bit HEVC smoke; the only
# differences are the pixel format and the encoder profile.
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_hevc_p010.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"

GUEST_CODEC="${GUEST_CODEC:-hevc_videotoolbox}" \
EXPECTED_CODEC_NAME="${EXPECTED_CODEC_NAME:-hevc}" \
PIXEL_FORMAT="${PIXEL_FORMAT:-0x78343230}" \
GUEST_CODEC_ARGS="${GUEST_CODEC_ARGS:--profile:v main10}" \
exec "${SCRIPT_DIR}/prove_smolvm_videotoolbox_encode.sh" "$@"
