#!/bin/sh
#
# Multi-process concurrent transcode smoke.
#
# Two ffmpeg PROCESSES inside the same guest VM, each doing a
# decode + encode pipeline through the host worker, running in
# parallel. Validates that the worker's spawn-per-connection
# refactor (vt-ferry-worker/src/server.rs) actually delivers the
# concurrency it promises:
#
#   - two simultaneous Unix-socket connections to the worker
#   - two independent backend instances with isolated session id
#     namespaces, buffer pools, output queues
#   - launcher pre-registers TWO IOSurface pools of the same
#     shape so each guest process can claim one for its
#     encode-input pool (the IOSurfacePoolDirectory's
#     take_matching consumes the entry, so two concurrent
#     guests need two pre-registered pools)
#
# Decode side is intentionally libavcodec native (no
# `-hwaccel videotoolbox` on the input). With Phase 15 enabling
# real VT decode, a single ffmpeg now claims TWO pool entries
# (decoder + encoder); two concurrent ffmpegs would need four,
# but XNU's `TASK_PORT_REGISTER_MAX = 3` caps registered
# IOSurface ports. Software decode is fast enough to feed the
# encoder at 480p, so we pay almost no perf to keep the test
# focused on multi-PROCESS protocol concurrency through the
# worker.
#
# Pre-Phase-14 (single-connection worker), this would have
# DEADLOCKED: the second ffmpeg's `connect()` would have hung
# waiting for the first to disconnect. Now both proceed in
# parallel.
#
# Capped at 480p so each transcode takes <10 seconds and total
# wallclock stays manageable. The interesting test is concurrency
# correctness, not throughput.
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_transcode_concurrent.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"
SMOLVM_ROOT="${REPO_ROOT}/third_party/smolvm"

SMOLVM_BIN="${SMOLVM_BIN:-${SMOLVM_ROOT}/target/release/smolvm}"
SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS:-${SMOLVM_ROOT}/target/agent-rootfs}"
HOST_WORKER_BIN="${HOST_WORKER_BIN:-${REPO_ROOT}/target/debug/vt-ferry-worker}"
BROKER_BIN="${BROKER_BIN:-${REPO_ROOT}/target/release/vt-ferry-broker}"
GUEST_FFMPEG_BIN="${GUEST_FFMPEG_BIN:-${REPO_ROOT}/artifacts/ffmpeg-build/n8.1-linux-debug/ffmpeg}"
HOST_FFMPEG_BIN="${HOST_FFMPEG_BIN:-ffmpeg}"
SHIM_LIBDIR="${SHIM_LIBDIR:-${REPO_ROOT}/artifacts/ffmpeg-shim-libs/linux-debug}"
SHIM_TARGET_TRIPLE="${SHIM_TARGET_TRIPLE:-aarch64-unknown-linux-gnu}"
STAGE_SHIM_LIBS="${STAGE_SHIM_LIBS:-1}"
VM_IMAGE="${VM_IMAGE:-public.ecr.aws/docker/library/ubuntu:24.04}"
VM_NAME="${VM_NAME:-vt-ferry-transcode-concurrent-$$}"
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/transcode-concurrent-proof-$$}"
FRAME_SIZE="${FRAME_SIZE:-640x480}"
FRAME_RATE="${FRAME_RATE:-24}"
DURATION="${DURATION:-2}"
VSOCK_PORT="${VSOCK_PORT:-6606}"
SLOT_COUNT="${SLOT_COUNT:-8}"

WIDTH="${FRAME_SIZE%x*}"
HEIGHT="${FRAME_SIZE#*x}"

LAUNCHER_PID=""

die() {
  echo "ERROR: $*" >&2
  exit 1
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

smolvm_cmd() {
  SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS}" \
  "${SMOLVM_BIN}" "$@"
}

guest_repo_path() {
  case "$1" in
    "${REPO_ROOT}") printf '/repo\n' ;;
    "${REPO_ROOT}"/*) printf '/repo/%s\n' "${1#${REPO_ROOT}/}" ;;
    *) return 1 ;;
  esac
}

[ -e "${SMOLVM_BIN}" ] || die "missing ${SMOLVM_BIN}"
[ -e "${HOST_WORKER_BIN}" ] || die "missing ${HOST_WORKER_BIN}"
[ -e "${BROKER_BIN}" ] || die "missing ${BROKER_BIN}"
command -v "${HOST_FFMPEG_BIN}" >/dev/null 2>&1 || die "missing host ffmpeg"

# Image mode (USE_VT_FERRY_IMAGE=1): patched ffmpeg + shim live
# in VM_IMAGE under /opt/vt-ferry/. Skip host-side staging.
USE_VT_FERRY_IMAGE="${USE_VT_FERRY_IMAGE:-0}"
if [ "${USE_VT_FERRY_IMAGE}" != "1" ]; then
  [ -e "${GUEST_FFMPEG_BIN}" ] || die "missing ${GUEST_FFMPEG_BIN}"
  if [ "${STAGE_SHIM_LIBS}" = "1" ]; then
    "${REPO_ROOT}/ffmpeg/scripts/stage_guest_shim_libs.sh" \
      debug \
      "${SHIM_LIBDIR#${REPO_ROOT}/}" \
      "${SHIM_TARGET_TRIPLE}"
  fi
  [ -e "${SHIM_LIBDIR}/libguest_shim.so" ] || die "missing ${SHIM_LIBDIR}/libguest_shim.so"
fi

mkdir -p "${WORKDIR}"
INTERMEDIATE_MP4="${WORKDIR}/source.mp4"
GUEST_OUTPUT_A="${WORKDIR}/out-a.mp4"
GUEST_OUTPUT_B="${WORKDIR}/out-b.mp4"
LAUNCHER_LOG="${WORKDIR}/launcher.log"
GUEST_LOG_A="${WORKDIR}/guest-ffmpeg-a.log"
GUEST_LOG_B="${WORKDIR}/guest-ffmpeg-b.log"

# Stage a host-encoded source for both guest transcodes to consume.
echo "host: encoding ${WIDTH}x${HEIGHT}@${FRAME_RATE}fps for ${DURATION}s with libx264"
"${HOST_FFMPEG_BIN}" -hide_banner -y \
  -f lavfi -i "testsrc2=size=${WIDTH}x${HEIGHT}:rate=${FRAME_RATE}:duration=${DURATION}" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
  "${INTERMEDIATE_MP4}" >/dev/null 2>&1
[ -s "${INTERMEDIATE_MP4}" ] || die "host encode produced empty file"

GUEST_INTERMEDIATE="$(guest_repo_path "${INTERMEDIATE_MP4}")" \
  || die "WORKDIR must be under ${REPO_ROOT}"
if [ "${USE_VT_FERRY_IMAGE}" = "1" ]; then
  GUEST_FFMPEG_GUEST_BIN="${GUEST_FFMPEG_GUEST_BIN:-/opt/vt-ferry/bin/ffmpeg}"
  GUEST_SHIM_LIBDIR="${GUEST_SHIM_LIBDIR:-}"
else
  GUEST_FFMPEG_GUEST_BIN="$(guest_repo_path "${GUEST_FFMPEG_BIN}")" \
    || die "GUEST_FFMPEG_BIN must be under ${REPO_ROOT}"
  GUEST_SHIM_LIBDIR="$(guest_repo_path "${SHIM_LIBDIR}")" \
    || die "SHIM_LIBDIR must be under ${REPO_ROOT}"
fi

# Two pre-registered IOSurface pools at the same shape — one
# per concurrent guest process. Each guest's encode-input pool
# claim consumes a directory entry; without two entries, the
# second guest's CREATE_BUFFER_POOL would reject.
make_pool_json() {
  python3 -c "
import json
print(json.dumps({
    'guest_phys_addr': $1,
    'slot_count': ${SLOT_COUNT},
    'width': ${WIDTH},
    'height': ${HEIGHT},
    'pixel_format': 0x34323076,
    'writable': True,
}))"
}
POOL_JSON_A="$(make_pool_json 0x800000000)"
POOL_JSON_B="$(make_pool_json 0x900000000)"

"${REPO_ROOT}/third_party/prepare_krun_runtime.sh" >/dev/null
. "${REPO_ROOT}/artifacts/krun-runtime/macos-arm64/env.sh"

smolvm_cmd machine create \
  --net \
  --image "${VM_IMAGE}" \
  "${VM_NAME}" \
  -v "${REPO_ROOT}:/repo" \
  >/dev/null

SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS}" \
"${BROKER_BIN}" \
  --vsock-port "${VSOCK_PORT}" \
  --pool "${POOL_JSON_A}" \
  --pool "${POOL_JSON_B}" \
  --host-worker "${HOST_WORKER_BIN}" \
  -- "${SMOLVM_BIN}" machine start --name "${VM_NAME}" \
  >"${LAUNCHER_LOG}" 2>&1 &
LAUNCHER_PID=$!

for _ in $(seq 1 120); do
  if grep -q "running (PID" "${LAUNCHER_LOG}" 2>/dev/null; then
    break
  fi
  if ! kill -0 "${LAUNCHER_PID}" 2>/dev/null; then
    die "broker/smolvm exited before ready; log: $(cat "${LAUNCHER_LOG}")"
  fi
  sleep 1
done
sleep 3

smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
  'apt-get update >/tmp/apt-update.log 2>&1 && DEBIAN_FRONTEND=noninteractive apt-get install -y libxcb1 libxcb-shm0 libxau6 libxdmcp6 >/tmp/apt-install.log 2>&1'

# Inside the VM, launch TWO transcodes in parallel. Each one
# opens its own connection to the worker. Pre-Phase-14 they'd
# serialize at the worker's accept loop; post-Phase-14 they
# proceed in parallel.
#
# We use `&` + `wait` to enforce parallelism at the shell layer.
# Both processes share the same vsock port (the worker's accept
# loop fans out to per-connection threads), so VT_FERRY_VSOCK_PORT
# is identical on both sides.
echo "guest: launching 2 concurrent transcodes"
GUEST_INTERMEDIATE_PATH="${GUEST_INTERMEDIATE}"
GUEST_OUT_A_PATH="$(guest_repo_path "${GUEST_OUTPUT_A}")"
GUEST_OUT_B_PATH="$(guest_repo_path "${GUEST_OUTPUT_B}")"
GUEST_LOG_A_PATH="$(guest_repo_path "${GUEST_LOG_A}")"
GUEST_LOG_B_PATH="$(guest_repo_path "${GUEST_LOG_B}")"

# Stage the run script into the VM's /repo mount as a file —
# avoids escaping a multi-line script through `sh -lc "..."` which
# has caused subtle quoting problems (the inner shell re-parsed
# `\$!` and friends in surprising ways). The script lives in the
# host workdir which is bind-mounted as /repo in the guest.
RUN_SCRIPT_HOST="${WORKDIR}/run-concurrent.sh"
# DEBUG MODE: set CONCURRENT_RUN_MODE=sequential to run the
# transcodes one after another instead of in parallel. Used to
# isolate whether failures are concurrency-specific or just
# generic transcode regressions.
CONCURRENT_RUN_MODE="${CONCURRENT_RUN_MODE:-parallel}"
if [ "${CONCURRENT_RUN_MODE}" = "sequential" ]; then
cat > "${RUN_SCRIPT_HOST}" <<EOF
#!/bin/sh
# Sequential transcodes (debug mode). Validates the script
# pattern itself works without testing concurrency.
export VT_FERRY_TRANSPORT=vsock
export VT_FERRY_VSOCK_PORT=${VSOCK_PORT}
${GUEST_SHIM_LIBDIR:+export LD_LIBRARY_PATH=${GUEST_SHIM_LIBDIR}}

${GUEST_FFMPEG_GUEST_BIN} -hide_banner \\
  -i ${GUEST_INTERMEDIATE_PATH} \\
  -map 0:v:0 -an -frames:v ${FRAME_RATE} \\
  -vf format=nv12 -c:v h264_videotoolbox \\
  -profile:v main -b:v 4000000 -bf 0 \\
  -y ${GUEST_OUT_A_PATH} \\
  > ${GUEST_LOG_A_PATH} 2>&1
STATUS_A=\$?

${GUEST_FFMPEG_GUEST_BIN} -hide_banner \\
  -i ${GUEST_INTERMEDIATE_PATH} \\
  -map 0:v:0 -an -frames:v ${FRAME_RATE} \\
  -vf format=nv12 -c:v h264_videotoolbox \\
  -profile:v main -b:v 2000000 -bf 0 \\
  -y ${GUEST_OUT_B_PATH} \\
  > ${GUEST_LOG_B_PATH} 2>&1
STATUS_B=\$?

echo "transcode_a_status=\$STATUS_A"
echo "transcode_b_status=\$STATUS_B"
[ \$STATUS_A -eq 0 ] && [ \$STATUS_B -eq 0 ]
EOF
else
cat > "${RUN_SCRIPT_HOST}" <<EOF
#!/bin/sh
# Two concurrent transcodes. Background each ffmpeg with &,
# capture PIDs, wait for both, report each exit code.
#
# Logs go to /tmp inside the VM (tmpfs) — virtiofs writes
# from a doomed ffmpeg don't always flush before exit, so
# logs redirected straight at /repo come back empty. We
# write to /tmp and the host script pulls them out via
# `machine exec cat /tmp/...` on failure.
export VT_FERRY_TRANSPORT=vsock
export VT_FERRY_VSOCK_PORT=${VSOCK_PORT}
${GUEST_SHIM_LIBDIR:+export LD_LIBRARY_PATH=${GUEST_SHIM_LIBDIR}}

${GUEST_FFMPEG_GUEST_BIN} -hide_banner \\
  -i ${GUEST_INTERMEDIATE_PATH} \\
  -map 0:v:0 -an -frames:v ${FRAME_RATE} \\
  -vf format=nv12 -c:v h264_videotoolbox \\
  -profile:v main -b:v 4000000 -bf 0 \\
  -y ${GUEST_OUT_A_PATH} \\
  > /tmp/guest-ffmpeg-a.log 2>&1 &
PID_A=\$!

${GUEST_FFMPEG_GUEST_BIN} -hide_banner \\
  -i ${GUEST_INTERMEDIATE_PATH} \\
  -map 0:v:0 -an -frames:v ${FRAME_RATE} \\
  -vf format=nv12 -c:v h264_videotoolbox \\
  -profile:v main -b:v 2000000 -bf 0 \\
  -y ${GUEST_OUT_B_PATH} \\
  > /tmp/guest-ffmpeg-b.log 2>&1 &
PID_B=\$!

wait \$PID_A
STATUS_A=\$?
wait \$PID_B
STATUS_B=\$?
echo "transcode_a_status=\$STATUS_A"
echo "transcode_b_status=\$STATUS_B"
[ \$STATUS_A -eq 0 ] && [ \$STATUS_B -eq 0 ]
EOF
fi
chmod +x "${RUN_SCRIPT_HOST}"
GUEST_RUN_SCRIPT="$(guest_repo_path "${RUN_SCRIPT_HOST}")"

GUEST_SCRIPT="sh ${GUEST_RUN_SCRIPT}"

GUEST_OUTPUT="$(smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc "${GUEST_SCRIPT}" 2>&1)" \
  || GUEST_STATUS=$?
GUEST_STATUS="${GUEST_STATUS:-0}"

echo "${GUEST_OUTPUT}"

if [ "${GUEST_STATUS}" -ne 0 ]; then
  echo ""
  echo "ERROR: at least one concurrent transcode failed"
  # Cat the /tmp logs from inside the VM directly — the run-script's
  # cp+sync to virtiofs doesn't reliably flush, so we pull them via
  # exec instead.
  for name in guest-ffmpeg-a.log guest-ffmpeg-b.log; do
    echo "--- ${name} (last 60 lines) ---"
    smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc "tail -60 /tmp/${name}" 2>/dev/null || true
  done
  exit "${GUEST_STATUS}"
fi

# Verify both output files exist + are non-empty + look like H.264.
[ -s "${GUEST_OUTPUT_A}" ] || die "transcode A produced empty output"
[ -s "${GUEST_OUTPUT_B}" ] || die "transcode B produced empty output"

CODEC_A="$("${HOST_FFMPEG_BIN}" -hide_banner -i "${GUEST_OUTPUT_A}" 2>&1 | grep -oE 'Video: h264' | head -1)"
CODEC_B="$("${HOST_FFMPEG_BIN}" -hide_banner -i "${GUEST_OUTPUT_B}" 2>&1 | grep -oE 'Video: h264' | head -1)"
[ -n "${CODEC_A}" ] || die "transcode A output isn't H.264"
[ -n "${CODEC_B}" ] || die "transcode B output isn't H.264"

SIZE_A="$(wc -c < "${GUEST_OUTPUT_A}")"
SIZE_B="$(wc -c < "${GUEST_OUTPUT_B}")"

echo ""
echo "smolvm videotoolbox concurrent transcode proof passed"
echo "  transcode_a: $(basename "${GUEST_OUTPUT_A}") size=${SIZE_A} bytes (4 Mb/s)"
echo "  transcode_b: $(basename "${GUEST_OUTPUT_B}") size=${SIZE_B} bytes (2 Mb/s)"
echo "  workdir=${WORKDIR}"
