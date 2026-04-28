#!/bin/sh
#
# Decode-side fidelity comparison: drive a reference encoded clip
# through host VideoToolbox decode and guest VideoToolbox decode
# (via the smolvm + libkrun vsock shim), then compare the two raw
# NV12 outputs frame-by-frame via ffmpeg's `psnr` / `ssim` filters.
#
# Catches regressions in:
#   - the chunked OP_READ_BUFFER path (pool-bound delivery for >720p)
#   - per-plane stride math on either side of the worker copy
#   - parameter-set extraction (CMVideoFormatDescription{H264,HEVC}
#     ParameterSet packing — a missed byte here would corrupt every
#     frame's first slice header)
#
# Output is raw rawvideo (NV12) on both sides — no re-encoding —
# so PSNR/SSIM measure the actual pixel deviation rather than
# encoder noise. Bit-identical decode on both paths produces
# PSNR=inf / SSIM=1.0; the gate (long_decode_fidelity.sh) sets
# defensive thresholds slightly below that to catch real
# regressions while tolerating measurement-floor jitter.

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
VM_NAME="${VM_NAME:-vt-ferry-decode-compare-$$}"
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/ffmpeg-decode-compare-$$}"
SLOT_COUNT="${SLOT_COUNT:-4}"
PIXEL_FORMAT="${PIXEL_FORMAT:-0x34323076}"
FRAME_COUNT="${FRAME_COUNT:-300}"
VSOCK_PORT="${VSOCK_PORT:-6603}"
MIN_PSNR_AVERAGE="${MIN_PSNR_AVERAGE:-}"
MIN_PSNR_MIN="${MIN_PSNR_MIN:-}"
MIN_SSIM_ALL="${MIN_SSIM_ALL:-}"

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

HOST_RAW="${WORKDIR}/host-vt-decode.nv12"
GUEST_RAW="${WORKDIR}/guest-vt-decode.nv12"
HOST_LOG="${WORKDIR}/host-vt-decode.log"
GUEST_LOG="${WORKDIR}/guest-vt-decode.log"
LAUNCHER_LOG="${WORKDIR}/launcher.log"
PSNR_STATS="${WORKDIR}/psnr.stats"
SSIM_STATS="${WORKDIR}/ssim.stats"
PSNR_STDERR="${WORKDIR}/psnr.stderr"
SSIM_STDERR="${WORKDIR}/ssim.stderr"
GUEST_REFERENCE_VIDEO="$(guest_repo_path "${REFERENCE_VIDEO}")" || die "REFERENCE_VIDEO must be under ${REPO_ROOT}"
if [ "${USE_VT_FERRY_IMAGE}" = "1" ]; then
  GUEST_FFMPEG_GUEST_BIN="${GUEST_FFMPEG_GUEST_BIN:-/opt/vt-ferry/bin/ffmpeg}"
  GUEST_SHIM_LIBDIR="${GUEST_SHIM_LIBDIR:-}"
else
  GUEST_FFMPEG_GUEST_BIN="$(guest_repo_path "${GUEST_FFMPEG_BIN}")" || die "GUEST_FFMPEG_BIN must be under ${REPO_ROOT}"
  GUEST_SHIM_LIBDIR="$(guest_repo_path "${SHIM_LIBDIR}")" || die "SHIM_LIBDIR must be under ${REPO_ROOT}"
fi
GUEST_WORKDIR="$(guest_repo_path "${WORKDIR}")" || die "WORKDIR must be under ${REPO_ROOT}"
GUEST_RAW_NAME="$(basename "${GUEST_RAW}")"
GUEST_LOG_NAME="$(basename "${GUEST_LOG}")"

probe_json="$("${FFPROBE_BIN}" -hide_banner -select_streams v:0 -show_streams -print_format json "${REFERENCE_VIDEO}")"
read width height <<EOF
$(python3 - <<PY
import json
probe = json.loads("""${probe_json}""")
stream = probe["streams"][0]
print(stream["width"], stream["height"])
PY
)
EOF

[ -n "${width}" ] || die "failed to parse reference width"
[ -n "${height}" ] || die "failed to parse reference height"

echo "reference=${REFERENCE_VIDEO}"
echo "size=${width}x${height}"
echo "frame_count=${FRAME_COUNT}"

# Step 1: host-side VideoToolbox decode → raw NV12.
echo "host: decoding via -hwaccel videotoolbox"
"${HOST_FFMPEG_BIN}" -hide_banner -y \
  -hwaccel videotoolbox \
  -i "${REFERENCE_VIDEO}" \
  -map 0:v:0 -an \
  -frames:v "${FRAME_COUNT}" \
  -f rawvideo -pix_fmt nv12 \
  "${HOST_RAW}" >"${HOST_LOG}" 2>&1

require_file "${HOST_RAW}"

# Step 2: VM bringup for the guest decode.
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

# Step 3: guest-side VideoToolbox decode → raw NV12 over vsock.
echo "guest: decoding via -hwaccel videotoolbox (vsock shim)"
DECODE_STATUS=0
smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
  "env ${GUEST_VT_TRANSPORT_ENV} \
       ${GUEST_SHIM_LIBDIR:+LD_LIBRARY_PATH=${GUEST_SHIM_LIBDIR}} \
       ${GUEST_FFMPEG_GUEST_BIN} -hide_banner \
       -hwaccel videotoolbox -i ${GUEST_REFERENCE_VIDEO} \
       -map 0:v:0 -an \
       -frames:v ${FRAME_COUNT} \
       -f rawvideo -pix_fmt nv12 -y \
       ${GUEST_WORKDIR}/${GUEST_RAW_NAME} \
       > ${GUEST_WORKDIR}/${GUEST_LOG_NAME} 2>&1" \
  || DECODE_STATUS=$?

if [ "${DECODE_STATUS}" -ne 0 ]; then
  echo "ERROR: guest ffmpeg decode exited ${DECODE_STATUS}; log at ${GUEST_LOG}" >&2
  if [ -f "${GUEST_LOG}" ]; then
    echo "--- guest ffmpeg log (last 80 lines) ---" >&2
    tail -80 "${GUEST_LOG}" >&2
    echo "--- end guest ffmpeg log ---" >&2
  fi
  exit "${DECODE_STATUS}"
fi

require_file "${GUEST_RAW}"

# Step 4: PSNR / SSIM on the two raw NV12 streams. We feed both
# files as `-f rawvideo` inputs with explicit dimensions so ffmpeg
# doesn't try to demux them as containers — psnr/ssim filters
# operate on raw frames.
echo "comparing host-decoded vs guest-decoded NV12"
"${HOST_FFMPEG_BIN}" -hide_banner \
  -f rawvideo -pixel_format nv12 -video_size "${width}x${height}" -i "${HOST_RAW}" \
  -f rawvideo -pixel_format nv12 -video_size "${width}x${height}" -i "${GUEST_RAW}" \
  -lavfi "[0:v][1:v]psnr=stats_file=${PSNR_STATS}" \
  -f null - 2>"${PSNR_STDERR}" || die "psnr ffmpeg pass failed; see ${PSNR_STDERR}"
"${HOST_FFMPEG_BIN}" -hide_banner \
  -f rawvideo -pixel_format nv12 -video_size "${width}x${height}" -i "${HOST_RAW}" \
  -f rawvideo -pixel_format nv12 -video_size "${width}x${height}" -i "${GUEST_RAW}" \
  -lavfi "[0:v][1:v]ssim=stats_file=${SSIM_STATS}" \
  -f null - 2>"${SSIM_STDERR}" || die "ssim ffmpeg pass failed; see ${SSIM_STDERR}"

PSNR_SUMMARY="$(grep -E "Parsed_psnr.*PSNR" "${PSNR_STDERR}" | tail -1)"
SSIM_SUMMARY="$(grep -E "Parsed_ssim.*SSIM" "${SSIM_STDERR}" | tail -1)"

# Step 5: parse the summaries + check thresholds. Bit-identical
# decode produces PSNR=inf / SSIM=1.0; thresholds (when set) must
# allow for that.
python3 - <<PY
import re

psnr_summary = """${PSNR_SUMMARY}""".strip()
ssim_summary = """${SSIM_SUMMARY}""".strip()
min_psnr_average = """${MIN_PSNR_AVERAGE}""".strip() or None
min_psnr_min = """${MIN_PSNR_MIN}""".strip() or None
min_ssim_all = """${MIN_SSIM_ALL}""".strip() or None

def parse_metric(text, key):
    m = re.search(rf"\b{key}:(inf|[\d.]+)", text)
    if not m:
        return None
    raw = m.group(1)
    return float("inf") if raw == "inf" else float(raw)

psnr_average = parse_metric(psnr_summary, "average")
psnr_min = parse_metric(psnr_summary, "min")
ssim_all = parse_metric(ssim_summary, "All")

print(f"psnr_summary: {psnr_summary or 'unavailable'}")
if psnr_average is not None:
    print(f"psnr_average_db: {psnr_average}")
if psnr_min is not None:
    print(f"psnr_min_db: {psnr_min}")
print(f"ssim_summary: {ssim_summary or 'unavailable'}")
if ssim_all is not None:
    print(f"ssim_all: {ssim_all}")

failures = []
def check(name, value, threshold_str):
    if threshold_str is None or value is None:
        return
    threshold = float(threshold_str)
    if value < threshold:
        failures.append(f"{name}={value} below threshold {threshold}")

check("psnr_average_db", psnr_average, min_psnr_average)
check("psnr_min_db", psnr_min, min_psnr_min)
check("ssim_all", ssim_all, min_ssim_all)
if failures:
    raise SystemExit("decode fidelity gate failed: " + "; ".join(failures))
PY

echo ""
echo "workdir=${WORKDIR}"
