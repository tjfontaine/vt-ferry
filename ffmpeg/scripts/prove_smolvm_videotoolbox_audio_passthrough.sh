#!/bin/sh
#
# Audio-passthrough variant of the smolvm VideoToolbox proof.
#
# The default smoke uses lavfi sources without audio and forces
# `-an`, so audio handling has zero coverage today. Real-world video
# pipelines almost always carry an audio track alongside the video,
# and FFmpeg's demux/mux machinery has to thread that audio through
# unchanged while the encoder shim is busy on the video stream.
#
# This wrapper drives the smoke against a reference clip that DOES
# carry audio (bbb_sunflower 1080p) and asks ffmpeg to:
#
#   * encode video via h264_videotoolbox (the shim's hot path)
#   * `-c:a copy` the audio (bypasses any audio encoder; ffmpeg just
#     remuxes the existing track into the output container)
#   * assert via ffprobe that BOTH a video AND an audio stream
#     survive the round-trip
#
# Verifies that the shim is non-disruptive to upstream/downstream
# stream handling — important because the shim sits in the middle
# of FFmpeg's filter graph for video and a regression there could
# silently corrupt audio timing or strip the track entirely.
#
# Defaults assume the reference clip lives at the canonical path
# (artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4).
# Override REFERENCE_VIDEO to use a different audio-bearing clip.
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_audio_passthrough.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

REFERENCE_VIDEO="${REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4}"
FRAME_LIMIT="${FRAME_LIMIT:-60}"

if [ ! -f "${REFERENCE_VIDEO}" ]; then
    echo "ERROR: REFERENCE_VIDEO not found: ${REFERENCE_VIDEO}" >&2
    echo "" >&2
    echo "Download the bbb_sunflower clip per artifacts/reference-videos/README.md" >&2
    echo "or set REFERENCE_VIDEO=/path/to/clip_with_audio.mp4." >&2
    exit 1
fi

REFERENCE_VIDEO="${REFERENCE_VIDEO}" \
FRAME_COUNT="${FRAME_COUNT:-${FRAME_LIMIT}}" \
AUDIO_ARGS="${AUDIO_ARGS:--c:a copy}" \
EXPECT_AUDIO_STREAM=1 \
GUEST_CODEC="${GUEST_CODEC:-h264_videotoolbox}" \
EXPECTED_CODEC_NAME="${EXPECTED_CODEC_NAME:-h264}" \
exec "${SCRIPT_DIR}/prove_smolvm_videotoolbox_encode.sh" "$@"
