#!/bin/sh
#
# HEVC variant of the smolvm VideoToolbox proof.
#
# Drives prove_smolvm_videotoolbox_encode.sh with
# `-c:v hevc_videotoolbox` and asserts the decoded codec_name is
# `hevc`. Same broker / vsock path as the H.264 smoke; only the
# encoder selection changes.
#
# The HEVC encode goes through the same VTCompressionSession surface
# in the guest shim — codec discrimination happens in two places:
#
#   1. Worker (vt_real.rs) extracts VPS+SPS+PPS via
#      `CMVideoFormatDescriptionGetHEVCParameterSetAtIndex` instead of
#      the H.264 fn when the session codec FourCC is `hvc1`.
#   2. Guest shim (videotoolbox.rs deliver_outputs) passes the codec
#      FourCC (sourced from the worker reply, falling back to the
#      session's codec_type) into vtf_create_video_format_description,
#      so the resulting CMSampleBuffer reports the right codec_type.
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_hevc.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"

GUEST_CODEC="${GUEST_CODEC:-hevc_videotoolbox}" \
EXPECTED_CODEC_NAME="${EXPECTED_CODEC_NAME:-hevc}" \
exec "${SCRIPT_DIR}/prove_smolvm_videotoolbox_encode.sh" "$@"
