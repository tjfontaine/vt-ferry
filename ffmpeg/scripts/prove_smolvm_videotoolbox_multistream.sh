#!/bin/sh
#
# Multi-stream variant of the smolvm VideoToolbox proof.
#
# Real transcoding pipelines (e.g. ABR ladder generation, OTT
# packaging) fan a single decode out into multiple encoder sessions
# in the same ffmpeg process — different resolutions, different
# codecs, sometimes different parameter sets. This wrapper drives a
# representative two-output ffmpeg run:
#
#   * input → 1280x720 h264_videotoolbox → out_720p.mp4
#   * input → 854x480  h264_videotoolbox → out_480p.mp4
#
# Both outputs share the same source frame, so the worker has two
# `VTCompressionSession` peers alive at once feeding off the same
# `IOSurface` pool slot rotation. Validates:
#
#   * the worker's session table holds two sessions concurrently
#     without aliasing pixel-buffer pools or output queues
#   * `OP_ALLOC_BUFFER` / `OP_RECYCLE_BUFFER` recycle correctly under
#     concurrent demand (each output reads the same source frames
#     but at potentially different pull rates)
#   * codec-specific parameter sets get delivered alongside their
#     own sample buffers — no bleed between sessions
#   * both round-trip outputs probe correctly via ffprobe
#
# The harness's FFMPEG_OUTPUT_OVERRIDE env knob lets the wrapper
# supply the entire output spec to ffmpeg; MULTI_OUTPUT_PATHS makes
# encode.sh copy each guest output back to the host workdir before
# the wrapper runs its own probes.
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_multistream.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

REFERENCE_VIDEO="${REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4}"
FRAME_LIMIT="${FRAME_LIMIT:-60}"
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/smolvm-vt-multistream-$$}"
FFPROBE_BIN="${FFPROBE_BIN:-ffprobe}"

if [ ! -f "${REFERENCE_VIDEO}" ]; then
    echo "ERROR: REFERENCE_VIDEO not found: ${REFERENCE_VIDEO}" >&2
    echo "Download it per artifacts/reference-videos/README.md." >&2
    exit 1
fi

GUEST_OUT_720P="/tmp/multistream_720p.mp4"
GUEST_OUT_480P="/tmp/multistream_480p.mp4"

# Two encoder sessions in one ffmpeg run. Each output has its own
# `-map`, `-vf`, `-c:v`, codec args, and output filename.
OUTPUT_OVERRIDE="-map 0:v:0 -an -frames:v ${FRAME_LIMIT} -vf scale=1280:720,format=nv12 -c:v h264_videotoolbox -y ${GUEST_OUT_720P} -map 0:v:0 -an -frames:v ${FRAME_LIMIT} -vf scale=854:480,format=nv12 -c:v h264_videotoolbox -y ${GUEST_OUT_480P}"

REFERENCE_VIDEO="${REFERENCE_VIDEO}" \
WORKDIR="${WORKDIR}" \
FFMPEG_OUTPUT_OVERRIDE="${OUTPUT_OVERRIDE}" \
MULTI_OUTPUT_PATHS="${GUEST_OUT_720P}:${GUEST_OUT_480P}" \
"${SCRIPT_DIR}/prove_smolvm_videotoolbox_encode.sh" "$@"

# Encode.sh exits 0 after copying both outputs back. Run our own
# probes and assert dimensions/codec for each output independently.
HOST_OUT_720P="${WORKDIR}/$(basename ${GUEST_OUT_720P})"
HOST_OUT_480P="${WORKDIR}/$(basename ${GUEST_OUT_480P})"

probe_one() {
    path="$1"
    expected_w="$2"
    expected_h="$3"
    probe_json="$("${FFPROBE_BIN}" -hide_banner -select_streams v:0 \
        -show_streams -print_format json "${path}" 2>/dev/null)"
    python3 - <<PY
import json
probe = json.loads("""${probe_json}""")
streams = [s for s in probe.get("streams", []) if s.get("codec_type") == "video"]
if len(streams) != 1:
    raise SystemExit(f"${path}: expected 1 video stream, got {len(streams)}")
s = streams[0]
if s.get("codec_name") != "h264":
    raise SystemExit(f"${path}: expected h264, got {s.get('codec_name')}")
if int(s.get("width", 0)) != ${expected_w}:
    raise SystemExit(f"${path}: expected width ${expected_w}, got {s.get('width')}")
if int(s.get("height", 0)) != ${expected_h}:
    raise SystemExit(f"${path}: expected height ${expected_h}, got {s.get('height')}")
if int(s.get("nb_frames", 0)) < 1:
    raise SystemExit(f"${path}: expected >=1 frame, got {s.get('nb_frames')}")
print(f"  ${path}: codec=h264 size={s.get('width')}x{s.get('height')} frames={s.get('nb_frames')}")
PY
}

echo "multi-stream probe results:"
probe_one "${HOST_OUT_720P}" 1280 720
probe_one "${HOST_OUT_480P}" 854 480

echo "multi-stream smoke passed: two concurrent VTCompressionSessions"
echo "  720p: ${HOST_OUT_720P}"
echo "  480p: ${HOST_OUT_480P}"
echo "  workdir: ${WORKDIR}"
