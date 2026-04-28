#!/bin/sh
#
# ABR (mid-stream format change) variant of the decode proof.
#
# Adaptive bitrate ladders splice multiple encodes — different
# resolutions, sometimes different parameter sets — into a single
# logical playback. Each splice point hands the decoder a fresh
# `CMVideoFormatDescription`. The worker's `OP_SET_DECODE_FORMAT`
# handler probes `VTDecompressionSessionCanAcceptFormatDescription`
# on every re-issue and either keeps the live VT session
# (parameter-set swap) or drops + recreates it for the new format.
# That branch landed with the original decode work but had no
# end-to-end smoke until now (item 3 of the post-Phase-16 follow-up
# list).
#
# Why not extend `prove_smolvm_videotoolbox_decode_1080p.sh` /
# `_4k.sh` with an ABR mode: those wrappers are thin env-var
# overrides for the single-resolution parent driver. The parent
# driver assumes one encoded mp4 staged into the guest. ABR needs
# two host encodes, a concat splice step, and a verification that
# sums two distinct NV12 layouts × per-segment frame counts. That
# is enough divergence that copying the host-encode/launcher/decode
# machinery is cleaner than threading a "two intermediates" mode
# through every check site in the parent. This wrapper is a smoke,
# not a gate — like its sister 1080p / 4K / hevc wrappers, it
# validates only that decoded bytes match expected NV12 layout ×
# frame count across both segments. The mid-stream branch's
# correctness is otherwise covered by the worker's unit tests.
#
# Pipeline:
#   1. Host ffmpeg encodes 854x480 testsrc2 → seg_480p.mp4
#      (libx264 baseline, 30fps).
#   2. Host ffmpeg encodes 1280x720 testsrc2 → seg_720p.mp4
#      (libx264 baseline, 30fps).
#   3. `ffmpeg -f concat -safe 0 -i list.txt -c copy spliced.mp4`
#      splices them at the container layer — the resulting bitstream
#      switches `CMVideoFormatDescription` mid-stream when the
#      decoder crosses the segment boundary.
#   4. Guest ffmpeg decodes via `-hwaccel videotoolbox`. The shim
#      ships the second segment's parameter sets through
#      `OP_SET_DECODE_FORMAT` on top of the live session id;
#      worker's CanAcceptFormatDescription branch decides
#      reuse-or-recreate transparently.
#   5. Verify total decoded bytes == 480p NV12 × N1 + 720p NV12 × N2.
#
# Both segments stay ≤720p so per-frame NV12 fits the 1.5 MiB
# inline `OP_READ_DECODED_FRAME` budget — inline path, same as the
# canonical 480p smoke. (The pool-bound large-frame path is covered
# by the 1080p / 4K wrappers; layering both axes on the same smoke
# would obscure which one regressed.)
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_decode_abr.sh

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
VM_NAME="${VM_NAME:-vt-ferry-decode-abr-$$}"
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/decode-abr-proof-$$}"
# Two ABR rungs. Both H.264 baseline, both 30fps; only the spatial
# resolution differs so the format-description swap is the
# distinguishing event.
SEG1_SIZE="${SEG1_SIZE:-854x480}"
SEG2_SIZE="${SEG2_SIZE:-1280x720}"
FRAME_RATE="${FRAME_RATE:-30}"
SEG1_DURATION="${SEG1_DURATION:-1}"
SEG2_DURATION="${SEG2_DURATION:-1}"
VSOCK_PORT="${VSOCK_PORT:-6601}"
SLOT_COUNT="${SLOT_COUNT:-4}"

SEG1_WIDTH="${SEG1_SIZE%x*}"
SEG1_HEIGHT="${SEG1_SIZE#*x}"
SEG2_WIDTH="${SEG2_SIZE%x*}"
SEG2_HEIGHT="${SEG2_SIZE#*x}"

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

# Image mode (USE_VT_FERRY_IMAGE=1): the patched ffmpeg + guest
# shim live inside VM_IMAGE under /opt/vt-ferry/. Skip host-side
# build/staging checks. See ffmpeg/scripts/lib/image_mode.sh.
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
SEG1_MP4="${WORKDIR}/seg_480p.mp4"
SEG2_MP4="${WORKDIR}/seg_720p.mp4"
CONCAT_LIST="${WORKDIR}/list.txt"
SPLICED_MP4="${WORKDIR}/spliced.mp4"
DECODED_SEG1_RAW="${WORKDIR}/decoded_seg1.yuv"
DECODED_SEG2_RAW="${WORKDIR}/decoded_seg2.yuv"
FFMPEG_LOG="${WORKDIR}/ffmpeg-decode.log"

# Step 1: encode each ABR rung host-side. libx264 baseline pins the
# profile across both segments so the only thing that changes at
# the splice is the SPS-declared dimensions.
echo "host: encoding seg1 ${SEG1_WIDTH}x${SEG1_HEIGHT}@${FRAME_RATE}fps for ${SEG1_DURATION}s"
"${HOST_FFMPEG_BIN}" -hide_banner -y \
  -f lavfi -i "testsrc2=size=${SEG1_WIDTH}x${SEG1_HEIGHT}:rate=${FRAME_RATE}:duration=${SEG1_DURATION}" \
  -c:v libx264 -profile:v baseline -preset ultrafast -pix_fmt yuv420p \
  "${SEG1_MP4}" >/dev/null 2>&1
[ -s "${SEG1_MP4}" ] || die "host encode seg1 produced empty file"

echo "host: encoding seg2 ${SEG2_WIDTH}x${SEG2_HEIGHT}@${FRAME_RATE}fps for ${SEG2_DURATION}s"
"${HOST_FFMPEG_BIN}" -hide_banner -y \
  -f lavfi -i "testsrc2=size=${SEG2_WIDTH}x${SEG2_HEIGHT}:rate=${FRAME_RATE}:duration=${SEG2_DURATION}" \
  -c:v libx264 -profile:v baseline -preset ultrafast -pix_fmt yuv420p \
  "${SEG2_MP4}" >/dev/null 2>&1
[ -s "${SEG2_MP4}" ] || die "host encode seg2 produced empty file"

# Step 2: splice with the concat demuxer + stream copy. The
# resulting mp4 carries two distinct CMVideoFormatDescriptions in a
# single video track — the splice boundary is exactly what triggers
# the worker's mid-stream SET_DECODE_FORMAT re-issue branch.
echo "host: splicing segments via concat demuxer"
{
  printf "file '%s'\n" "${SEG1_MP4}"
  printf "file '%s'\n" "${SEG2_MP4}"
} > "${CONCAT_LIST}"
"${HOST_FFMPEG_BIN}" -hide_banner -y \
  -f concat -safe 0 -i "${CONCAT_LIST}" \
  -c copy "${SPLICED_MP4}" >/dev/null 2>&1
[ -s "${SPLICED_MP4}" ] || die "concat splice produced empty file"

# Determine actual per-segment frame counts from the source mp4s.
# testsrc2 + concat is deterministic but reading nb_frames keeps
# the predicted byte count honest if the duration knobs ever drift.
seg_frame_count() {
  "${HOST_FFMPEG_BIN}" -hide_banner -nostats -i "$1" \
    -map 0:v:0 -c copy -f null - 2>&1 \
    | awk '/^frame=/{f=$0} END{ if (f=="") exit 1; n=f; sub(/.*frame= */,"",n); sub(/ .*/,"",n); print n }'
}
SEG1_FRAMES="$(seg_frame_count "${SEG1_MP4}")" \
  || die "could not probe seg1 frame count"
SEG2_FRAMES="$(seg_frame_count "${SEG2_MP4}")" \
  || die "could not probe seg2 frame count"
[ "${SEG1_FRAMES}" -gt 0 ] || die "seg1 frame count is zero"
[ "${SEG2_FRAMES}" -gt 0 ] || die "seg2 frame count is zero"

GUEST_SPLICED="$(guest_repo_path "${SPLICED_MP4}")" \
  || die "WORKDIR must be under ${REPO_ROOT}"
GUEST_DECODED_SEG1="/tmp/$(basename "${DECODED_SEG1_RAW}")"
GUEST_DECODED_SEG2="/tmp/$(basename "${DECODED_SEG2_RAW}")"
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

# Pool spec — declare ONE pool per ABR rung. The worker requires
# an exact (width, height, pixel_format) match between the
# launcher's pre-declared IOSurface pool and the shim's
# `OP_CREATE_BUFFER_POOL` request; there is no copy / fallback
# path. FFmpeg's hwaccel re-init across the format change asks the
# shim for a fresh pool at the new resolution, so we declare both
# up-front. Broker caps multi-pool registration at 3
# (TASK_PORT_REGISTER_MAX); two is well under that.
POOL_JSON_SEG1="$(python3 -c "
import json
print(json.dumps({
    'guest_phys_addr': 0x800000000,
    'slot_count': ${SLOT_COUNT},
    'width': ${SEG1_WIDTH},
    'height': ${SEG1_HEIGHT},
    'pixel_format': 0x34323076,
    'writable': True,
}))
")"
POOL_JSON_SEG2="$(python3 -c "
import json
print(json.dumps({
    'guest_phys_addr': 0x900000000,
    'slot_count': ${SLOT_COUNT},
    'width': ${SEG2_WIDTH},
    'height': ${SEG2_HEIGHT},
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
  --pool "${POOL_JSON_SEG1}" \
  --pool "${POOL_JSON_SEG2}" \
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

# Step 3: decode the spliced bitstream via the shim's
# VTDecompressionSession path. FFmpeg's hwaccel-videotoolbox driver
# tracks parameter set changes per AVCodecContext; the second
# segment's SPS/PPS lands as a fresh OP_SET_DECODE_FORMAT on the
# same session id, exercising the worker's
# CanAcceptFormatDescription branch.
#
# Two ffmpeg invocations sharing the same spliced.mp4 input. The
# `rawvideo` muxer can only carry one fixed frame size per output
# and there is no variable-frame-size raw container, so we capture
# each rung's native resolution in a separate pass:
#
#   * Pass 1 — full-stream decode that crosses the splice. Output
#     is capped to `-vframes SEG1_FRAMES` so only seg1's 854×480
#     NV12 frames land in the raw file. Pass 1 is what proves the
#     format-change branch fires end-to-end: the decoder must
#     successfully cross the splice to even reach EOF, and a
#     regression here would surface as a hwaccel
#     "Failed setup for format videotoolbox_vld" log line +
#     fewer-than-N1 frames in the raw output.
#   * Pass 2 — input-side `-ss SEG1_DURATION` seeks the demuxer
#     past the splice straight to seg2's IDR. The VT session
#     starts fresh at 1280×720 (no format change in this pass) so
#     the rawvideo output locks at seg2's native resolution. This
#     is the easy verification half: we re-decode seg2 standalone
#     to capture its exact bytes.
#
# Together the two passes give us byte counts at native
# resolution for both rungs (verifies the per-segment NV12 layout
# math) and at least one full-stream traversal across the splice
# (verifies the worker's format-change branch).
echo "guest: decoding spliced ABR bitstream (pass 1 — full-stream, captures seg1)"
DECODE_STATUS=0
# Image mode: ldconfig already wired the shim libs; skip explicit
# LD_LIBRARY_PATH (an empty value would shadow default search).
if [ -n "${GUEST_SHIM_LIBDIR}" ]; then
  GUEST_LD_PREFIX="LD_LIBRARY_PATH=${GUEST_SHIM_LIBDIR}"
else
  GUEST_LD_PREFIX=""
fi
smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
  "env VT_FERRY_TRANSPORT=vsock VT_FERRY_VSOCK_PORT=${VSOCK_PORT} \
       ${GUEST_LD_PREFIX} \
       ${GUEST_FFMPEG_GUEST_BIN} -hide_banner \
       -hwaccel videotoolbox -i ${GUEST_SPLICED} \
       -vframes ${SEG1_FRAMES} -fps_mode passthrough \
       -f rawvideo -pix_fmt nv12 -y ${GUEST_DECODED_SEG1} \
       -map 0:v -fps_mode passthrough -f null - \
       > ${GUEST_FFMPEG_LOG} 2>&1" \
  || DECODE_STATUS=$?

if [ "${DECODE_STATUS}" -eq 0 ]; then
  echo "guest: decoding spliced ABR bitstream (pass 2 — seg2 via input -ss)"
  smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
    "env VT_FERRY_TRANSPORT=vsock VT_FERRY_VSOCK_PORT=${VSOCK_PORT} \
         ${GUEST_LD_PREFIX} \
         ${GUEST_FFMPEG_GUEST_BIN} -hide_banner \
         -hwaccel videotoolbox -ss ${SEG1_DURATION} -i ${GUEST_SPLICED} \
         -vframes ${SEG2_FRAMES} -fps_mode passthrough \
         -f rawvideo -pix_fmt nv12 -y ${GUEST_DECODED_SEG2} \
         >> ${GUEST_FFMPEG_LOG} 2>&1" \
    || DECODE_STATUS=$?
fi

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
  "${VM_NAME}:${GUEST_DECODED_SEG1}" "${DECODED_SEG1_RAW}" >/dev/null
smolvm_cmd machine cp \
  "${VM_NAME}:${GUEST_DECODED_SEG2}" "${DECODED_SEG2_RAW}" >/dev/null
[ -s "${DECODED_SEG1_RAW}" ] || die "seg1 decoded output is empty: ${DECODED_SEG1_RAW}"
[ -s "${DECODED_SEG2_RAW}" ] || die "seg2 decoded output is empty: ${DECODED_SEG2_RAW}"

# Step 4: verify each per-segment raw matches its native NV12
# layout × per-segment frame count, and the combined total equals
# the sum. A drop, duplicate, or post-format-change mis-routed
# frame lands here as a byte-count mismatch on one of the two
# partitions.
SEG1_BYTES_PER_FRAME="$((SEG1_WIDTH * SEG1_HEIGHT * 3 / 2))"
SEG2_BYTES_PER_FRAME="$((SEG2_WIDTH * SEG2_HEIGHT * 3 / 2))"
SEG1_EXPECTED="$((SEG1_FRAMES * SEG1_BYTES_PER_FRAME))"
SEG2_EXPECTED="$((SEG2_FRAMES * SEG2_BYTES_PER_FRAME))"
EXPECTED_TOTAL="$((SEG1_EXPECTED + SEG2_EXPECTED))"
SEG1_ACTUAL="$(wc -c < "${DECODED_SEG1_RAW}")"
SEG2_ACTUAL="$(wc -c < "${DECODED_SEG2_RAW}")"
ACTUAL_TOTAL="$((SEG1_ACTUAL + SEG2_ACTUAL))"

if [ "${SEG1_ACTUAL}" -ne "${SEG1_EXPECTED}" ]; then
  die "seg1 (${SEG1_WIDTH}x${SEG1_HEIGHT}) size mismatch: \
got ${SEG1_ACTUAL}, expected ${SEG1_EXPECTED} (${SEG1_FRAMES}f × ${SEG1_BYTES_PER_FRAME})"
fi
if [ "${SEG2_ACTUAL}" -ne "${SEG2_EXPECTED}" ]; then
  die "seg2 (${SEG2_WIDTH}x${SEG2_HEIGHT}) size mismatch: \
got ${SEG2_ACTUAL}, expected ${SEG2_EXPECTED} (${SEG2_FRAMES}f × ${SEG2_BYTES_PER_FRAME})"
fi

# Sanity check the format-change branch actually fired in pass 1
# without falling back to software decode. Three signals:
#
#   * "Reconfiguring filter graph because video parameters
#     changed to nv12(...), 1280x720" — ffmpeg's filter chain saw
#     the post-splice frame size at the null sink, meaning decode
#     crossed the splice and the worker's
#     CanAcceptFormatDescription branch fired.
#   * No "Failed setup for format videotoolbox_vld" — hwaccel
#     re-init succeeded.
#   * No "create_buffer_pool failed" — the launcher's two-rung
#     pool spec covered the post-splice request.
if ! grep -q "Reconfiguring filter graph.*${SEG2_WIDTH}x${SEG2_HEIGHT}" "${FFMPEG_LOG}" 2>/dev/null; then
  die "pass 1 did not traverse the splice — no filter-graph \
reconfigure to ${SEG2_WIDTH}x${SEG2_HEIGHT} in the log \
(see ${FFMPEG_LOG})"
fi
if grep -q "Failed setup for format videotoolbox" "${FFMPEG_LOG}" 2>/dev/null; then
  die "pass 1 fell back to software decode at the splice — \
videotoolbox hwaccel re-init failed (see ${FFMPEG_LOG})"
fi
if grep -q "create_buffer_pool failed" "${FFMPEG_LOG}" 2>/dev/null; then
  die "pass 1 hit a buffer-pool mismatch at the splice — \
launcher pool spec needs to cover both ABR rungs (see ${FFMPEG_LOG})"
fi

echo "smolvm videotoolbox ABR decode proof passed"
echo "  seg1=${SEG1_WIDTH}x${SEG1_HEIGHT} frames=${SEG1_FRAMES} bytes=${SEG1_ACTUAL} (expected=${SEG1_EXPECTED})"
echo "  seg2=${SEG2_WIDTH}x${SEG2_HEIGHT} frames=${SEG2_FRAMES} bytes=${SEG2_ACTUAL} (expected=${SEG2_EXPECTED})"
echo "  decoded_bytes=${ACTUAL_TOTAL} (expected=${EXPECTED_TOTAL})"
echo "  workdir=${WORKDIR}"
