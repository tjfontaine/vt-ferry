#!/bin/sh
#
# Multi-rendition transcode benchmark — exercises N concurrent
# VTCompressionSessions in one ffmpeg process, fed by one
# VTDecompressionSession. This is the ABR-ladder pattern: one
# decode, multiple encode outputs at different bitrates (or
# sizes) for adaptive streaming.
#
# Why this matters that single-output transcode doesn't:
# - The worker holds N VTCompressionSessions concurrently. Tests
#   that the per-session state (encode-input pool slots, output
#   queue, callback context) doesn't bleed across sessions.
# - All N encoders share the decode side's pixel-buffer flow.
#   FFmpeg's filter graph fans out the decoded CVPixelBuffer
#   through multiple swscale→format→encoder branches. Each branch
#   acquires its own encode-input pool slot.
# - Worker dispatch handles requests from all N encode sessions
#   plus the decode session in interleaved order. Scheduling
#   regressions show up here.
#
# Worker is single-connection per accept loop, so we measure one
# multi-rendition ffmpeg INSIDE the guest, not multiple guest
# processes. (Multi-process concurrent transcode would need the
# worker to spawn a per-connection thread — a separate
# architectural change.)
#
# Defaults: 1080p input, two 1080p output bitrates (6 Mb/s and
# 3 Mb/s). Both outputs at source dims so a single launcher pool
# at source dims suffices. Override REFERENCE_VIDEO for 4K;
# override RENDITION_BITRATES for different output specs.
#
# Usage:
#   ffmpeg/scripts/benchmark_host_guest_reference_transcode_multirendition.sh
#   FRAME_LIMIT=120 \
#     ffmpeg/scripts/benchmark_host_guest_reference_transcode_multirendition.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"
SMOLVM_ROOT="${REPO_ROOT}/third_party/smolvm"

REFERENCE_VIDEO="${REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4}"
HOST_FFMPEG_BIN="${HOST_FFMPEG_BIN:-ffmpeg}"
FFPROBE_BIN="${FFPROBE_BIN:-ffprobe}"
SMOLVM_BIN="${SMOLVM_BIN:-${SMOLVM_ROOT}/target/release/smolvm}"
SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS:-${SMOLVM_ROOT}/target/agent-rootfs}"
HOST_WORKER_BIN="${HOST_WORKER_BIN:-${REPO_ROOT}/target/debug/vt-ferry-worker}"
BROKER_BIN="${BROKER_BIN:-${REPO_ROOT}/target/release/vt-ferry-broker}"
GUEST_FFMPEG_BIN="${GUEST_FFMPEG_BIN:-${REPO_ROOT}/artifacts/ffmpeg-build/n8.1-linux-debug/ffmpeg}"
SHIM_LIBDIR="${SHIM_LIBDIR:-${REPO_ROOT}/artifacts/ffmpeg-shim-libs/linux-debug}"
SHIM_TARGET_TRIPLE="${SHIM_TARGET_TRIPLE:-aarch64-unknown-linux-gnu}"
STAGE_SHIM_LIBS="${STAGE_SHIM_LIBS:-1}"
VM_IMAGE="${VM_IMAGE:-public.ecr.aws/docker/library/ubuntu:24.04}"
VM_NAME="${VM_NAME:-vt-ferry-multirend-bench-$$}"
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/ffmpeg-reference-transcode-multirend-benchmark-$$}"
SLOT_COUNT="${SLOT_COUNT:-32}"
PIXEL_FORMAT="${PIXEL_FORMAT:-0x34323076}"
FRAME_LIMIT="${FRAME_LIMIT:-300}"
VSOCK_PORT="${VSOCK_PORT:-6605}"

# Bitrates for each rendition. Both outputs at source dims (no
# scaling) so a single launcher pool covers both encoders.
RENDITION_BITRATES="${RENDITION_BITRATES:-6000000 3000000}"

LAUNCHER_PID=""

die() {
  echo "ERROR: $*" >&2
  exit 1
}

GUEST_VT_TRANSPORT_ENV="VT_FERRY_TRANSPORT=vsock VT_FERRY_VSOCK_PORT=${VSOCK_PORT}"

smolvm_cmd() {
  SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS}" \
  "${SMOLVM_BIN}" "$@"
}

cleanup() {
  set +e
  smolvm_cmd machine stop --name "${VM_NAME}" >/dev/null 2>&1 || true
  if [ -n "${LAUNCHER_PID}" ]; then
    wait "${LAUNCHER_PID}" 2>/dev/null || true
  fi
  smolvm_cmd machine delete "${VM_NAME}" -f >/dev/null 2>&1 || true
}

trap cleanup EXIT

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

require_file() {
  [ -e "$1" ] || die "missing required path: $1"
}

guest_repo_path() {
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

extract_bench_field() {
  python3 - "$1" "$2" <<'PY'
import pathlib
import re
import sys

text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8", errors="replace")
field = sys.argv[2]
matches = re.findall(rf"bench:\s+.*?{field}=([0-9.]+)s", text)
if not matches:
    raise SystemExit(1)
print(matches[-1])
PY
}

require_cmd "${HOST_FFMPEG_BIN}"
require_cmd "${FFPROBE_BIN}"
require_cmd python3
require_file "${REFERENCE_VIDEO}"
require_file "${SMOLVM_BIN}"
require_file "${SMOLVM_AGENT_ROOTFS}"
require_file "${HOST_WORKER_BIN}"
require_file "${BROKER_BIN}"

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

mkdir -p "${WORKDIR}"

HOST_VT_LOG="${WORKDIR}/host-vt-multirend-transcode.log"
GUEST_VT_LOG="${WORKDIR}/guest-vt-multirend-transcode.log"
LAUNCHER_LOG="${WORKDIR}/launcher.log"
SUMMARY_JSON="${WORKDIR}/summary.json"
GUEST_REFERENCE_VIDEO="$(guest_repo_path "${REFERENCE_VIDEO}")" || die "REFERENCE_VIDEO must be under ${REPO_ROOT}"
if [ "${USE_VT_FERRY_IMAGE}" = "1" ]; then
  GUEST_FFMPEG_GUEST_BIN="${GUEST_FFMPEG_GUEST_BIN:-/opt/vt-ferry/bin/ffmpeg}"
  GUEST_SHIM_LIBDIR="${GUEST_SHIM_LIBDIR:-}"
else
  GUEST_FFMPEG_GUEST_BIN="$(guest_repo_path "${GUEST_FFMPEG_BIN}")" || die "GUEST_FFMPEG_BIN must be under ${REPO_ROOT}"
  GUEST_SHIM_LIBDIR="$(guest_repo_path "${SHIM_LIBDIR}")" || die "SHIM_LIBDIR must be under ${REPO_ROOT}"
fi
GUEST_WORKDIR="$(guest_repo_path "${WORKDIR}")" || die "WORKDIR must be under ${REPO_ROOT}"

probe_json="$("${FFPROBE_BIN}" -hide_banner -select_streams v:0 -show_streams -show_format -print_format json "${REFERENCE_VIDEO}")"
read width height duration_seconds frame_count <<EOF
$(python3 - <<PY
import json
probe = json.loads("""${probe_json}""")
stream = probe["streams"][0]
fmt = probe["format"]
print(stream["width"], stream["height"], fmt["duration"], stream.get("nb_frames", "0"))
PY
)
EOF

[ -n "${width}" ] || die "failed to parse reference width"
[ -n "${height}" ] || die "failed to parse reference height"

# Count renditions and build the multi-output ffmpeg command tail.
# Each rendition gets its own -filter:v (split by index) → encoder
# → null mux. Source-dim renditions all share the same swscale-
# free format chain.
NUM_RENDITIONS="$(echo "${RENDITION_BITRATES}" | wc -w | tr -d ' ')"
[ "${NUM_RENDITIONS}" -ge 2 ] || die "RENDITION_BITRATES must contain at least 2 bitrates; got: ${RENDITION_BITRATES}"

echo "reference=${REFERENCE_VIDEO}"
echo "reference_width=${width}"
echo "reference_height=${height}"
echo "reference_duration_s=${duration_seconds}"
echo "reference_frames=${frame_count}"
echo "frame_limit=${FRAME_LIMIT:-full}"
echo "renditions=${NUM_RENDITIONS} (${RENDITION_BITRATES})"
echo "vt_ferry_transport=vsock"

frame_limit_args=""
frame_limit_value="${FRAME_LIMIT:-0}"
if [ -n "${FRAME_LIMIT}" ]; then
  frame_limit_args="-frames:v ${FRAME_LIMIT}"
fi

# Build the ffmpeg filter_complex + per-rendition encoder args.
# The filter graph splits the input into N branches; each branch
# is `[v_i] format=nv12 [vout_i]`, then each output maps a
# vout_i to a -c:v h264_videotoolbox encoder with its own bitrate.
# Build the filter graph spec and the per-rendition output args
# separately. The filter graph contains `;` which would be
# interpreted as a shell command separator if it leaked into a
# `sh -lc` invocation; keeping it in a dedicated variable lets us
# single-quote it explicitly on the guest side.
build_filter_complex() {
  i=0
  split_labels=""
  filter_chain=""
  for bitrate in ${RENDITION_BITRATES}; do
    label="v${i}"
    out_label="vout${i}"
    split_labels="${split_labels}[${label}]"
    if [ -z "${filter_chain}" ]; then
      filter_chain="[${label}]format=nv12[${out_label}]"
    else
      filter_chain="${filter_chain};[${label}]format=nv12[${out_label}]"
    fi
    i=$((i + 1))
  done
  printf -- "[0:v]split=%d%s;%s" "${NUM_RENDITIONS}" "${split_labels}" "${filter_chain}"
}

build_output_args() {
  i=0
  output_args=""
  for bitrate in ${RENDITION_BITRATES}; do
    out_label="vout${i}"
    output_args="${output_args} -map [${out_label}] -c:v h264_videotoolbox -profile:v main -b:v ${bitrate} -bf 0 ${frame_limit_args} -f null -"
    i=$((i + 1))
  done
  printf -- "%s" "${output_args}"
}

FILTER_COMPLEX="$(build_filter_complex)"
OUTPUT_ARGS="$(build_output_args)"

echo "host: VideoToolbox multi-rendition transcode (${NUM_RENDITIONS} outputs)"
# shellcheck disable=SC2086
"${HOST_FFMPEG_BIN}" -hide_banner -benchmark -y \
  -hwaccel videotoolbox \
  -i "${REFERENCE_VIDEO}" \
  -an \
  -filter_complex "${FILTER_COMPLEX}" \
  ${OUTPUT_ARGS} >"${HOST_VT_LOG}" 2>&1

"${REPO_ROOT}/third_party/prepare_krun_runtime.sh" >/dev/null
# shellcheck disable=SC1090
. "${REPO_ROOT}/artifacts/krun-runtime/macos-arm64/env.sh"

# Single launcher pool at source dims — works because all
# renditions encode at source dims (different bitrates only).
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

smolvm_cmd machine create \
  --net \
  --image "${VM_IMAGE}" \
  "${VM_NAME}" \
  -v "${REPO_ROOT}:/repo" \
  >/dev/null

SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS}" \
"${BROKER_BIN}" \
  --vsock-port "${VSOCK_PORT}" \
  --pool "${POOL_JSON}" \
  --host-worker "${HOST_WORKER_BIN}" \
  -- "${SMOLVM_BIN}" machine start --name "${VM_NAME}" \
  >"${LAUNCHER_LOG}" 2>&1 &
LAUNCHER_PID=$!

for _ in $(seq 1 120); do
  if grep -q "running (PID" "${LAUNCHER_LOG}" 2>/dev/null; then
    break
  fi
  if ! kill -0 "${LAUNCHER_PID}" 2>/dev/null; then
    die "broker/smolvm exited before VM became ready; log: $(cat "${LAUNCHER_LOG}")"
  fi
  sleep 1
done
sleep 3

smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
  'apt-get update >/tmp/apt-update.log 2>&1 && DEBIAN_FRONTEND=noninteractive apt-get install -y libxcb1 libxcb-shm0 libxau6 libxdmcp6 >/tmp/apt-install.log 2>&1'

echo "guest: VideoToolbox multi-rendition transcode (vsock shim, ${NUM_RENDITIONS} outputs)"
# Single-quote the filter_complex value so the inner `sh -lc`
# parser doesn't interpret the `;` chain separators as shell
# command separators.
if [ -n "${GUEST_SHIM_LIBDIR}" ]; then
  GUEST_LD_PREFIX="LD_LIBRARY_PATH=${GUEST_SHIM_LIBDIR}"
else
  GUEST_LD_PREFIX=""
fi
smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
  "env ${GUEST_VT_TRANSPORT_ENV} ${GUEST_LD_PREFIX} ${GUEST_FFMPEG_GUEST_BIN} -hide_banner -benchmark -y -hwaccel videotoolbox -i ${GUEST_REFERENCE_VIDEO} -an -filter_complex '${FILTER_COMPLEX}' ${OUTPUT_ARGS} > ${GUEST_WORKDIR}/$(basename "${GUEST_VT_LOG}") 2>&1"

for path in "${HOST_VT_LOG}" "${GUEST_VT_LOG}"; do
  require_file "${path}"
done

HOST_VT_RTIME="$(extract_bench_rtime "${HOST_VT_LOG}")"
GUEST_VT_RTIME="$(extract_bench_rtime "${GUEST_VT_LOG}")"
HOST_VT_UTIME="$(extract_bench_field "${HOST_VT_LOG}" utime || echo 0)"
HOST_VT_STIME="$(extract_bench_field "${HOST_VT_LOG}" stime || echo 0)"
GUEST_VT_UTIME="$(extract_bench_field "${GUEST_VT_LOG}" utime || echo 0)"
GUEST_VT_STIME="$(extract_bench_field "${GUEST_VT_LOG}" stime || echo 0)"

python3 - <<PY
import json
from pathlib import Path

reference_duration_s = float("${duration_seconds}")
reference_frame_count = int("${frame_count}")
frame_limit = int("${frame_limit_value}")
num_renditions = int("${NUM_RENDITIONS}")

input_frames = frame_limit if frame_limit > 0 else reference_frame_count
encoded_frames = input_frames * num_renditions
source_fps = (reference_frame_count / reference_duration_s) if reference_duration_s > 0 else 0.0
input_duration_s = (input_frames / source_fps) if source_fps > 0 else reference_duration_s

host_vt = float("${HOST_VT_RTIME}")
guest_vt = float("${GUEST_VT_RTIME}")
host_vt_cpu = float("${HOST_VT_UTIME}") + float("${HOST_VT_STIME}")
guest_vt_cpu = float("${GUEST_VT_UTIME}") + float("${GUEST_VT_STIME}")

def cores(cpu_s, rtime_s):
    return (cpu_s / rtime_s) if rtime_s > 0 else 0.0

summary = {
    "reference": {
        "path": "${REFERENCE_VIDEO}",
        "duration_s": reference_duration_s,
        "frame_count": reference_frame_count,
        "frame_limit": frame_limit,
        "width": int("${width}"),
        "height": int("${height}"),
    },
    "renditions": {
        "count": num_renditions,
        "bitrates": "${RENDITION_BITRATES}".split(),
    },
    "transcoded": {
        "input_frames": input_frames,
        "encoded_frames_total": encoded_frames,
        "input_duration_s": input_duration_s,
        "source_fps": source_fps,
    },
    "host": {
        "videotoolbox_rtime_s": host_vt,
        "videotoolbox_cpu_s": host_vt_cpu,
        "videotoolbox_avg_cores": cores(host_vt_cpu, host_vt),
        "videotoolbox_x_realtime": input_duration_s / host_vt,
        "videotoolbox_input_fps": input_frames / host_vt if host_vt > 0 else None,
    },
    "guest": {
        "transport": "vsock",
        "videotoolbox_rtime_s": guest_vt,
        "videotoolbox_cpu_s": guest_vt_cpu,
        "videotoolbox_avg_cores": cores(guest_vt_cpu, guest_vt),
        "videotoolbox_x_realtime": input_duration_s / guest_vt,
        "videotoolbox_input_fps": input_frames / guest_vt if guest_vt > 0 else None,
    },
    "overhead": {
        "guest_vs_host_videotoolbox_ratio": (guest_vt / host_vt) if host_vt > 0 else None,
        "guest_videotoolbox_extra_seconds_vs_host": guest_vt - host_vt,
    },
}

Path("${SUMMARY_JSON}").write_text(json.dumps(summary, indent=2) + "\\n", encoding="utf-8")

print("Reference:")
print(f"  path: ${REFERENCE_VIDEO}")
print(f"  duration_s: {reference_duration_s:.3f}")
print(f"  frames: {reference_frame_count}")
print(f"  size: ${width}x${height}")
print(f"  source_fps: {source_fps:.3f}")
print("")
print(f"Renditions: {num_renditions}")
print(f"  bitrates: ${RENDITION_BITRATES}")
print("")
print(f"Transcoded {input_frames} input frames × {num_renditions} renditions = {encoded_frames} encoded frames")
print(f"  input_duration_s: {input_duration_s:.3f}")
print("")
print("Host VT (multi-rendition):")
print(f"  rtime_s: {host_vt:.3f}")
print(f"  x_realtime: {input_duration_s / host_vt:.3f}x (vs input wallclock)")
if host_vt > 0:
    print(f"  input_fps: {input_frames / host_vt:.1f}")
print(f"  cpu_s: {host_vt_cpu:.3f} ({cores(host_vt_cpu, host_vt):.2f} cores avg)")
print("")
print("Guest VT (multi-rendition, vsock shim):")
print(f"  rtime_s: {guest_vt:.3f}")
print(f"  x_realtime: {input_duration_s / guest_vt:.3f}x")
if guest_vt > 0:
    print(f"  input_fps: {input_frames / guest_vt:.1f}")
print(f"  cpu_s: {guest_vt_cpu:.3f} ({cores(guest_vt_cpu, guest_vt):.2f} cores avg)")
print("")
print("Guest Overhead vs Host:")
if host_vt > 0:
    print(f"  videotoolbox_ratio: {guest_vt / host_vt:.3f}x")
print(f"  guest_videotoolbox_extra_seconds: {guest_vt - host_vt:.3f}")
print("")
print("Notes:")
print("  one VTDecompressionSession + N concurrent VTCompressionSessions")
print("  all renditions encode at source dims (different bitrates only)")
print("  all outputs muxed to /dev/null — measures encoder/decoder pipeline only")
print("")
print(f"summary_json: ${SUMMARY_JSON}")
print(f"workdir: ${WORKDIR}")
PY
