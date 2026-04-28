#!/bin/sh
#
# Container coverage variant of the smolvm VideoToolbox proof.
#
# Runs the canonical smoke once per container extension to verify
# that the encoder shim path is mux-agnostic. ffmpeg picks the muxer
# from the output filename extension, so this just iterates a small
# set of muxers known to differ in how they consume the encoded
# bitstream:
#
#   * mp4 — Annex-B → AVCC, sample tables, fragmented metadata. The
#     default container in the standard smoke
#   * mov — same as mp4 but slightly different brand/track defaults;
#     should be functionally identical from the shim's view
#   * mkv — Matroska, very different muxer code path; consumes
#     parameter sets and packets independently
#   * ts  — MPEG-TS, no global header, parameter sets get prefixed to
#     each IDR. Stresses the shim's per-frame parameter set delivery
#
# Each pass spins up a fresh broker / VM so the runtime is the sum
# of the individual smokes. Override CONTAINERS to a subset for
# faster iteration.
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_containers.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"

CONTAINERS="${CONTAINERS:-mp4 mov mkv ts}"
GUEST_CODEC="${GUEST_CODEC:-h264_videotoolbox}"
EXPECTED_CODEC_NAME="${EXPECTED_CODEC_NAME:-h264}"

failures=0
for ext in ${CONTAINERS}; do
  echo ""
  echo "=== container=${ext} ==="
  if OUTPUT_EXT="${ext}" \
     GUEST_CODEC="${GUEST_CODEC}" \
     EXPECTED_CODEC_NAME="${EXPECTED_CODEC_NAME}" \
     "${SCRIPT_DIR}/prove_smolvm_videotoolbox_encode.sh" "$@"; then
    echo "PASS container=${ext}"
  else
    failures=$((failures + 1))
    echo "FAIL container=${ext}" >&2
  fi
done

if [ "${failures}" -ne 0 ]; then
  echo "container smoke: ${failures} failure(s)" >&2
  exit 1
fi

echo ""
echo "container smoke: all ${CONTAINERS} passed"
