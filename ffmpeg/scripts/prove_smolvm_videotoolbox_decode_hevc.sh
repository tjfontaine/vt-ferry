#!/bin/sh
#
# HEVC variant of the smolvm VideoToolbox decode proof.
#
# Drives prove_smolvm_videotoolbox_decode.sh with libx265 as the
# host-side encoder. Guest-side decode is codec-agnostic — we feed
# `-hwaccel videotoolbox`, and FFmpeg's hwaccel selection picks
# `hevc_videotoolbox_hwaccel` (enabled in our patch) once it sees
# an HEVC bitstream.
#
# This proves:
#   1. CMVideoFormatDescriptionCreateFromHEVCParameterSets exports
#      from the shim and packs VPS+SPS+PPS into OP_SET_DECODE_FORMAT.
#   2. Worker (vt_real.rs) takes the codec=hvc1 branch in
#      OP_SET_DECODE_FORMAT and calls
#      `CMVideoFormatDescription::from_hevc_parameter_sets` on the
#      host VT side.
#   3. The decompression callback delivers NV12 frames just like the
#      H.264 path — codec discrimination is invisible to the output
#      surface.
#
# Frame size capped at 480p so the decoded NV12 fits the 1.5 MiB
# inline pixel-data response cap.
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_decode_hevc.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"

HOST_CODEC="${HOST_CODEC:-libx265}" \
HOST_PRESET="${HOST_PRESET:-ultrafast}" \
HOST_PIX_FMT="${HOST_PIX_FMT:-yuv420p}" \
CODEC_LABEL="${CODEC_LABEL:-hevc}" \
VM_NAME="${VM_NAME:-vt-ferry-decode-hevc-$$}" \
WORKDIR="${WORKDIR:-${SCRIPT_DIR}/../../artifacts/decode-hevc-proof-$$}" \
exec "${SCRIPT_DIR}/prove_smolvm_videotoolbox_decode.sh" "$@"
