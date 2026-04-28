#!/bin/sh
#
# End-to-end decode validation for the smolvm VideoToolbox shim.
#
# The decode path landed in Phase 10. This smoke exercises the
# full chain:
#
#   1. Host ffmpeg encodes a small (640x480) testsrc2 lavfi source
#      to a known-good bitstream via the configured HOST_CODEC
#      (libx264 by default; libx265 for the HEVC variant) —
#      purely host-side, no shim involvement.
#   2. The encoded mp4 gets staged into the guest's repo mount.
#   3. Guest ffmpeg decodes via `-hwaccel videotoolbox`, which
#      picks our shim's VTDecompressionSession entrypoints. Output
#      is raw NV12.
#   4. Verify the raw output matches expected dimensions ×
#      frame-count × bytes-per-pixel.
#
# Capped at 480p so the decoded frames fit the 1.5 MiB inline
# pixel-data response cap (see VTF_TRANSPORT_MAX_DECODED_FRAME_BYTES).
# Frames larger than 720p NV12 trip STATUS_RESOURCE_EXHAUSTED on
# OP_READ_DECODED_FRAME — those need the future pool-binding path.
#
# Codec selection is parameterized via HOST_CODEC / CODEC_LABEL so
# the HEVC variant (prove_smolvm_videotoolbox_decode_hevc.sh) can
# reuse this driver. The guest side is codec-agnostic: -hwaccel
# videotoolbox auto-picks the {h264,hevc}_videotoolbox_hwaccel
# based on the input bitstream.
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_decode.sh

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
VM_NAME="${VM_NAME:-vt-ferry-decode-$$}"
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/decode-proof-$$}"
FRAME_SIZE="${FRAME_SIZE:-640x480}"
FRAME_RATE="${FRAME_RATE:-24}"
DURATION="${DURATION:-1}"
VSOCK_PORT="${VSOCK_PORT:-6601}"
SLOT_COUNT="${SLOT_COUNT:-4}"
# Host-side encoder selection. Decode side is codec-agnostic
# (`-hwaccel videotoolbox` auto-selects from input bitstream).
# Override these to drive the HEVC variant.
HOST_CODEC="${HOST_CODEC:-libx264}"
HOST_PRESET="${HOST_PRESET:-ultrafast}"
HOST_PIX_FMT="${HOST_PIX_FMT:-yuv420p}"
CODEC_LABEL="${CODEC_LABEL:-h264}"
# Optional explicit bitrate. Default rate-control on testsrc2 produces
# tiny IDRs (sub-32 KiB even at 4K), which would silently bypass the
# `VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES` cap path. The 4K wrapper
# sets this to 60M so the smoke exercises >256 KiB IDRs and would
# fail loudly on a future cap regression.
HOST_BITRATE="${HOST_BITRATE:-}"

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

# Image mode (USE_VT_FERRY_IMAGE=1) — the patched ffmpeg + guest
# shim are baked into VM_IMAGE under /opt/vt-ferry/. Skip the
# host-side build/staging checks. See ffmpeg/scripts/lib/image_mode.sh
# for the pattern.
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
INTERMEDIATE_MP4="${WORKDIR}/intermediate.mp4"
DECODED_RAW="${WORKDIR}/decoded.yuv"
FFMPEG_LOG="${WORKDIR}/ffmpeg-decode.log"

# Step 1: encode a known-good bitstream host-side via HOST_CODEC.
# HOST_BITRATE (-b:v) is opt-in; the 4K wrapper sets it to 60M so the
# resulting IDRs exceed 256 KiB and the smoke exercises the
# `VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES` regression sentinel.
if [ -n "${HOST_BITRATE}" ]; then
  echo "host: encoding ${WIDTH}x${HEIGHT}@${FRAME_RATE}fps for ${DURATION}s with ${HOST_CODEC} at ${HOST_BITRATE}"
  "${HOST_FFMPEG_BIN}" -hide_banner -y \
    -f lavfi -i "testsrc2=size=${WIDTH}x${HEIGHT}:rate=${FRAME_RATE}:duration=${DURATION}" \
    -c:v "${HOST_CODEC}" -preset "${HOST_PRESET}" -pix_fmt "${HOST_PIX_FMT}" \
    -b:v "${HOST_BITRATE}" \
    "${INTERMEDIATE_MP4}" >/dev/null 2>&1
else
  echo "host: encoding ${WIDTH}x${HEIGHT}@${FRAME_RATE}fps for ${DURATION}s with ${HOST_CODEC}"
  "${HOST_FFMPEG_BIN}" -hide_banner -y \
    -f lavfi -i "testsrc2=size=${WIDTH}x${HEIGHT}:rate=${FRAME_RATE}:duration=${DURATION}" \
    -c:v "${HOST_CODEC}" -preset "${HOST_PRESET}" -pix_fmt "${HOST_PIX_FMT}" \
    "${INTERMEDIATE_MP4}" >/dev/null 2>&1
fi

[ -s "${INTERMEDIATE_MP4}" ] || die "host encode produced empty file"

GUEST_INTERMEDIATE="$(guest_repo_path "${INTERMEDIATE_MP4}")" \
  || die "WORKDIR must be under ${REPO_ROOT}"
GUEST_DECODED="/tmp/$(basename "${DECODED_RAW}")"
GUEST_FFMPEG_LOG="/tmp/$(basename "${FFMPEG_LOG}")"
if [ "${USE_VT_FERRY_IMAGE}" = "1" ]; then
  GUEST_FFMPEG_GUEST_BIN="${GUEST_FFMPEG_GUEST_BIN:-/opt/vt-ferry/bin/ffmpeg}"
  GUEST_SHIM_LIBDIR="${GUEST_SHIM_LIBDIR:-}"
else
  GUEST_FFMPEG_GUEST_BIN="$(guest_repo_path "${GUEST_FFMPEG_BIN}")" \
    || die "GUEST_FFMPEG_BIN must be under ${REPO_ROOT}"
  GUEST_SHIM_LIBDIR="$(guest_repo_path "${SHIM_LIBDIR}")" \
    || die "SHIM_LIBDIR must be under ${REPO_ROOT}"
fi

# Pool spec — guest shim still creates a 4-slot pool for the
# encode-input flow (FFmpeg always opens an encoder pool even
# when only decoding). Decode output uses inline pixel data via
# OP_READ_DECODED_FRAME.
POOL_JSON="$(python3 -c "
import json
print(json.dumps({
    'guest_phys_addr': 0x800000000,
    'slot_count': ${SLOT_COUNT},
    'width': ${WIDTH},
    'height': ${HEIGHT},
    'pixel_format': 0x34323076,
    'writable': True,
}))
")"

"${REPO_ROOT}/third_party/prepare_krun_runtime.sh" >/dev/null
. "${REPO_ROOT}/artifacts/krun-runtime/macos-arm64/env.sh"

smolvm_cmd machine create \
  --net \
  --image "${VM_IMAGE}" \
  "${VM_NAME}" \
  -v "${REPO_ROOT}:/repo" \
  >/dev/null

LAUNCHER_LOG="${WORKDIR}/launcher.log"
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
    die "broker/smolvm exited before ready; log: $(cat "${LAUNCHER_LOG}")"
  fi
  sleep 1
done
sleep 3

smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
  'apt-get update >/tmp/apt-update.log 2>&1 && DEBIAN_FRONTEND=noninteractive apt-get install -y libxcb1 libxcb-shm0 libxau6 libxdmcp6 >/tmp/apt-install.log 2>&1'

# Step 2: decode the staged bitstream via the shim's
# VTDecompressionSession path (auto-selected from input codec).
echo "guest: decoding ${CODEC_LABEL} bitstream via -hwaccel videotoolbox"
DECODE_STATUS=0
# Image mode: ldconfig already wired /opt/vt-ferry/lib so we skip
# the explicit LD_LIBRARY_PATH (an empty value would shadow the
# default search order).
if [ -n "${GUEST_SHIM_LIBDIR}" ]; then
  GUEST_LD_PREFIX="LD_LIBRARY_PATH=${GUEST_SHIM_LIBDIR}"
else
  GUEST_LD_PREFIX=""
fi
smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
  "env VT_FERRY_TRANSPORT=vsock VT_FERRY_VSOCK_PORT=${VSOCK_PORT} \
       ${GUEST_LD_PREFIX} \
       ${GUEST_FFMPEG_GUEST_BIN} -hide_banner \
       -hwaccel videotoolbox -i ${GUEST_INTERMEDIATE} \
       -f rawvideo -pix_fmt nv12 -y ${GUEST_DECODED} \
       > ${GUEST_FFMPEG_LOG} 2>&1" \
  || DECODE_STATUS=$?

# Always copy the log back so a failure surfaces decoder diagnostics.
smolvm_cmd machine cp \
  "${VM_NAME}:${GUEST_FFMPEG_LOG}" "${FFMPEG_LOG}" >/dev/null 2>&1 || true

if [ "${DECODE_STATUS}" -ne 0 ]; then
  echo "ERROR: guest ffmpeg decode exited ${DECODE_STATUS}; log at ${FFMPEG_LOG}" >&2
  if [ -f "${FFMPEG_LOG}" ]; then
    echo "--- guest ffmpeg log (last 80 lines) ---" >&2
    tail -80 "${FFMPEG_LOG}" >&2
    echo "--- end guest ffmpeg log ---" >&2
  fi
  exit "${DECODE_STATUS}"
fi

smolvm_cmd machine cp \
  "${VM_NAME}:${GUEST_DECODED}" "${DECODED_RAW}" >/dev/null
[ -s "${DECODED_RAW}" ] || die "decoded output is empty: ${DECODED_RAW}"

# Step 3: verify output size matches expected NV12 layout.
EXPECTED_FRAMES="$((FRAME_RATE * DURATION))"
EXPECTED_BYTES_PER_FRAME="$((WIDTH * HEIGHT * 3 / 2))"
EXPECTED_TOTAL="$((EXPECTED_FRAMES * EXPECTED_BYTES_PER_FRAME))"
ACTUAL_TOTAL="$(wc -c < "${DECODED_RAW}")"

if [ "${ACTUAL_TOTAL}" -ne "${EXPECTED_TOTAL}" ]; then
  die "decoded output size mismatch: got ${ACTUAL_TOTAL} bytes, expected ${EXPECTED_TOTAL} (${EXPECTED_FRAMES} frames × ${EXPECTED_BYTES_PER_FRAME} NV12 bytes each)"
fi

echo "smolvm videotoolbox decode proof passed"
echo "  codec=${CODEC_LABEL} host_encoder=${HOST_CODEC}"
echo "  width=${WIDTH} height=${HEIGHT} frames=${EXPECTED_FRAMES}"
echo "  decoded_bytes=${ACTUAL_TOTAL}"
echo "  workdir=${WORKDIR}"
