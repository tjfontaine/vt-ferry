#!/bin/sh
#
# Transcode smoke: VT decode + VT encode in the same guest FFmpeg
# process. Exercises the most common real-world workflow —
#
#   ffmpeg -hwaccel videotoolbox -i in.mp4 -c:v h264_videotoolbox out.mp4
#
# — with both `VTDecompressionSession` and `VTCompressionSession`
# alive on the same broker simultaneously.
#
# What this proves that the encode-only and decode-only smokes don't:
#
#   - both VT session kinds (compression + decompression) coexist
#     on the same broker without state-machine collision
#   - OP_DRAIN routes correctly when both encode and decode sessions
#     are pending (encode drain ≠ decode drain handler-wise)
#   - the worker's session-id keying stays clean across kinds —
#     a pool bound to a decode session must not get claimed by an
#     encode session and vice versa
#   - per-frame round-trip semantics: decode emits a CVPixelBuffer,
#     FFmpeg's transcode pipeline hands it back to the encode side,
#     which round-trips through OP_ENCODE_FRAME / OP_WRITE_BUFFER.
#     Any subtle ref-count or generation-bump bug shows up as
#     truncated output or a midstream NUL frame
#
# Capped at 480p so the decode side stays in the inline
# `OP_READ_DECODED_FRAME` path. 1080p+ would require the launcher
# to pre-register two pools (one for encode-input, one for
# decode-output binding), which the broker supports but adds setup
# complexity not needed for the basic transcode contract. A
# 1080p-transcode wrapper can land later as a thin extension.
#
# Pipeline:
#   1. Host libx264 produces a known-good 480p H.264 bitstream
#      (purely host-side, no shim involvement). Mirrors the decode
#      smoke's stage-source step.
#   2. The encoded mp4 gets staged under the repo mount.
#   3. Delegates to prove_smolvm_videotoolbox_encode.sh with
#      INPUT_DECODER_ARGS=-hwaccel videotoolbox so the canonical
#      driver's guest ffmpeg invocation does VT decode → VT encode.
#   4. The encode driver's existing ffprobe pass validates the
#      output codec_name matches `h264`, frame count matches the
#      source's nb_frames, and the file isn't truncated.
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_transcode.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

HOST_FFMPEG_BIN="${HOST_FFMPEG_BIN:-ffmpeg}"
FRAME_SIZE="${FRAME_SIZE:-640x480}"
FRAME_RATE="${FRAME_RATE:-24}"
DURATION="${DURATION:-1}"
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/transcode-proof-$$}"

WIDTH="${FRAME_SIZE%x*}"
HEIGHT="${FRAME_SIZE#*x}"

die() {
  echo "ERROR: $*" >&2
  exit 1
}

command -v "${HOST_FFMPEG_BIN}" >/dev/null 2>&1 || die "missing host ffmpeg"

mkdir -p "${WORKDIR}"
INTERMEDIATE_MP4="${WORKDIR}/source.mp4"

# Step 1: stage a host-side libx264 source. Same shape as the decode
# driver's source step, so guest VT decode lands on a known-good
# bitstream rather than a synthetic fixture that might trip an edge
# case in VTDecompressionSession initialization.
echo "host: encoding ${WIDTH}x${HEIGHT}@${FRAME_RATE}fps for ${DURATION}s with libx264"
"${HOST_FFMPEG_BIN}" -hide_banner -y \
  -f lavfi -i "testsrc2=size=${WIDTH}x${HEIGHT}:rate=${FRAME_RATE}:duration=${DURATION}" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
  "${INTERMEDIATE_MP4}" >/dev/null 2>&1

[ -s "${INTERMEDIATE_MP4}" ] || die "host encode produced empty file"

# Step 2: delegate to the encode driver. REFERENCE_VIDEO points at
# the staged source; INPUT_DECODER_ARGS adds -hwaccel videotoolbox
# so the guest decode side goes through the shim.
#
# The encode driver picks up FRAME_SIZE / FRAME_RATE for pool
# sizing. SLOT_COUNT bumps to 32 automatically because
# REFERENCE_VIDEO is set (real-video keeps several frames in
# flight; lavfi defaults of 4 would exhaust the input pool).
echo "guest: transcoding via VT decode → VT encode"
REFERENCE_VIDEO="${INTERMEDIATE_MP4}" \
INPUT_DECODER_ARGS="-hwaccel videotoolbox" \
FRAME_SIZE="${FRAME_SIZE}" \
FRAME_RATE="${FRAME_RATE}" \
WORKDIR="${WORKDIR}/encode" \
VM_NAME="${VM_NAME:-vt-ferry-transcode-$$}" \
exec "${SCRIPT_DIR}/prove_smolvm_videotoolbox_encode.sh" "$@"
