#!/bin/sh
#
# Broker-driven smolvm VideoToolbox proof.
#
# vt-ferry-broker registers IOSurface Mach ports for vt-real, starts
# vt-ferry-host-worker, publishes the worker socket via vsock, and execs
# `smolvm machine start`. smolvm/libkrun bridge that vsock port to the guest.
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"
SMOLVM_ROOT="${REPO_ROOT}/third_party/smolvm"

SMOLVM_BIN="${SMOLVM_BIN:-${SMOLVM_ROOT}/target/release/smolvm}"
SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS:-${SMOLVM_ROOT}/target/agent-rootfs}"
HOST_WORKER_BIN="${HOST_WORKER_BIN:-${REPO_ROOT}/target/debug/vt-ferry-worker}"
BROKER_BIN="${BROKER_BIN:-${REPO_ROOT}/target/release/vt-ferry-broker}"
FFMPEG_BIN="${FFMPEG_BIN:-${REPO_ROOT}/artifacts/ffmpeg-build/n8.1-linux-debug/ffmpeg}"
SHIM_LIBDIR="${SHIM_LIBDIR:-${REPO_ROOT}/artifacts/ffmpeg-shim-libs/linux-debug}"
SHIM_TARGET_TRIPLE="${SHIM_TARGET_TRIPLE:-aarch64-unknown-linux-gnu}"
STAGE_SHIM_LIBS="${STAGE_SHIM_LIBS:-1}"
FFPROBE_BIN="${FFPROBE_BIN:-ffprobe}"
VM_IMAGE="${VM_IMAGE:-public.ecr.aws/docker/library/ubuntu:24.04}"
VM_NAME="${VM_NAME:-vt-ferry-ffmpeg-broker-$$}"
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/ffmpeg-broker-proof-$$}"
FRAME_SIZE="${FRAME_SIZE:-128x72}"
FRAME_RATE="${FRAME_RATE:-4}"
DURATION="${DURATION-1}"
FRAME_COUNT="${FRAME_COUNT:-}"
PIXEL_FORMAT="${PIXEL_FORMAT:-0x34323076}"
GUEST_CODEC_ARGS="${GUEST_CODEC_ARGS:-}"
# Optional FFmpeg filter-graph fragment inserted BEFORE the
# `format=<pix>` stage. Lets richer pipelines exercise the shim
# without forking the smoke — e.g.
# `PRE_FORMAT_FILTERS='scale=1280x720,fps=24'` so the encoder sees a
# different size/fps than the source, validating that the shim
# tolerates dimension changes between source and encoder. Multiple
# filters must be comma-separated (FFmpeg `-vf` syntax).
PRE_FORMAT_FILTERS="${PRE_FORMAT_FILTERS:-}"
# Probe overrides for cases where the encoded output's reported
# dimensions intentionally differ from the source (e.g. when
# PRE_FORMAT_FILTERS includes `scale=...`). Falls back to source
# dimensions when unset.
EXPECTED_OUTPUT_WIDTH="${EXPECTED_OUTPUT_WIDTH:-}"
EXPECTED_OUTPUT_HEIGHT="${EXPECTED_OUTPUT_HEIGHT:-}"
# Audio-stream handling. Default `-an` strips audio (matches the
# original smoke since lavfi inputs have no audio). Override with
# `AUDIO_ARGS='-c:a copy'` when REFERENCE_VIDEO carries an audio
# track that should pass through, or `AUDIO_ARGS='-c:a aac'` to
# exercise a software audio re-encode in parallel with VT video.
# This is purely upstream/downstream of the shim — the audio
# pipeline doesn't talk to VideoToolbox at all — so passing it
# through validates that the shim doesn't disturb FFmpeg's
# multi-stream demux/mux paths.
AUDIO_ARGS="${AUDIO_ARGS:--an}"
# Whether the smoke should require an audio stream in the output.
# Defaults to off so the existing tests stay green; container/audio
# wrappers set it to "1" to assert the audio track survived.
EXPECT_AUDIO_STREAM="${EXPECT_AUDIO_STREAM:-0}"
# Full-output override. When set, replaces the entire portion of
# the ffmpeg command after the input args (`-i ${REFERENCE_VIDEO}`
# or the lavfi source) — so the wrapper is responsible for `-map`,
# `-vf`, `-c:v`, codec args, and output file(s). Designed for
# multi-output scenarios (one input → multiple encoder sessions
# in flight, each opening its own VTCompressionSession). Setting
# this env var also implies `SKIP_PROBE_ASSERTIONS=1` since the
# default single-output probe doesn't apply.
FFMPEG_OUTPUT_OVERRIDE="${FFMPEG_OUTPUT_OVERRIDE:-}"
# Skip the default ffprobe assertions on `${OUTPUT_MP4}`. The
# encode still runs to completion and the log is captured; the
# wrapper does its own validation.
SKIP_PROBE_ASSERTIONS="${SKIP_PROBE_ASSERTIONS:-0}"
if [ -n "${FFMPEG_OUTPUT_OVERRIDE}" ]; then
  SKIP_PROBE_ASSERTIONS=1
fi
# FFmpeg encoder name to pass after `-c:v`. Defaults to H.264 over the
# VideoToolbox guest shim. Set GUEST_CODEC=hevc_videotoolbox to drive
# the HEVC path through the same shim.
GUEST_CODEC="${GUEST_CODEC:-h264_videotoolbox}"
# Expected codec_name in the ffprobe output. Probe-side validation
# uses this to assert the decoder roundtrips back to the same codec.
EXPECTED_CODEC_NAME="${EXPECTED_CODEC_NAME:-h264}"
REFERENCE_VIDEO="${REFERENCE_VIDEO:-}"
# Optional extra args inserted before `-i` in the guest ffmpeg
# invocation. Empty by default → FFmpeg picks the software demuxer
# + decoder. Set to `-hwaccel videotoolbox` to drive the transcode
# path through the shim's VTDecompressionSession (this is how
# prove_smolvm_videotoolbox_transcode.sh exercises decode + encode
# in the same FFmpeg run).
INPUT_DECODER_ARGS="${INPUT_DECODER_ARGS:-}"
# Default pool slot count: lavfi smokes need only a handful of slots; reading
# from a real video file means FFmpeg keeps several frames in flight, so the
# IOSurface pool must be larger or ALLOC_BUFFER will exhaust before recycling.
if [ -n "${REFERENCE_VIDEO}" ]; then
  SLOT_COUNT="${SLOT_COUNT:-32}"
else
  SLOT_COUNT="${SLOT_COUNT:-4}"
fi
VSOCK_PORT="${VSOCK_PORT:-6600}"
case "${PIXEL_FORMAT}" in
  0x42475241|1111970369) FFMPEG_GUEST_FORMAT=bgra ;;
  0x34323076|875704438)  FFMPEG_GUEST_FORMAT=nv12 ;;
  # P010 (10-bit 4:2:0): 'x420' = 0x78343230 (video range),
  # 'xf20' = 0x78663230 (full range). FFmpeg's swscale stages it as
  # `p010le` on macOS / Linux. Pairs with hevc_videotoolbox + Main10.
  0x78343230|2016686640) FFMPEG_GUEST_FORMAT=p010le ;;
  0x78663230|2019963440) FFMPEG_GUEST_FORMAT=p010le ;;
  *) FFMPEG_GUEST_FORMAT=nv12 ;;
esac

LAUNCHER_PID=""

die() {
  echo "ERROR: $*" >&2
  exit 1
}

smolvm_cmd() {
  SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS}" \
  "${SMOLVM_BIN}" "$@"
}

now_ms() {
  python3 -c 'import time; print(int(time.monotonic_ns() / 1_000_000))'
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

require_file() {
  [ -e "$1" ] || die "missing required path: $1"
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

guest_repo_path() {
  case "$1" in
    "${REPO_ROOT}") printf '/repo\n' ;;
    "${REPO_ROOT}"/*) printf '/repo/%s\n' "${1#${REPO_ROOT}/}" ;;
    *) return 1 ;;
  esac
}

require_file "${SMOLVM_BIN}"
require_file "${SMOLVM_AGENT_ROOTFS}"
require_file "${HOST_WORKER_BIN}"
require_file "${BROKER_BIN}"
require_cmd "${FFPROBE_BIN}"

# Picks up host-artifact vs image mode based on USE_VT_FERRY_IMAGE.
# See the helper file for the full contract.
. "${SCRIPT_DIR}/lib/image_mode.sh"
smoke_validate_host_artifacts

mkdir -p "${WORKDIR}/tmp"
# Output container extension. ffmpeg picks the muxer from the
# extension, so this is the cheapest knob to validate that container
# choice doesn't perturb the encoder shim path. Supported by default:
# mp4, mov, mkv, ts. Variable name keeps `OUTPUT_MP4` for backward
# compatibility — same path, just a different suffix.
OUTPUT_EXT="${OUTPUT_EXT:-mp4}"
OUTPUT_MP4="${WORKDIR}/out.${OUTPUT_EXT}"
FFMPEG_LOG="${WORKDIR}/ffmpeg-run.log"
LAUNCHER_LOG="${WORKDIR}/launcher.log"
SCRIPT_START_MS="$(now_ms)"

smoke_resolve_guest_paths
GUEST_OUTPUT_MP4="/tmp/$(basename "${OUTPUT_MP4}")"
GUEST_FFMPEG_LOG="/tmp/$(basename "${FFMPEG_LOG}")"

if [ -n "${REFERENCE_VIDEO}" ]; then
  require_file "${REFERENCE_VIDEO}"
  GUEST_REFERENCE_VIDEO="$(guest_repo_path "${REFERENCE_VIDEO}")" \
    || die "REFERENCE_VIDEO must be under ${REPO_ROOT}"
  ref_probe="$("${FFPROBE_BIN}" -hide_banner -select_streams v:0 \
    -show_streams -print_format json "${REFERENCE_VIDEO}" 2>/dev/null)"
  read width height <<EOF
$(python3 - <<PY
import json
probe = json.loads("""${ref_probe}""")
stream = probe["streams"][0]
print(stream["width"], stream["height"])
PY
)
EOF
  [ -n "${width}" ] && [ -n "${height}" ] || die "failed to probe REFERENCE_VIDEO dimensions"
else
  width="${FRAME_SIZE%x*}"
  height="${FRAME_SIZE#*x}"
  [ -n "${width}" ] && [ -n "${height}" ] || die "invalid FRAME_SIZE: ${FRAME_SIZE}"
fi

frame_count_arg=""
if [ -n "${FRAME_COUNT}" ]; then
  frame_count_arg="-frames:v ${FRAME_COUNT}"
fi

if [ -n "${REFERENCE_VIDEO}" ]; then
  input_args="-i ${GUEST_REFERENCE_VIDEO}"
else
  input_filter="testsrc2=size=${FRAME_SIZE}:rate=${FRAME_RATE}"
  if [ -n "${DURATION}" ]; then
    input_filter="${input_filter}:duration=${DURATION}"
  fi
  input_args="-f lavfi -i ${input_filter}"
fi

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

# When the guest pipeline does VT decode AND VT encode in the
# same process (`INPUT_DECODER_ARGS` carries `-hwaccel
# videotoolbox`), both sides claim a distinct
# `IOSurfacePoolDirectory` entry — the decoder for its
# `AVHWFramesContext` output pool, the encoder for its
# `VTCompressionSessionGetPixelBufferPool` input pool. A single
# launcher-registered pool entry would let one of them claim and
# starve the other; we register a second entry at the same
# shape so each gets its own. The `guest_phys_addr` value picks
# a nominal page-aligned offset distinct from the first pool's
# 0x800000000 — the broker only uses it to disambiguate
# entries, not as a real address.
EXTRA_POOL_JSON=""
case "${INPUT_DECODER_ARGS}" in
  *-hwaccel*videotoolbox*)
    # Compact `separators=(',', ':')` keeps the JSON as a
    # single shell word — otherwise the `${EXTRA_POOL_ARG}`
    # expansion below would split on the spaces json.dumps
    # adds by default and break the broker's argv parsing.
    EXTRA_POOL_JSON="$(python3 -c "
import json
print(json.dumps({
    'guest_phys_addr': 0x900000000,
    'slot_count': ${SLOT_COUNT},
    'width': ${width},
    'height': ${height},
    'pixel_format': ${PIXEL_FORMAT},
    'writable': True,
}, separators=(',', ':')))
")"
    ;;
esac

"${REPO_ROOT}/third_party/prepare_krun_runtime.sh" >/dev/null
. "${REPO_ROOT}/artifacts/krun-runtime/macos-arm64/env.sh"

smolvm_cmd machine create \
  --net \
  --image "${VM_IMAGE}" \
  "${VM_NAME}" \
  -v "${REPO_ROOT}:/repo" \
  >/dev/null

# Background `vt-ferry-broker -- smolvm machine start`. The broker execs
# smolvm in the same task (so registered Mach ports are visible). smolvm
# consumes the broker-provided transport metadata and runs the VM in foreground
# (no fork, which would lose the registered ports). We background here so
# the rest of the script can issue `machine exec` against the running VM.
if [ -n "${EXTRA_POOL_JSON}" ]; then
  SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS}" \
  "${BROKER_BIN}" \
    --vsock-port "${VSOCK_PORT}" \
    --pool "${POOL_JSON}" \
    --pool "${EXTRA_POOL_JSON}" \
    --host-worker "${HOST_WORKER_BIN}" \
    -- "${SMOLVM_BIN}" machine start --name "${VM_NAME}" \
    >"${LAUNCHER_LOG}" 2>&1 &
else
  SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS}" \
  "${BROKER_BIN}" \
    --vsock-port "${VSOCK_PORT}" \
    --pool "${POOL_JSON}" \
    --host-worker "${HOST_WORKER_BIN}" \
    -- "${SMOLVM_BIN}" machine start --name "${VM_NAME}" \
    >"${LAUNCHER_LOG}" 2>&1 &
fi
LAUNCHER_PID=$!

# Wait for the launcher to finish image pull + init and report "running
# (PID ...)". Polling `machine status` too eagerly here is dangerous: an
# early probe while the guest agent is busy finishing init times out,
# status_vm doesn't detach on failure, and its manager Drop sends
# Shutdown to the VM we just started.
for _ in $(seq 1 120); do
  if grep -q "running (PID" "${LAUNCHER_LOG}" 2>/dev/null; then
    break
  fi
  if ! kill -0 "${LAUNCHER_PID}" 2>/dev/null; then
    die "broker/smolvm exited before VM became ready; log: $(cat "${LAUNCHER_LOG}")"
  fi
  sleep 1
done
# Give the agent a moment to drain post-init background tasks before
# other smolvm invocations start pinging it.
sleep 3

smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
  'apt-get update >/tmp/apt-update.log 2>&1 && DEBIAN_FRONTEND=noninteractive apt-get install -y libxcb1 libxcb-shm0 libxau6 libxdmcp6 >/tmp/apt-install.log 2>&1'

GUEST_TRANSPORT_ENV="VT_FERRY_TRANSPORT=vsock VT_FERRY_VSOCK_PORT=${VSOCK_PORT}"

GUEST_FFMPEG_START_MS="$(now_ms)"
guest_ffmpeg_status=0
if [ -n "${PRE_FORMAT_FILTERS}" ]; then
  vf_arg="-vf ${PRE_FORMAT_FILTERS},format=${FFMPEG_GUEST_FORMAT}"
else
  vf_arg="-vf format=${FFMPEG_GUEST_FORMAT}"
fi
if [ -n "${FFMPEG_OUTPUT_OVERRIDE}" ]; then
  # Override mode — the wrapper supplied the entire output spec
  # (mapping, filters, codec, codec args, output paths). Used by
  # multi-output smokes that stand up two encoder sessions
  # concurrently in one ffmpeg run.
  ffmpeg_full_cmd="${GUEST_FFMPEG_BIN} -hide_banner ${INPUT_DECODER_ARGS} ${input_args} ${FFMPEG_OUTPUT_OVERRIDE}"
else
  if [ "${AUDIO_ARGS}" = "-an" ]; then
    map_args="-map 0:v:0 ${AUDIO_ARGS}"
  else
    # Pass through both video and audio streams when audio is wanted.
    # `-map 0:a:0?` is conditional so synthetic lavfi sources without
    # audio don't fail the run.
    map_args="-map 0:v:0 -map 0:a:0? ${AUDIO_ARGS}"
  fi
  ffmpeg_full_cmd="${GUEST_FFMPEG_BIN} -hide_banner ${INPUT_DECODER_ARGS} ${input_args} ${map_args} ${frame_count_arg} ${vf_arg} -c:v ${GUEST_CODEC} ${GUEST_CODEC_ARGS} -y ${GUEST_OUTPUT_MP4}"
fi
guest_env="$(smoke_guest_env_prefix "${GUEST_TRANSPORT_ENV}")"
smolvm_cmd machine exec --name "${VM_NAME}" -- sh -lc \
  "env ${guest_env} ${ffmpeg_full_cmd} > ${GUEST_FFMPEG_LOG} 2>&1" \
  || guest_ffmpeg_status=$?
GUEST_FFMPEG_END_MS="$(now_ms)"
# Always copy the guest ffmpeg log back so a failure surfaces the
# encoder diagnostics, not just an exit code.
smolvm_cmd machine cp \
  "${VM_NAME}:${GUEST_FFMPEG_LOG}" "${FFMPEG_LOG}" >/dev/null 2>&1 || true
if [ "${guest_ffmpeg_status}" -ne 0 ]; then
  echo "ERROR: guest ffmpeg exited ${guest_ffmpeg_status}; log at ${FFMPEG_LOG}" >&2
  if [ -f "${FFMPEG_LOG}" ]; then
    echo "--- guest ffmpeg log (last 60 lines) ---" >&2
    tail -60 "${FFMPEG_LOG}" >&2
    echo "--- end guest ffmpeg log ---" >&2
  fi
  exit "${guest_ffmpeg_status}"
fi
GUEST_FFMPEG_EXEC_MS="$((GUEST_FFMPEG_END_MS - GUEST_FFMPEG_START_MS))"

if [ "${SKIP_PROBE_ASSERTIONS}" = "1" ]; then
  # Wrapper-managed validation. Refresh the ffmpeg log and copy
  # any wrapper-specified guest outputs back to the host workdir,
  # then exit cleanly so the wrapper can run its own ffprobe.
  smolvm_cmd machine cp \
    "${VM_NAME}:${GUEST_FFMPEG_LOG}" "${FFMPEG_LOG}" >/dev/null
  if [ -n "${MULTI_OUTPUT_PATHS:-}" ]; then
    # Colon-separated list of guest paths (e.g. /tmp/out_720p.mp4:
    # /tmp/out_1080p.mp4). Each gets copied to ${WORKDIR}/<basename>.
    OLD_IFS="${IFS}"
    IFS=':'
    set -- ${MULTI_OUTPUT_PATHS}
    IFS="${OLD_IFS}"
    for guest_path in "$@"; do
      host_path="${WORKDIR}/$(basename "${guest_path}")"
      smolvm_cmd machine cp \
        "${VM_NAME}:${guest_path}" "${host_path}" >/dev/null
      [ -s "${host_path}" ] || die "empty multi-stream output: ${host_path}"
    done
  fi
  echo "backend=vt-real (broker exec wrapper, vsock; probe assertions skipped)"
  echo "guest_ffmpeg_exec_ms=${GUEST_FFMPEG_EXEC_MS}"
  echo "launcher_pid=${LAUNCHER_PID}"
  echo "workdir=${WORKDIR}"
  exit 0
fi

smolvm_cmd machine cp \
  "${VM_NAME}:${GUEST_OUTPUT_MP4}" "${OUTPUT_MP4}" >/dev/null
smolvm_cmd machine cp \
  "${VM_NAME}:${GUEST_FFMPEG_LOG}" "${FFMPEG_LOG}" >/dev/null

[ -f "${OUTPUT_MP4}" ] || die "missing output file: ${OUTPUT_MP4}"
[ -s "${OUTPUT_MP4}" ] || die "empty output file: ${OUTPUT_MP4}"

probe_json="$("${FFPROBE_BIN}" -hide_banner -show_streams -show_format -print_format json "${OUTPUT_MP4}")"

# Decode-pass count: actually decodes the bitstream (rather than just
# reading container metadata) and reports how many frames came out.
# Catches malformed parameter-set deliveries where ffprobe's metadata
# read is happy but the decoder rejects the stream — a regression
# class the existing checks miss because nb_frames comes from the
# container, not from a real decode.
nb_decoded="$("${FFPROBE_BIN}" -hide_banner -count_frames \
    -select_streams v:0 -show_entries stream=nb_read_frames \
    -of default=noprint_wrappers=1:nokey=1 "${OUTPUT_MP4}" 2>/dev/null)"

python3 - <<PY
import json
probe = json.loads("""${probe_json}""")
streams = [s for s in probe.get("streams", []) if s.get("codec_type") == "video"]
if len(streams) != 1:
    raise SystemExit(f"expected exactly one video stream, got {len(streams)}")
stream = streams[0]
expected_codec = "${EXPECTED_CODEC_NAME}"
if stream.get("codec_name") != expected_codec:
    raise SystemExit(f"expected {expected_codec} codec, got {stream.get('codec_name')}")
expected_w = "${EXPECTED_OUTPUT_WIDTH}" or "${width}"
expected_h = "${EXPECTED_OUTPUT_HEIGHT}" or "${height}"
if int(stream.get("width", 0)) != int(expected_w):
    raise SystemExit(f"expected width {expected_w}, got {stream.get('width')}")
if int(stream.get("height", 0)) != int(expected_h):
    raise SystemExit(f"expected height {expected_h}, got {stream.get('height')}")
if int(stream.get("nb_frames", 0)) < 1:
    raise SystemExit(f"expected at least one frame, got {stream.get('nb_frames')}")
nb_decoded_str = "${nb_decoded}".strip()
if not nb_decoded_str.isdigit():
    raise SystemExit(f"decode-pass returned non-numeric: {nb_decoded_str!r}")
nb_decoded = int(nb_decoded_str)
if nb_decoded < int(stream.get("nb_frames", 0)):
    raise SystemExit(
        f"decode-pass yielded {nb_decoded} frames but container reports "
        f"{stream.get('nb_frames')} — bitstream is malformed even though "
        f"the container is internally consistent. likely a parameter-set "
        f"delivery regression"
    )
if "${EXPECT_AUDIO_STREAM}" == "1":
    audio_streams = [s for s in probe.get("streams", []) if s.get("codec_type") == "audio"]
    if len(audio_streams) < 1:
        raise SystemExit(f"expected at least one audio stream, got {len(audio_streams)}")
    a = audio_streams[0]
    if not a.get("codec_name"):
        raise SystemExit(f"audio stream missing codec_name: {a}")
print("smolvm videotoolbox broker proof passed")
PY

SCRIPT_END_MS="$(now_ms)"
SCRIPT_TOTAL_MS="$((SCRIPT_END_MS - SCRIPT_START_MS))"

echo "backend=vt-real (broker exec wrapper, vsock)"
echo "guest_ffmpeg_exec_ms=${GUEST_FFMPEG_EXEC_MS}"
echo "guest_total_ms=${SCRIPT_TOTAL_MS}"
echo "launcher_pid=${LAUNCHER_PID}"
echo "workdir=${WORKDIR}"
echo "output=${OUTPUT_MP4}"
