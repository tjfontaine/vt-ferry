#!/bin/sh
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
VM_NAME="${VM_NAME:-vt-ferry-cpu-budget-$$}"
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/ffmpeg-cpu-budget-$$}"
SLOT_COUNT="${SLOT_COUNT:-4}"
PIXEL_FORMAT="${PIXEL_FORMAT:-0x34323076}"
FRAME_LIMIT="${FRAME_LIMIT:-300}"
CONTENTION_SECONDS="${CONTENTION_SECONDS:-3}"
OPENSSL_BLOCK_BYTES="${OPENSSL_BLOCK_BYTES:-16384}"
SOFTWARE_ARGS="${SOFTWARE_ARGS:--preset medium -profile:v main -b:v 6000000 -bf 0}"
VIDEOTOOLBOX_ARGS="${VIDEOTOOLBOX_ARGS:--profile:v main -b:v 6000000 -bf 0}"
VSOCK_PORT="${VSOCK_PORT:-6600}"

LAUNCHER_PID=""

die() {
  echo "ERROR: $*" >&2
  exit 1
}

GUEST_VT_TRANSPORT_ENV="VT_FERRY_TRANSPORT=vsock VT_FERRY_VSOCK_PORT=${VSOCK_PORT}"

cleanup() {
  set +e
  stop_vm_instance || true
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

capture_process_times() {
  pid="$1"
  if [ -n "${pid}" ] && kill -0 "${pid}" 2>/dev/null; then
    ps -p "${pid}" -o utime=,stime= | awk 'NR==1 { print $1, $2 }'
  else
    echo "0:00.00 0:00.00"
  fi
}

capture_host_cpu_snapshot() {
  label="$1"
  worker_pid="$(pgrep -P "${LAUNCHER_PID}" -f vt-ferry-worker | head -n 1 || true)"

  set -- $(capture_process_times "${LAUNCHER_PID}")
  launcher_utime="$1"
  launcher_stime="$2"

  set -- $(capture_process_times "${worker_pid}")
  worker_utime="$1"
  worker_stime="$2"

  cat >"${WORKDIR}/${label}.hostcpu" <<EOF
launcher_pid=${LAUNCHER_PID}
launcher_utime=${launcher_utime}
launcher_stime=${launcher_stime}
worker_pid=${worker_pid:-0}
worker_utime=${worker_utime}
worker_stime=${worker_stime}
EOF
}

smolvm_cmd() {
  SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS}" \
  "${SMOLVM_BIN}" "$@"
}

run_guest() {
  command_string="$1"
  smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc "${command_string}"
}

stop_vm_instance() {
  set +e
  smolvm_cmd machine stop --name "${VM_NAME}" >/dev/null 2>&1 || true
  if [ -n "${LAUNCHER_PID}" ]; then
    kill "${LAUNCHER_PID}" >/dev/null 2>&1 || true
    LAUNCHER_PID=""
  fi
  smolvm_cmd machine delete "${VM_NAME}" -f >/dev/null 2>&1 || true
}

require_cmd "${HOST_FFMPEG_BIN}"
require_cmd "${FFPROBE_BIN}"
require_cmd python3
require_cmd ps
require_cmd pgrep
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

GUEST_SW_MP4="${WORKDIR}/guest-libx264.mp4"
GUEST_SW_LOG="${WORKDIR}/guest-libx264.log"
GUEST_SW_TIME="${WORKDIR}/guest-libx264.time"
GUEST_VT_MP4="${WORKDIR}/guest-videotoolbox.mp4"
GUEST_VT_LOG="${WORKDIR}/guest-videotoolbox.log"
GUEST_VT_TIME="${WORKDIR}/guest-videotoolbox.time"
GUEST_SSL_BASE_LOG="${WORKDIR}/guest-openssl-alone.log"
GUEST_SSL_BASE_TIME="${WORKDIR}/guest-openssl-alone.time"
GUEST_VT_SSL_BASE_LOG="${WORKDIR}/guest-openssl-alone-vt-vm.log"
GUEST_VT_SSL_BASE_TIME="${WORKDIR}/guest-openssl-alone-vt-vm.time"
GUEST_SW_CONT_MP4="${WORKDIR}/guest-libx264-contended.mp4"
GUEST_SW_CONT_LOG="${WORKDIR}/guest-libx264-contended.log"
GUEST_SW_CONT_TIME="${WORKDIR}/guest-libx264-contended.time"
GUEST_SW_SSL_CONT_LOG="${WORKDIR}/guest-openssl-with-libx264.log"
GUEST_SW_SSL_CONT_TIME="${WORKDIR}/guest-openssl-with-libx264.time"
GUEST_VT_CONT_MP4="${WORKDIR}/guest-videotoolbox-contended.mp4"
GUEST_VT_CONT_LOG="${WORKDIR}/guest-videotoolbox-contended.log"
GUEST_VT_CONT_TIME="${WORKDIR}/guest-videotoolbox-contended.time"
GUEST_VT_SSL_CONT_LOG="${WORKDIR}/guest-openssl-with-videotoolbox.log"
GUEST_VT_SSL_CONT_TIME="${WORKDIR}/guest-openssl-with-videotoolbox.time"
LAUNCHER_LOG_SW="${WORKDIR}/launcher-software.log"
LAUNCHER_LOG_VT_ISO="${WORKDIR}/launcher-vt-isolated.log"
LAUNCHER_LOG_VT_CONT="${WORKDIR}/launcher-vt-contended.log"
SUMMARY_JSON="${WORKDIR}/summary.json"

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
echo "frame_limit=${FRAME_LIMIT}"
echo "contention_seconds=${CONTENTION_SECONDS}"
echo "openssl_block_bytes=${OPENSSL_BLOCK_BYTES}"
echo "vt_ferry_transport=vsock"

frame_limit_args="-frames:v ${FRAME_LIMIT}"
frame_limit_value="${FRAME_LIMIT:-0}"
GUEST_REFERENCE_VIDEO="$(guest_repo_path "${REFERENCE_VIDEO}")" || die "REFERENCE_VIDEO must be under ${REPO_ROOT}"
if [ "${USE_VT_FERRY_IMAGE}" = "1" ]; then
  GUEST_FFMPEG_BIN_GUEST="${GUEST_FFMPEG_BIN_GUEST:-/opt/vt-ferry/bin/ffmpeg}"
  GUEST_SHIM_LIBDIR="${GUEST_SHIM_LIBDIR:-}"
else
  GUEST_FFMPEG_BIN_GUEST="$(guest_repo_path "${GUEST_FFMPEG_BIN}")" || die "GUEST_FFMPEG_BIN must be under ${REPO_ROOT}"
  GUEST_SHIM_LIBDIR="$(guest_repo_path "${SHIM_LIBDIR}")" || die "SHIM_LIBDIR must be under ${REPO_ROOT}"
fi
GUEST_WORKDIR="$(guest_repo_path "${WORKDIR}")" || die "WORKDIR must be under ${REPO_ROOT}"

"${REPO_ROOT}/third_party/prepare_krun_runtime.sh" >/dev/null
# shellcheck disable=SC1090
. "${REPO_ROOT}/artifacts/krun-runtime/macos-arm64/env.sh"

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

guest_sw_encode_cmd="/usr/bin/time -v -o ${GUEST_WORKDIR}/$(basename "${GUEST_SW_TIME}") /usr/bin/ffmpeg -hide_banner -benchmark -y -i ${GUEST_REFERENCE_VIDEO} -map 0:v:0 -an ${frame_limit_args} -vf format=nv12 -c:v libx264 ${SOFTWARE_ARGS} ${GUEST_WORKDIR}/$(basename "${GUEST_SW_MP4}") > ${GUEST_WORKDIR}/$(basename "${GUEST_SW_LOG}") 2>&1"
guest_vt_encode_cmd="/usr/bin/time -v -o ${GUEST_WORKDIR}/$(basename "${GUEST_VT_TIME}") env ${GUEST_VT_TRANSPORT_ENV} ${GUEST_SHIM_LIBDIR:+LD_LIBRARY_PATH=${GUEST_SHIM_LIBDIR}} ${GUEST_FFMPEG_BIN_GUEST} -hide_banner -benchmark -y -i ${GUEST_REFERENCE_VIDEO} -map 0:v:0 -an ${frame_limit_args} -vf format=nv12 -c:v h264_videotoolbox ${VIDEOTOOLBOX_ARGS} ${GUEST_WORKDIR}/$(basename "${GUEST_VT_MP4}") > ${GUEST_WORKDIR}/$(basename "${GUEST_VT_LOG}") 2>&1"
guest_ssl_base_cmd="/usr/bin/time -v -o ${GUEST_WORKDIR}/$(basename "${GUEST_SSL_BASE_TIME}") openssl speed -seconds ${CONTENTION_SECONDS} -bytes ${OPENSSL_BLOCK_BYTES} sha256 > ${GUEST_WORKDIR}/$(basename "${GUEST_SSL_BASE_LOG}") 2>&1"
guest_vt_ssl_base_cmd="/usr/bin/time -v -o ${GUEST_WORKDIR}/$(basename "${GUEST_VT_SSL_BASE_TIME}") openssl speed -seconds ${CONTENTION_SECONDS} -bytes ${OPENSSL_BLOCK_BYTES} sha256 > ${GUEST_WORKDIR}/$(basename "${GUEST_VT_SSL_BASE_LOG}") 2>&1"
guest_sw_contended_cmd="( /usr/bin/time -v -o ${GUEST_WORKDIR}/$(basename "${GUEST_SW_CONT_TIME}") /usr/bin/ffmpeg -hide_banner -benchmark -y -i ${GUEST_REFERENCE_VIDEO} -map 0:v:0 -an ${frame_limit_args} -vf format=nv12 -c:v libx264 ${SOFTWARE_ARGS} ${GUEST_WORKDIR}/$(basename "${GUEST_SW_CONT_MP4}") > ${GUEST_WORKDIR}/$(basename "${GUEST_SW_CONT_LOG}") 2>&1 ) & enc_pid=\$!; /usr/bin/time -v -o ${GUEST_WORKDIR}/$(basename "${GUEST_SW_SSL_CONT_TIME}") openssl speed -seconds ${CONTENTION_SECONDS} -bytes ${OPENSSL_BLOCK_BYTES} sha256 > ${GUEST_WORKDIR}/$(basename "${GUEST_SW_SSL_CONT_LOG}") 2>&1; wait \$enc_pid"
guest_vt_contended_cmd="( /usr/bin/time -v -o ${GUEST_WORKDIR}/$(basename "${GUEST_VT_CONT_TIME}") env ${GUEST_VT_TRANSPORT_ENV} ${GUEST_SHIM_LIBDIR:+LD_LIBRARY_PATH=${GUEST_SHIM_LIBDIR}} ${GUEST_FFMPEG_BIN_GUEST} -hide_banner -benchmark -y -i ${GUEST_REFERENCE_VIDEO} -map 0:v:0 -an ${frame_limit_args} -vf format=nv12 -c:v h264_videotoolbox ${VIDEOTOOLBOX_ARGS} ${GUEST_WORKDIR}/$(basename "${GUEST_VT_CONT_MP4}") > ${GUEST_WORKDIR}/$(basename "${GUEST_VT_CONT_LOG}") 2>&1 ) & enc_pid=\$!; /usr/bin/time -v -o ${GUEST_WORKDIR}/$(basename "${GUEST_VT_SSL_CONT_TIME}") openssl speed -seconds ${CONTENTION_SECONDS} -bytes ${OPENSSL_BLOCK_BYTES} sha256 > ${GUEST_WORKDIR}/$(basename "${GUEST_VT_SSL_CONT_LOG}") 2>&1; wait \$enc_pid"

start_vm_instance() {
  launcher_log="$1"
  stop_vm_instance || true

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
    >"${launcher_log}" 2>&1 &
  LAUNCHER_PID=$!

  for _ in $(seq 1 120); do
    if grep -q "running (PID" "${launcher_log}" 2>/dev/null; then
      break
    fi
    if ! kill -0 "${LAUNCHER_PID}" 2>/dev/null; then
      die "broker/smolvm exited before VM became ready; log: $(cat "${launcher_log}")"
    fi
    sleep 1
  done
  sleep 3

  run_guest 'apt-get update >/tmp/apt-update.log 2>&1 && DEBIAN_FRONTEND=noninteractive apt-get install -y ffmpeg openssl time libxcb1 libxcb-shm0 libxau6 libxdmcp6 >/tmp/apt-install.log 2>&1'
  run_guest "/usr/bin/ffmpeg -hide_banner -encoders | grep -q 'libx264'"
  run_guest "${GUEST_SHIM_LIBDIR:+LD_LIBRARY_PATH=${GUEST_SHIM_LIBDIR}} ${GUEST_FFMPEG_BIN_GUEST} -hide_banner -encoders | grep -q 'h264_videotoolbox'"
  run_guest "openssl version >/dev/null"
  run_guest "/usr/bin/time -p true >/dev/null 2>/dev/null"
}

start_vm_instance "${LAUNCHER_LOG_SW}"
echo "guest: openssl baseline"
run_guest "${guest_ssl_base_cmd}"

echo "guest: libx264 isolated"
capture_host_cpu_snapshot "guest-libx264.before"
run_guest "${guest_sw_encode_cmd}"
capture_host_cpu_snapshot "guest-libx264.after"

echo "guest: libx264 + openssl contention"
capture_host_cpu_snapshot "guest-libx264-contended.before"
run_guest "${guest_sw_contended_cmd}"
capture_host_cpu_snapshot "guest-libx264-contended.after"

start_vm_instance "${LAUNCHER_LOG_VT_ISO}"
echo "guest: h264_videotoolbox isolated"
capture_host_cpu_snapshot "guest-videotoolbox.before"
run_guest "${guest_vt_encode_cmd}"
capture_host_cpu_snapshot "guest-videotoolbox.after"

start_vm_instance "${LAUNCHER_LOG_VT_CONT}"
echo "guest: h264_videotoolbox + openssl contention"
run_guest "${guest_vt_ssl_base_cmd}"
capture_host_cpu_snapshot "guest-videotoolbox-contended.before"
run_guest "${guest_vt_contended_cmd}"
capture_host_cpu_snapshot "guest-videotoolbox-contended.after"

for path in \
  "${GUEST_SW_MP4}" "${GUEST_VT_MP4}" "${GUEST_SW_CONT_MP4}" "${GUEST_VT_CONT_MP4}" \
  "${GUEST_SW_LOG}" "${GUEST_VT_LOG}" "${GUEST_SW_CONT_LOG}" "${GUEST_VT_CONT_LOG}" \
  "${GUEST_SW_TIME}" "${GUEST_VT_TIME}" "${GUEST_SSL_BASE_LOG}" "${GUEST_SSL_BASE_TIME}" \
  "${GUEST_VT_SSL_BASE_LOG}" "${GUEST_VT_SSL_BASE_TIME}" \
  "${GUEST_SW_SSL_CONT_LOG}" "${GUEST_SW_SSL_CONT_TIME}" \
  "${GUEST_VT_SSL_CONT_LOG}" "${GUEST_VT_SSL_CONT_TIME}" \
  "${LAUNCHER_LOG_SW}" "${LAUNCHER_LOG_VT_ISO}" "${LAUNCHER_LOG_VT_CONT}" \
  "${WORKDIR}/guest-libx264.before.hostcpu" "${WORKDIR}/guest-libx264.after.hostcpu" \
  "${WORKDIR}/guest-videotoolbox.before.hostcpu" "${WORKDIR}/guest-videotoolbox.after.hostcpu" \
  "${WORKDIR}/guest-libx264-contended.before.hostcpu" "${WORKDIR}/guest-libx264-contended.after.hostcpu" \
  "${WORKDIR}/guest-videotoolbox-contended.before.hostcpu" "${WORKDIR}/guest-videotoolbox-contended.after.hostcpu"
do
  require_file "${path}"
done

python3 - <<PY
import json
import pathlib
import re

workdir = pathlib.Path("${WORKDIR}")
frame_count = int("${frame_limit_value}")
duration_s = float("${duration_seconds}")

def parse_time_file(path):
    text = pathlib.Path(path).read_text(encoding="utf-8", errors="replace")
    user_match = re.search(r"User time \\(seconds\\):\\s*([0-9.]+)", text)
    sys_match = re.search(r"System time \\(seconds\\):\\s*([0-9.]+)", text)
    cpu_match = re.search(r"Percent of CPU this job got:\\s*([0-9]+)%", text)
    elapsed_match = re.search(r"Elapsed \\(wall clock\\) time.*?:\\s*([^\\n]+)", text)
    return {
        "user_s": float(user_match.group(1)) if user_match else 0.0,
        "sys_s": float(sys_match.group(1)) if sys_match else 0.0,
        "cpu_percent": int(cpu_match.group(1)) if cpu_match else 0,
        "elapsed_text": elapsed_match.group(1).strip() if elapsed_match else "",
    }

def parse_ps_time(value):
    value = value.strip()
    if not value:
        return 0.0
    days = 0
    if "-" in value:
        day_part, value = value.split("-", 1)
        days = int(day_part)
    parts = value.split(":")
    if len(parts) == 3:
        hours, minutes, seconds = parts
    elif len(parts) == 2:
        hours = 0
        minutes, seconds = parts
    else:
        raise ValueError(f"unexpected ps time format: {value!r}")
    return days * 86400 + int(hours) * 3600 + int(minutes) * 60 + float(seconds)

def parse_host_snapshot(path):
    values = {}
    for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            values[key] = value
    return values

def host_delta(prefix):
    before = parse_host_snapshot(workdir / f"{prefix}.before.hostcpu")
    after = parse_host_snapshot(workdir / f"{prefix}.after.hostcpu")
    launcher_user = parse_ps_time(after["launcher_utime"]) - parse_ps_time(before["launcher_utime"])
    launcher_sys = parse_ps_time(after["launcher_stime"]) - parse_ps_time(before["launcher_stime"])
    worker_user = parse_ps_time(after["worker_utime"]) - parse_ps_time(before["worker_utime"])
    worker_sys = parse_ps_time(after["worker_stime"]) - parse_ps_time(before["worker_stime"])
    return {
        "launcher_user_s": max(0.0, launcher_user),
        "launcher_sys_s": max(0.0, launcher_sys),
        "worker_user_s": max(0.0, worker_user),
        "worker_sys_s": max(0.0, worker_sys),
    }

def extract_ffmpeg_rtime(path):
    text = pathlib.Path(path).read_text(encoding="utf-8", errors="replace")
    matches = re.findall(r"bench:\\s+.*?rtime=([0-9.]+)s", text)
    if not matches:
        raise RuntimeError(f"missing ffmpeg bench rtime in {path}")
    return float(matches[-1])

def extract_openssl_throughput(path):
    text = pathlib.Path(path).read_text(encoding="utf-8", errors="replace")
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped.startswith("sha256"):
            continue
        parts = stripped.split()
        if len(parts) < 2:
            continue
        value = parts[-1]
        if value.endswith("k"):
            return float(value[:-1]) * 1000.0
    raise RuntimeError(f"missing openssl sha256 throughput in {path}")

guest_sw_time = parse_time_file("${GUEST_SW_TIME}")
guest_vt_time = parse_time_file("${GUEST_VT_TIME}")
guest_ssl_base_time = parse_time_file("${GUEST_SSL_BASE_TIME}")
guest_vt_ssl_base_time = parse_time_file("${GUEST_VT_SSL_BASE_TIME}")
guest_sw_cont_time = parse_time_file("${GUEST_SW_CONT_TIME}")
guest_vt_cont_time = parse_time_file("${GUEST_VT_CONT_TIME}")
guest_sw_ssl_cont_time = parse_time_file("${GUEST_SW_SSL_CONT_TIME}")
guest_vt_ssl_cont_time = parse_time_file("${GUEST_VT_SSL_CONT_TIME}")

guest_sw_host = host_delta("guest-libx264")
guest_vt_host = host_delta("guest-videotoolbox")
guest_sw_cont_host = host_delta("guest-libx264-contended")
guest_vt_cont_host = host_delta("guest-videotoolbox-contended")

guest_sw_rtime = extract_ffmpeg_rtime("${GUEST_SW_LOG}")
guest_vt_rtime = extract_ffmpeg_rtime("${GUEST_VT_LOG}")
guest_sw_cont_rtime = extract_ffmpeg_rtime("${GUEST_SW_CONT_LOG}")
guest_vt_cont_rtime = extract_ffmpeg_rtime("${GUEST_VT_CONT_LOG}")

openssl_alone_sw_vm = extract_openssl_throughput("${GUEST_SSL_BASE_LOG}")
openssl_alone_vt_vm = extract_openssl_throughput("${GUEST_VT_SSL_BASE_LOG}")
openssl_with_sw = extract_openssl_throughput("${GUEST_SW_SSL_CONT_LOG}")
openssl_with_vt = extract_openssl_throughput("${GUEST_VT_SSL_CONT_LOG}")

def cpu_total(metrics):
    return metrics["user_s"] + metrics["sys_s"]

def host_total(delta):
    return delta["launcher_user_s"] + delta["launcher_sys_s"] + delta["worker_user_s"] + delta["worker_sys_s"]

summary = {
    "reference": {
        "path": "${REFERENCE_VIDEO}",
        "duration_s": duration_s,
        "frame_limit": frame_count,
        "width": int("${width}"),
        "height": int("${height}"),
        "contention_seconds": int("${CONTENTION_SECONDS}"),
        "openssl_block_bytes": int("${OPENSSL_BLOCK_BYTES}"),
        "vt_ferry_transport": "vsock",
    },
    "guest_isolated": {
        "libx264": {
            "ffmpeg_rtime_s": guest_sw_rtime,
            "guest_user_s": guest_sw_time["user_s"],
            "guest_sys_s": guest_sw_time["sys_s"],
            "guest_total_cpu_s": cpu_total(guest_sw_time),
            "host_total_cpu_s": host_total(guest_sw_host),
        },
        "videotoolbox": {
            "ffmpeg_rtime_s": guest_vt_rtime,
            "guest_user_s": guest_vt_time["user_s"],
            "guest_sys_s": guest_vt_time["sys_s"],
            "guest_total_cpu_s": cpu_total(guest_vt_time),
            "host_total_cpu_s": host_total(guest_vt_host),
        },
    },
    "guest_contention": {
        "libx264": {
            "openssl_alone_bytes_per_s": openssl_alone_sw_vm,
            "ffmpeg_rtime_s": guest_sw_cont_rtime,
            "guest_total_cpu_s": cpu_total(guest_sw_cont_time),
            "host_total_cpu_s": host_total(guest_sw_cont_host),
            "openssl_bytes_per_s": openssl_with_sw,
            "openssl_retention_vs_alone": openssl_with_sw / openssl_alone_sw_vm,
        },
        "videotoolbox": {
            "openssl_alone_bytes_per_s": openssl_alone_vt_vm,
            "ffmpeg_rtime_s": guest_vt_cont_rtime,
            "guest_total_cpu_s": cpu_total(guest_vt_cont_time),
            "host_total_cpu_s": host_total(guest_vt_cont_host),
            "openssl_bytes_per_s": openssl_with_vt,
            "openssl_retention_vs_alone": openssl_with_vt / openssl_alone_vt_vm,
        },
    },
    "derived": {
        "isolated_guest_cpu_savings_vt_vs_libx264": cpu_total(guest_sw_time) - cpu_total(guest_vt_time),
        "isolated_host_cpu_delta_vt_vs_libx264": host_total(guest_vt_host) - host_total(guest_sw_host),
        "contended_guest_cpu_savings_vt_vs_libx264": cpu_total(guest_sw_cont_time) - cpu_total(guest_vt_cont_time),
        "contended_host_cpu_delta_vt_vs_libx264": host_total(guest_vt_cont_host) - host_total(guest_sw_cont_host),
        "openssl_headroom_gain_ratio_vt_vs_libx264": (openssl_with_vt / openssl_alone_vt_vm) - (openssl_with_sw / openssl_alone_sw_vm),
    },
}

if frame_count > 0:
    summary["guest_isolated"]["libx264"]["guest_cpu_s_per_frame"] = cpu_total(guest_sw_time) / frame_count
    summary["guest_isolated"]["videotoolbox"]["guest_cpu_s_per_frame"] = cpu_total(guest_vt_time) / frame_count
    summary["guest_isolated"]["libx264"]["host_cpu_s_per_frame"] = host_total(guest_sw_host) / frame_count
    summary["guest_isolated"]["videotoolbox"]["host_cpu_s_per_frame"] = host_total(guest_vt_host) / frame_count

pathlib.Path("${SUMMARY_JSON}").write_text(json.dumps(summary, indent=2) + "\\n", encoding="utf-8")

print("Reference:")
print(f"  path: ${REFERENCE_VIDEO}")
print(f"  frame_limit: {frame_count}")
print(f"  size: ${width}x${height}")
print(f"  contention_seconds: ${CONTENTION_SECONDS}")
print(f"  openssl_block_bytes: ${OPENSSL_BLOCK_BYTES}")
print(f"  vt_ferry_transport: vsock")
print("")
print("Guest Isolated:")
print(f"  libx264_ffmpeg_rtime_s: {guest_sw_rtime:.3f}")
print(f"  libx264_guest_cpu_s: {cpu_total(guest_sw_time):.3f}")
print(f"  libx264_host_cpu_s: {host_total(guest_sw_host):.3f}")
print(f"  videotoolbox_ffmpeg_rtime_s: {guest_vt_rtime:.3f}")
print(f"  videotoolbox_guest_cpu_s: {cpu_total(guest_vt_time):.3f}")
print(f"  videotoolbox_host_cpu_s: {host_total(guest_vt_host):.3f}")
print(f"  guest_cpu_savings_vt_vs_libx264: {cpu_total(guest_sw_time) - cpu_total(guest_vt_time):.3f}")
print("")
print("Guest Contention:")
print(f"  openssl_alone_libx264_vm_bytes_per_s: {openssl_alone_sw_vm:.0f}")
print(f"  openssl_alone_videotoolbox_vm_bytes_per_s: {openssl_alone_vt_vm:.0f}")
print(f"  openssl_with_libx264_bytes_per_s: {openssl_with_sw:.0f}")
print(f"  openssl_with_videotoolbox_bytes_per_s: {openssl_with_vt:.0f}")
print(f"  openssl_retention_with_libx264: {openssl_with_sw / openssl_alone_sw_vm:.3f}x")
print(f"  openssl_retention_with_videotoolbox: {openssl_with_vt / openssl_alone_vt_vm:.3f}x")
print(f"  openssl_headroom_gain_ratio_vt_vs_libx264: {(openssl_with_vt / openssl_alone_vt_vm) - (openssl_with_sw / openssl_alone_sw_vm):.3f}")
print(f"  libx264_contended_ffmpeg_rtime_s: {guest_sw_cont_rtime:.3f}")
print(f"  videotoolbox_contended_ffmpeg_rtime_s: {guest_vt_cont_rtime:.3f}")
print("")
print(f"summary_json: ${SUMMARY_JSON}")
print(f"workdir: ${WORKDIR}")
PY
