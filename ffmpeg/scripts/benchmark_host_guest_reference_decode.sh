#!/bin/sh
#
# Decode-side companion to benchmark_host_guest_reference_encode.sh.
#
# Decodes the bundled reference clip via host ffmpeg and via guest
# ffmpeg over the smolvm + libkrun vsock path, capturing
# `-benchmark` rtime / utime / stime for each. Writes a JSON
# summary at ${WORKDIR}/summary.json that the gate (v1_decode_perf_gate.sh)
# consumes.
#
# Two encoders × two hosts = four decode runs total:
#
#   host: software decode (libavcodec h264 — the FFmpeg built-in)
#   host: VideoToolbox decode (-hwaccel videotoolbox)
#   guest: software decode (distro ffmpeg, no shim)
#   guest: VideoToolbox decode (shimmed ffmpeg, vsock to host worker)
#
# Output is muxed to /dev/null on the host (rawvideo would let
# benchmark numbers be dominated by container/disk overhead;
# `-f null -` is the canonical "decode-and-discard" pattern). On
# the guest we write to a tmpfs path under /tmp for the same reason.
#
# Why a separate decode benchmark vs. extending the encode one:
#   - decoder selection is structurally different (-c:v vs -hwaccel)
#   - the metric of interest is decode wallclock + CPU, which has
#     no overlap with encode-side rate-control / GOP machinery
#   - the JSON summary's keys ("decode_*", "guest_vt_decode_*") let
#     the gate assertions stay independent

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
VM_NAME="${VM_NAME:-vt-ferry-decode-bench-$$}"
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/ffmpeg-reference-decode-benchmark-$$}"
SLOT_COUNT="${SLOT_COUNT:-4}"
PIXEL_FORMAT="${PIXEL_FORMAT:-0x34323076}"
FRAME_LIMIT="${FRAME_LIMIT:-300}"
VSOCK_PORT="${VSOCK_PORT:-6602}"

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

# Same regex extractors used by the encode benchmark — `-benchmark`
# emits identical lines on both encode and decode runs.
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

# Image mode (USE_VT_FERRY_IMAGE=1): patched ffmpeg + shim live
# inside VM_IMAGE under /opt/vt-ferry/. Skip host-side staging.
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

HOST_SW_LOG="${WORKDIR}/host-sw-decode.log"
HOST_VT_LOG="${WORKDIR}/host-vt-decode.log"
GUEST_SW_LOG="${WORKDIR}/guest-sw-decode.log"
GUEST_VT_LOG="${WORKDIR}/guest-vt-decode.log"
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

echo "reference=${REFERENCE_VIDEO}"
echo "reference_width=${width}"
echo "reference_height=${height}"
echo "reference_duration_s=${duration_seconds}"
echo "reference_frames=${frame_count}"
echo "frame_limit=${FRAME_LIMIT:-full}"
echo "vt_ferry_transport=vsock"

frame_limit_args=""
frame_limit_value="${FRAME_LIMIT:-0}"
frame_limit_display="${FRAME_LIMIT:-full}"
if [ -n "${FRAME_LIMIT}" ]; then
  frame_limit_args="-frames:v ${FRAME_LIMIT}"
fi

# Host-side decode runs. `-f null -` discards the decoded frames so
# the benchmark numbers reflect decode wallclock + CPU, not disk.
echo "host: software decode (libavcodec h264)"
"${HOST_FFMPEG_BIN}" -hide_banner -benchmark -y \
  -i "${REFERENCE_VIDEO}" \
  -map 0:v:0 \
  -an \
  ${frame_limit_args} \
  -f null - >"${HOST_SW_LOG}" 2>&1

echo "host: VideoToolbox decode (-hwaccel videotoolbox)"
"${HOST_FFMPEG_BIN}" -hide_banner -benchmark -y \
  -hwaccel videotoolbox \
  -i "${REFERENCE_VIDEO}" \
  -map 0:v:0 \
  -an \
  ${frame_limit_args} \
  -f null - >"${HOST_VT_LOG}" 2>&1

"${REPO_ROOT}/third_party/prepare_krun_runtime.sh" >/dev/null
# shellcheck disable=SC1090
. "${REPO_ROOT}/artifacts/krun-runtime/macos-arm64/env.sh"

# Pre-register an IOSurface pool the shim's auto-allocated decode
# output pool (1080p NV12 > 1.5 MiB inline cap, so the pool path
# fires) can claim via the worker's zero-copy IOSurface fast path.
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

# Distro ffmpeg gives us the libavcodec software decode baseline
# inside the guest; libxcb deps are needed for the shimmed ffmpeg.
smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
  'apt-get update >/tmp/apt-update.log 2>&1 && DEBIAN_FRONTEND=noninteractive apt-get install -y ffmpeg libxcb1 libxcb-shm0 libxau6 libxdmcp6 >/tmp/apt-install.log 2>&1'

echo "guest: software decode (distro ffmpeg)"
# /usr/bin/ffmpeg explicitly so we use the distro libavcodec
# baseline; in image mode /opt/vt-ferry/bin/ffmpeg is also on
# PATH and would shadow it.
smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
  "/usr/bin/ffmpeg -hide_banner -benchmark -y -i ${GUEST_REFERENCE_VIDEO} -map 0:v:0 -an ${frame_limit_args} -f null - > ${GUEST_WORKDIR}/$(basename "${GUEST_SW_LOG}") 2>&1"

echo "guest: VideoToolbox decode (vsock shim)"
# Image mode: ldconfig already wired the shim libs; skip explicit
# LD_LIBRARY_PATH (an empty value would shadow default search).
if [ -n "${GUEST_SHIM_LIBDIR}" ]; then
  GUEST_LD_PREFIX="LD_LIBRARY_PATH=${GUEST_SHIM_LIBDIR}"
else
  GUEST_LD_PREFIX=""
fi
smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
  "env ${GUEST_VT_TRANSPORT_ENV} ${GUEST_LD_PREFIX} ${GUEST_FFMPEG_GUEST_BIN} -hide_banner -benchmark -y -hwaccel videotoolbox -i ${GUEST_REFERENCE_VIDEO} -map 0:v:0 -an ${frame_limit_args} -f null - > ${GUEST_WORKDIR}/$(basename "${GUEST_VT_LOG}") 2>&1"

for path in "${HOST_SW_LOG}" "${HOST_VT_LOG}" "${GUEST_SW_LOG}" "${GUEST_VT_LOG}"; do
  require_file "${path}"
done

HOST_SW_RTIME="$(extract_bench_rtime "${HOST_SW_LOG}")"
HOST_VT_RTIME="$(extract_bench_rtime "${HOST_VT_LOG}")"
GUEST_SW_RTIME="$(extract_bench_rtime "${GUEST_SW_LOG}")"
GUEST_VT_RTIME="$(extract_bench_rtime "${GUEST_VT_LOG}")"

HOST_SW_UTIME="$(extract_bench_field "${HOST_SW_LOG}" utime || echo 0)"
HOST_SW_STIME="$(extract_bench_field "${HOST_SW_LOG}" stime || echo 0)"
HOST_VT_UTIME="$(extract_bench_field "${HOST_VT_LOG}" utime || echo 0)"
HOST_VT_STIME="$(extract_bench_field "${HOST_VT_LOG}" stime || echo 0)"
GUEST_SW_UTIME="$(extract_bench_field "${GUEST_SW_LOG}" utime || echo 0)"
GUEST_SW_STIME="$(extract_bench_field "${GUEST_SW_LOG}" stime || echo 0)"
GUEST_VT_UTIME="$(extract_bench_field "${GUEST_VT_LOG}" utime || echo 0)"
GUEST_VT_STIME="$(extract_bench_field "${GUEST_VT_LOG}" stime || echo 0)"

python3 - <<PY
import json
from pathlib import Path

reference_duration_s = float("${duration_seconds}")
reference_frame_count = int("${frame_count}")
frame_limit = int("${frame_limit_value}")

decoded_frames = frame_limit if frame_limit > 0 else reference_frame_count
source_fps = (reference_frame_count / reference_duration_s) if reference_duration_s > 0 else 0.0
decoded_duration_s = (decoded_frames / source_fps) if source_fps > 0 else reference_duration_s

host_sw = float("${HOST_SW_RTIME}")
host_vt = float("${HOST_VT_RTIME}")
guest_sw = float("${GUEST_SW_RTIME}")
guest_vt = float("${GUEST_VT_RTIME}")

host_sw_cpu = float("${HOST_SW_UTIME}") + float("${HOST_SW_STIME}")
host_vt_cpu = float("${HOST_VT_UTIME}") + float("${HOST_VT_STIME}")
guest_sw_cpu = float("${GUEST_SW_UTIME}") + float("${GUEST_SW_STIME}")
guest_vt_cpu = float("${GUEST_VT_UTIME}") + float("${GUEST_VT_STIME}")

def cores(cpu_s, rtime_s):
    return (cpu_s / rtime_s) if rtime_s > 0 else 0.0

def cpu_efficiency(vt_cpu, sw_cpu):
    """Fraction of libavcodec's CPU cost that VT decode spends.
    A value of 0.10 means VT does the same decode for 10% of the
    CPU. Lower is better (libavcodec is the baseline at 1.00)."""
    return (vt_cpu / sw_cpu) if sw_cpu > 0 else None

summary = {
    "reference": {
        "path": "${REFERENCE_VIDEO}",
        "duration_s": reference_duration_s,
        "frame_count": reference_frame_count,
        "frame_limit": frame_limit,
        "width": int("${width}"),
        "height": int("${height}"),
    },
    "decoded": {
        "frames": decoded_frames,
        "duration_s": decoded_duration_s,
        "source_fps": source_fps,
    },
    "host": {
        "sw_decode_rtime_s": host_sw,
        "videotoolbox_rtime_s": host_vt,
        "sw_decode_cpu_s": host_sw_cpu,
        "videotoolbox_cpu_s": host_vt_cpu,
        "sw_decode_avg_cores": cores(host_sw_cpu, host_sw),
        "videotoolbox_avg_cores": cores(host_vt_cpu, host_vt),
        "videotoolbox_speedup_vs_sw": (host_sw / host_vt) if host_vt > 0 else None,
        "sw_decode_x_realtime": decoded_duration_s / host_sw,
        "videotoolbox_x_realtime": decoded_duration_s / host_vt,
    },
    "guest": {
        "transport": "vsock",
        "sw_decode_rtime_s": guest_sw,
        "videotoolbox_rtime_s": guest_vt,
        "sw_decode_cpu_s": guest_sw_cpu,
        "videotoolbox_cpu_s": guest_vt_cpu,
        "sw_decode_avg_cores": cores(guest_sw_cpu, guest_sw),
        "videotoolbox_avg_cores": cores(guest_vt_cpu, guest_vt),
        "videotoolbox_speedup_vs_sw": (guest_sw / guest_vt) if guest_vt > 0 else None,
        "sw_decode_x_realtime": decoded_duration_s / guest_sw,
        "videotoolbox_x_realtime": decoded_duration_s / guest_vt,
    },
    "overhead": {
        "guest_vs_host_sw_decode_ratio": (guest_sw / host_sw) if host_sw > 0 else None,
        "guest_vs_host_videotoolbox_ratio": (guest_vt / host_vt) if host_vt > 0 else None,
        "guest_videotoolbox_extra_seconds_vs_host": guest_vt - host_vt,
    },
    "value_of_hardware_decode": {
        "host_vt_cpu_efficiency_vs_sw": cpu_efficiency(host_vt_cpu, host_sw_cpu),
        "guest_vt_cpu_efficiency_vs_sw": cpu_efficiency(guest_vt_cpu, guest_sw_cpu),
    },
}

if decoded_frames > 0:
    summary["host"]["sw_decode_fps"] = decoded_frames / host_sw
    summary["host"]["videotoolbox_fps"] = decoded_frames / host_vt
    summary["guest"]["sw_decode_fps"] = decoded_frames / guest_sw
    summary["guest"]["videotoolbox_fps"] = decoded_frames / guest_vt

Path("${SUMMARY_JSON}").write_text(json.dumps(summary, indent=2) + "\\n", encoding="utf-8")

print("Reference:")
print(f"  path: ${REFERENCE_VIDEO}")
print(f"  duration_s: {reference_duration_s:.3f}")
print(f"  frames: {reference_frame_count}")
print(f"  frame_limit: ${frame_limit_display}")
print(f"  size: ${width}x${height}")
print(f"  source_fps: {source_fps:.3f}")
print("")
print(f"Decoded {decoded_frames} frames ({decoded_duration_s:.3f}s of source).")
print("")
print("Host:")
print(f"  sw_decode_rtime_s: {host_sw:.3f}")
print(f"  videotoolbox_rtime_s: {host_vt:.3f}")
if host_vt > 0:
    print(f"  videotoolbox_speedup_vs_sw: {host_sw / host_vt:.3f}x")
print(f"  sw_decode_x_realtime: {decoded_duration_s / host_sw:.3f}x")
print(f"  videotoolbox_x_realtime: {decoded_duration_s / host_vt:.3f}x")
print(f"  sw_decode_cpu_s: {host_sw_cpu:.3f} ({cores(host_sw_cpu, host_sw):.2f} cores avg)")
print(f"  videotoolbox_cpu_s: {host_vt_cpu:.3f} ({cores(host_vt_cpu, host_vt):.2f} cores avg)")
if decoded_frames > 0:
    print(f"  sw_decode_fps: {decoded_frames / host_sw:.3f}")
    print(f"  videotoolbox_fps: {decoded_frames / host_vt:.3f}")
print("")
print("Guest:")
print(f"  sw_decode_rtime_s: {guest_sw:.3f}")
print(f"  videotoolbox_rtime_s: {guest_vt:.3f}")
if guest_vt > 0:
    print(f"  videotoolbox_speedup_vs_sw: {guest_sw / guest_vt:.3f}x")
print(f"  sw_decode_x_realtime: {decoded_duration_s / guest_sw:.3f}x")
print(f"  videotoolbox_x_realtime: {decoded_duration_s / guest_vt:.3f}x")
print(f"  sw_decode_cpu_s: {guest_sw_cpu:.3f} ({cores(guest_sw_cpu, guest_sw):.2f} cores avg)")
print(f"  videotoolbox_cpu_s: {guest_vt_cpu:.3f} ({cores(guest_vt_cpu, guest_vt):.2f} cores avg)")
if decoded_frames > 0:
    print(f"  sw_decode_fps: {decoded_frames / guest_sw:.3f}")
    print(f"  videotoolbox_fps: {decoded_frames / guest_vt:.3f}")
print("")
print("Value of Hardware Decode (lower CPU cost ratio = better):")
host_eff = cpu_efficiency(host_vt_cpu, host_sw_cpu)
guest_eff = cpu_efficiency(guest_vt_cpu, guest_sw_cpu)
if host_eff is not None:
    print(f"  host_vt_cpu_vs_sw: {host_eff:.3f} "
          f"({(1.0 - host_eff) * 100:.1f}% CPU saved)")
if guest_eff is not None:
    print(f"  guest_vt_cpu_vs_sw: {guest_eff:.3f} "
          f"({(1.0 - guest_eff) * 100:.1f}% CPU saved)")
print("")
print("Guest Overhead vs Host:")
if host_sw > 0:
    print(f"  sw_decode_ratio: {guest_sw / host_sw:.3f}x")
if host_vt > 0:
    print(f"  videotoolbox_ratio: {guest_vt / host_vt:.3f}x")
print(f"  guest_videotoolbox_extra_seconds: {guest_vt - host_vt:.3f}")
print("")
print("Notes:")
print("  guest sw_decode baseline uses the distro ffmpeg inside the VM")
print("  guest videotoolbox uses the shimmed ffmpeg build over vsock")
print("  output is muxed to /dev/null on both sides — measures decode wallclock + CPU")
print("")
print(f"summary_json: ${SUMMARY_JSON}")
print(f"workdir: ${WORKDIR}")
PY
