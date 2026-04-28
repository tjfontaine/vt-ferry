#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

REFERENCE_VIDEO="${REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4}"
FFPROBE_BIN="${FFPROBE_BIN:-ffprobe}"
HOST_WORKER_BIN="${HOST_WORKER_BIN:-${REPO_ROOT}/target/release/vt-ferry-worker}"
BROKER_BIN="${BROKER_BIN:-${REPO_ROOT}/target/release/vt-ferry-broker}"
GUEST_FFMPEG_BIN="${GUEST_FFMPEG_BIN:-${REPO_ROOT}/artifacts/ffmpeg-build/n8.1-linux-debug/ffmpeg}"
SHIM_LIBDIR="${SHIM_LIBDIR:-${REPO_ROOT}/artifacts/ffmpeg-shim-libs/linux-debug}"
SHIM_TARGET_TRIPLE="${SHIM_TARGET_TRIPLE:-aarch64-unknown-linux-gnu}"
STAGE_SHIM_LIBS="${STAGE_SHIM_LIBS:-1}"
DOCKER_IMAGE="${DOCKER_IMAGE:-${VM_IMAGE:-localhost:5005/vt-ferry-vt-bench:ubuntu-24.04-arm64}}"
DOCKER_PLATFORM="${DOCKER_PLATFORM:-linux/arm64}"
DOCKER_HOST_NAME="${DOCKER_HOST_NAME:-host.docker.internal}"
DOCKER_EXTRA_ARGS="${DOCKER_EXTRA_ARGS:-}"
TCP_BIND="${TCP_BIND:-127.0.0.1}"
TCP_PORT="${TCP_PORT:-0}"
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/docker-vt-tcp-$$}"
FRAME_LIMIT="${FRAME_LIMIT:-120}"
VIDEOTOOLBOX_ARGS="${VIDEOTOOLBOX_ARGS:--profile:v main -b:v 6000000 -bf 0}"
PIXEL_FORMAT="${PIXEL_FORMAT:-0x34323076}"
# When set to "1", prepend `-hwaccel videotoolbox` to the input,
# turning the smoke from "decode in libavcodec, encode in VT" into
# a true end-to-end VT transcode (decoder + encoder both go
# through the shimmed worker). Lets the same harness produce a
# transcode timing point comparable to the smolvm + vsock
# transcode perf gate without forking the script.
DOCKER_USE_HWACCEL="${DOCKER_USE_HWACCEL:-0}"
# FFmpeg encoder name (parameterised so HEVC and future codecs can
# share this Docker/TCP smoke). Default keeps the historical
# h264_videotoolbox baseline. The matching `EXPECTED_CODEC_NAME` is
# what ffprobe should report on the round-trip — set both when
# overriding (or use a wrapper script).
DOCKER_GUEST_CODEC="${DOCKER_GUEST_CODEC:-h264_videotoolbox}"
DOCKER_EXPECTED_CODEC_NAME="${DOCKER_EXPECTED_CODEC_NAME:-h264}"
# Map PIXEL_FORMAT FourCC to the ffmpeg `format=` filter argument.
# Mirrors the smolvm vsock smoke; new pixel formats need an entry
# here AND in the worker's layout fns.
case "${PIXEL_FORMAT}" in
  0x42475241|1111970369) DOCKER_FFMPEG_GUEST_FORMAT=bgra ;;
  0x34323076|875704438)  DOCKER_FFMPEG_GUEST_FORMAT=nv12 ;;
  0x78343230|2016686640) DOCKER_FFMPEG_GUEST_FORMAT=p010le ;;
  0x78663230|2019963440) DOCKER_FFMPEG_GUEST_FORMAT=p010le ;;
  *) DOCKER_FFMPEG_GUEST_FORMAT=nv12 ;;
esac
VT_FERRY_OUTPUT_POLL_INTERVAL="${VT_FERRY_OUTPUT_POLL_INTERVAL:-32}"
VT_FERRY_OUTPUT_BATCH_SIZE="${VT_FERRY_OUTPUT_BATCH_SIZE:-24}"
VT_FERRY_GUEST_POOL_BUFFER_COUNT="${VT_FERRY_GUEST_POOL_BUFFER_COUNT:-32}"

if [ -z "${SLOT_COUNT+x}" ]; then
  if [ "${FRAME_LIMIT}" -lt 32 ]; then
    if [ "${FRAME_LIMIT}" -lt 4 ]; then
      SLOT_COUNT=4
    else
      SLOT_COUNT="${FRAME_LIMIT}"
    fi
  else
    SLOT_COUNT=32
  fi
fi

case "${WORKDIR}" in
  /*) ;;
  *) WORKDIR="${REPO_ROOT}/${WORKDIR}" ;;
esac

die() {
  echo "ERROR: $*" >&2
  exit 1
}

require_file() {
  [ -e "$1" ] || die "missing required path: $1"
}

repo_path() {
  case "$1" in
    "${REPO_ROOT}") printf '/repo\n' ;;
    "${REPO_ROOT}"/*) printf '/repo/%s\n' "${1#${REPO_ROOT}/}" ;;
    *) return 1 ;;
  esac
}

extract_bench_rtime() {
  python3 - "$1" <<'PY'
import pathlib
import re
import sys

text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8", errors="replace")
matches = re.findall(r"bench:\s+.*?rtime=([0-9.]+)s", text)
if not matches:
    raise SystemExit(1)
print(matches[-1])
PY
}

command -v docker >/dev/null 2>&1 || die "docker is required"
docker info >/dev/null 2>&1 || die "docker is installed, but the Docker daemon is not reachable"
command -v python3 >/dev/null 2>&1 || die "missing required command: python3"
command -v "${FFPROBE_BIN}" >/dev/null 2>&1 || die "missing required command: ${FFPROBE_BIN}"

require_file "${REFERENCE_VIDEO}"
require_file "${HOST_WORKER_BIN}"
require_file "${BROKER_BIN}"

# Image mode (USE_VT_FERRY_IMAGE=1): the patched ffmpeg + shim
# live inside DOCKER_IMAGE under /opt/vt-ferry/. Skip host-side
# build/staging checks; the Docker invocation below uses
# /opt/vt-ferry/bin/ffmpeg directly and relies on ldconfig in
# the image to resolve the shim libs (no LD_LIBRARY_PATH).
USE_VT_FERRY_IMAGE="${USE_VT_FERRY_IMAGE:-0}"
if [ "${USE_VT_FERRY_IMAGE}" != "1" ]; then
  require_file "${GUEST_FFMPEG_BIN}"
  if [ "${STAGE_SHIM_LIBS}" = "1" ]; then
    "${REPO_ROOT}/ffmpeg/scripts/stage_guest_shim_libs.sh" \
      debug \
      "${SHIM_LIBDIR#${REPO_ROOT}/}" \
      "${SHIM_TARGET_TRIPLE}"
  fi
  require_file "${SHIM_LIBDIR}/libguest_shim.so"
fi

mkdir -p "${WORKDIR}/runtime"

probe_json="$("${FFPROBE_BIN}" -hide_banner -select_streams v:0 -show_streams -show_format -print_format json "${REFERENCE_VIDEO}")"
read width height duration_seconds <<EOF
$(python3 - <<PY
import json
probe = json.loads("""${probe_json}""")
stream = probe["streams"][0]
fmt = probe["format"]
print(stream["width"], stream["height"], fmt["duration"])
PY
)
EOF

POOL_JSON="$(python3 -c "
import json
print(json.dumps({
    'guest_phys_addr': 0x800000000,
    'slot_count': ${SLOT_COUNT},
    'width': ${width},
    'height': ${height},
    'pixel_format': ${PIXEL_FORMAT},
    'writable': True,
}))
")"

CONTAINER_REFERENCE_VIDEO="$(repo_path "${REFERENCE_VIDEO}")" || die "REFERENCE_VIDEO must be under ${REPO_ROOT}"
CONTAINER_WORKDIR="$(repo_path "${WORKDIR}")" || die "WORKDIR must be under ${REPO_ROOT}"
if [ "${USE_VT_FERRY_IMAGE}" = "1" ]; then
  # Image mode: ffmpeg + shim live inside DOCKER_IMAGE under
  # /opt/vt-ferry/. Skip the host-to-container path translation;
  # the binary is at a fixed absolute path inside the image.
  CONTAINER_FFMPEG_BIN="${CONTAINER_FFMPEG_BIN:-/opt/vt-ferry/bin/ffmpeg}"
  CONTAINER_SHIM_LIBDIR="${CONTAINER_SHIM_LIBDIR:-}"
else
  CONTAINER_FFMPEG_BIN="$(repo_path "${GUEST_FFMPEG_BIN}")" || die "GUEST_FFMPEG_BIN must be under ${REPO_ROOT}"
  CONTAINER_SHIM_LIBDIR="$(repo_path "${SHIM_LIBDIR}")" || die "SHIM_LIBDIR must be under ${REPO_ROOT}"
fi

LOG_PATH="${WORKDIR}/docker-videotoolbox-tcp.log"
MP4_PATH="${WORKDIR}/docker-videotoolbox-tcp.mp4"
HOST_WORKER_LOG="${WORKDIR}/host-worker.log"
LAUNCHER_LOG="${WORKDIR}/launcher.log"
SUMMARY_JSON="${WORKDIR}/summary.json"

rm -f "${LOG_PATH}" "${MP4_PATH}" "${HOST_WORKER_LOG}" "${LAUNCHER_LOG}" "${SUMMARY_JSON}"

if [ "${DOCKER_USE_HWACCEL}" = "1" ]; then
  hwaccel_args="-hwaccel videotoolbox"
else
  hwaccel_args=""
fi
# Image mode: ldconfig in the image already wired the shim libs;
# skip explicit LD_LIBRARY_PATH (an empty value would shadow the
# default search order). Otherwise point at the bind-mounted
# CONTAINER_SHIM_LIBDIR.
if [ -n "${CONTAINER_SHIM_LIBDIR}" ]; then
  CONTAINER_LD_PREFIX="LD_LIBRARY_PATH=${CONTAINER_SHIM_LIBDIR}"
else
  CONTAINER_LD_PREFIX=""
fi
docker_ffmpeg_cmd="env ${CONTAINER_LD_PREFIX} ${CONTAINER_FFMPEG_BIN} -hide_banner -benchmark -y ${hwaccel_args} -i ${CONTAINER_REFERENCE_VIDEO} -map 0:v:0 -an -frames:v ${FRAME_LIMIT} -vf format=${DOCKER_FFMPEG_GUEST_FORMAT} -c:v ${DOCKER_GUEST_CODEC} ${VIDEOTOOLBOX_ARGS} ${CONTAINER_WORKDIR}/$(basename "${MP4_PATH}") > ${CONTAINER_WORKDIR}/$(basename "${LOG_PATH}") 2>&1"

echo "docker: ${DOCKER_GUEST_CODEC} over TCP bridge"
echo "workdir: ${WORKDIR}"
echo "image: ${DOCKER_IMAGE}"
echo "frames: ${FRAME_LIMIT}"

set +e
VT_FERRY_HOST_WORKER_BACKEND=vt-real \
VT_FERRY_HOST_WORKER_STDERR_LOG="${HOST_WORKER_LOG}" \
OBJC_DISABLE_INITIALIZE_FORK_SAFETY="${OBJC_DISABLE_INITIALIZE_FORK_SAFETY:-YES}" \
"${BROKER_BIN}" \
  --transport tcp \
  --tcp-bind "${TCP_BIND}" \
  --tcp-port "${TCP_PORT}" \
  --tcp-guest-host "${DOCKER_HOST_NAME}" \
  --runtime-dir "${WORKDIR}/runtime" \
  --pool "${POOL_JSON}" \
  --host-worker "${HOST_WORKER_BIN}" \
  -- docker run --rm \
    --platform "${DOCKER_PLATFORM}" \
    ${DOCKER_EXTRA_ARGS} \
    -v "${REPO_ROOT}:/repo" \
    -e VT_FERRY_TRANSPORT \
    -e VT_FERRY_TCP_HOST \
    -e VT_FERRY_TCP_PORT \
    -e VT_FERRY_STREAM_TRACE \
    -e VT_FERRY_OUTPUT_POLL_INTERVAL="${VT_FERRY_OUTPUT_POLL_INTERVAL}" \
    -e VT_FERRY_OUTPUT_BATCH_SIZE="${VT_FERRY_OUTPUT_BATCH_SIZE}" \
    -e VT_FERRY_GUEST_POOL_BUFFER_COUNT="${VT_FERRY_GUEST_POOL_BUFFER_COUNT}" \
    ${CONTAINER_SHIM_LIBDIR:+-e LD_LIBRARY_PATH="${CONTAINER_SHIM_LIBDIR}"} \
    "${DOCKER_IMAGE}" \
    sh -lc "${docker_ffmpeg_cmd}" \
  >"${LAUNCHER_LOG}" 2>&1
rc=$?
set -e

if [ "${rc}" -ne 0 ]; then
  echo "launcher log:" >&2
  tail -n 80 "${LAUNCHER_LOG}" >&2 || true
  echo "ffmpeg log:" >&2
  tail -n 80 "${LOG_PATH}" >&2 || true
  exit "${rc}"
fi

require_file "${LOG_PATH}"
require_file "${MP4_PATH}"
[ -s "${MP4_PATH}" ] || die "Docker VideoToolbox output is empty: ${MP4_PATH}"
if grep -q "Conversion failed!" "${LOG_PATH}"; then
  die "Docker VideoToolbox encode failed; log: ${LOG_PATH}"
fi
RTIME="$(extract_bench_rtime "${LOG_PATH}")" || die "ffmpeg log is missing bench rtime: ${LOG_PATH}"

probe_output_json="$("${FFPROBE_BIN}" -hide_banner -show_streams -show_format -print_format json "${MP4_PATH}")"
python3 - <<PY
import json
import pathlib

probe = json.loads("""${probe_output_json}""")
stream = next(s for s in probe["streams"] if s.get("codec_type") == "video")
expected_codec = "${DOCKER_EXPECTED_CODEC_NAME}"
if stream.get("codec_name") != expected_codec:
    raise SystemExit(
        f"expected {expected_codec} codec, got {stream.get('codec_name')}"
    )
rtime = float("${RTIME}")
frame_limit = int("${FRAME_LIMIT}")
summary = {
    "reference": {
        "path": "${REFERENCE_VIDEO}",
        "duration_s": float("${duration_seconds}"),
        "frame_limit": frame_limit,
        "width": int("${width}"),
        "height": int("${height}"),
        "slot_count": int("${SLOT_COUNT}"),
        "pixel_format": int("${PIXEL_FORMAT}", 0),
    },
    "docker_videotoolbox_tcp": {
        "rtime_s": rtime,
        "fps": frame_limit / rtime,
        "image": "${DOCKER_IMAGE}",
        "host": "${DOCKER_HOST_NAME}",
        "output": "${MP4_PATH}",
        "codec_name": stream.get("codec_name"),
        "profile": stream.get("profile"),
        "width": stream.get("width"),
        "height": stream.get("height"),
        "pix_fmt": stream.get("pix_fmt"),
        "nb_frames": stream.get("nb_frames"),
    },
}
pathlib.Path("${SUMMARY_JSON}").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")

print(f"Docker ${DOCKER_GUEST_CODEC} over TCP bridge:")
print(f"  frames: {frame_limit}")
print(f"  size: ${width}x${height}")
print(f"  rtime: {rtime:.3f}s")
print(f"  fps: {frame_limit / rtime:.3f}")
print(f"  output: ${MP4_PATH}")
print(f"  summary_json: ${SUMMARY_JSON}")
PY
