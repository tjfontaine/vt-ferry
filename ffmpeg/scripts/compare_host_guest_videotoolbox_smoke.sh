#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

HOST_FFMPEG_BIN="${HOST_FFMPEG_BIN:-ffmpeg}"
FFPROBE_BIN="${FFPROBE_BIN:-ffprobe}"
FRAME_SIZE="${FRAME_SIZE:-128x72}"
FRAME_RATE="${FRAME_RATE:-4}"
FRAME_COUNT="${FRAME_COUNT:-4}"
WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/ffmpeg-host-guest-compare-$$}"
HOST_MP4="${WORKDIR}/host.mp4"
GUEST_WORKDIR="${WORKDIR}/guest"
GUEST_MP4="${GUEST_WORKDIR}/out.mp4"
GUEST_CODEC_ARGS="${GUEST_CODEC_ARGS:-}"
HOST_CODEC_ARGS="${HOST_CODEC_ARGS:-}"
MIN_PSNR_AVERAGE="${MIN_PSNR_AVERAGE:-}"
MIN_PSNR_MIN="${MIN_PSNR_MIN:-}"
MIN_SSIM_ALL="${MIN_SSIM_ALL:-}"
REFERENCE_VIDEO="${REFERENCE_VIDEO:-}"

die() {
  echo "ERROR: $*" >&2
  exit 1
}

now_ms() {
  python3 -c 'import time; print(int(time.monotonic_ns() / 1_000_000))'
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

mkdir -p "${WORKDIR}"
require_cmd "${FFPROBE_BIN}"
require_cmd "${HOST_FFMPEG_BIN}"

if [ -n "${REFERENCE_VIDEO}" ]; then
  [ -f "${REFERENCE_VIDEO}" ] || die "REFERENCE_VIDEO not found: ${REFERENCE_VIDEO}"
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

if [ -z "${GUEST_CODEC_ARGS}" ]; then
  GUEST_CODEC_ARGS="-profile:v main -b:v 6000000 -bf 0"
fi
if [ -z "${HOST_CODEC_ARGS}" ]; then
  HOST_CODEC_ARGS="${GUEST_CODEC_ARGS}"
fi

host_start_ms="$(now_ms)"
if [ -n "${REFERENCE_VIDEO}" ]; then
  "${HOST_FFMPEG_BIN}" -hide_banner -y \
    -i "${REFERENCE_VIDEO}" \
    -map 0:v:0 -an \
    -frames:v "${FRAME_COUNT}" \
    -vf format=nv12 \
    -c:v h264_videotoolbox \
    ${HOST_CODEC_ARGS} \
    "${HOST_MP4}" >/dev/null 2>&1
else
  "${HOST_FFMPEG_BIN}" -hide_banner -y \
    -f lavfi -i "testsrc2=size=${FRAME_SIZE}:rate=${FRAME_RATE}" \
    -frames:v "${FRAME_COUNT}" \
    -vf format=nv12 \
    -c:v h264_videotoolbox \
    ${HOST_CODEC_ARGS} \
    "${HOST_MP4}" >/dev/null 2>&1
fi
host_end_ms="$(now_ms)"
HOST_FFMPEG_MS="$((host_end_ms - host_start_ms))"

guest_output="$(
WORKDIR="${GUEST_WORKDIR}" \
FRAME_SIZE="${FRAME_SIZE}" \
FRAME_RATE="${FRAME_RATE}" \
FRAME_COUNT="${FRAME_COUNT}" \
DURATION="" \
GUEST_CODEC_ARGS="${GUEST_CODEC_ARGS}" \
HOST_WORKER_BACKEND=vt-real \
REFERENCE_VIDEO="${REFERENCE_VIDEO}" \
sh "${REPO_ROOT}/ffmpeg/scripts/prove_smolvm_videotoolbox_encode.sh"
)"

GUEST_FFMPEG_MS="$(printf '%s\n' "${guest_output}" | awk -F= '/^guest_ffmpeg_exec_ms=/{print $2; exit}')"
[ -n "${GUEST_FFMPEG_MS}" ] || die "missing guest_ffmpeg_exec_ms in guest proof output"
GUEST_TOTAL_MS="$(printf '%s\n' "${guest_output}" | awk -F= '/^guest_total_ms=/{print $2; exit}')"
[ -n "${GUEST_TOTAL_MS}" ] || die "missing guest_total_ms in guest proof output"

[ -f "${HOST_MP4}" ] || die "missing host mp4: ${HOST_MP4}"
[ -f "${GUEST_MP4}" ] || die "missing guest mp4: ${GUEST_MP4}"

HOST_PROBE_JSON="$("${FFPROBE_BIN}" -hide_banner -show_streams -show_format -print_format json "${HOST_MP4}")"
GUEST_PROBE_JSON="$("${FFPROBE_BIN}" -hide_banner -show_streams -show_format -print_format json "${GUEST_MP4}")"

PSNR_STATS="${WORKDIR}/psnr.stats"
SSIM_STATS="${WORKDIR}/ssim.stats"
PSNR_STDERR="${WORKDIR}/psnr.stderr"
SSIM_STDERR="${WORKDIR}/ssim.stderr"

"${HOST_FFMPEG_BIN}" -hide_banner -i "${HOST_MP4}" -i "${GUEST_MP4}" \
  -lavfi "[0:v][1:v]psnr=stats_file=${PSNR_STATS}" \
  -f null - 2>"${PSNR_STDERR}" || die "psnr ffmpeg pass failed; see ${PSNR_STDERR}"
"${HOST_FFMPEG_BIN}" -hide_banner -i "${HOST_MP4}" -i "${GUEST_MP4}" \
  -lavfi "[0:v][1:v]ssim=stats_file=${SSIM_STATS}" \
  -f null - 2>"${SSIM_STDERR}" || die "ssim ffmpeg pass failed; see ${SSIM_STDERR}"

PSNR_SUMMARY="$(grep -E "Parsed_psnr.*PSNR" "${PSNR_STDERR}" | tail -1)"
SSIM_SUMMARY="$(grep -E "Parsed_ssim.*SSIM" "${SSIM_STDERR}" | tail -1)"

python3 - <<PY
import json
import re

host = json.loads("""${HOST_PROBE_JSON}""")
guest = json.loads("""${GUEST_PROBE_JSON}""")
host_ffmpeg_ms = int("${HOST_FFMPEG_MS}")
guest_ffmpeg_ms = int("${GUEST_FFMPEG_MS}")
guest_total_ms = int("${GUEST_TOTAL_MS}")
frame_count = int("${FRAME_COUNT}")
psnr_summary = """${PSNR_SUMMARY}""".strip()
ssim_summary = """${SSIM_SUMMARY}""".strip()
min_psnr_average = """${MIN_PSNR_AVERAGE}""".strip() or None
min_psnr_min = """${MIN_PSNR_MIN}""".strip() or None
min_ssim_all = """${MIN_SSIM_ALL}""".strip() or None

def first_video_stream(data):
    for stream in data.get("streams", []):
        if stream.get("codec_type") == "video":
            return stream
    raise SystemExit("missing video stream")

host_stream = first_video_stream(host)
guest_stream = first_video_stream(guest)

fields = [
    ("codec_name", host_stream.get("codec_name"), guest_stream.get("codec_name")),
    ("profile", host_stream.get("profile"), guest_stream.get("profile")),
    ("width", host_stream.get("width"), guest_stream.get("width")),
    ("height", host_stream.get("height"), guest_stream.get("height")),
    ("pix_fmt", host_stream.get("pix_fmt"), guest_stream.get("pix_fmt")),
    ("level", host_stream.get("level"), guest_stream.get("level")),
    ("nb_frames", host_stream.get("nb_frames"), guest_stream.get("nb_frames")),
    ("extradata_size", host_stream.get("extradata_size"), guest_stream.get("extradata_size")),
    ("bit_rate", host_stream.get("bit_rate"), guest_stream.get("bit_rate")),
]

print("host:", "${HOST_MP4}")
print("guest:", "${GUEST_MP4}")
print("")
print(f"host_ffmpeg_ms: {host_ffmpeg_ms}")
print(f"guest_ffmpeg_exec_ms: {guest_ffmpeg_ms}")
print(f"guest_total_ms: {guest_total_ms}")
print(f"guest_exec_overhead_ms: {guest_ffmpeg_ms - host_ffmpeg_ms}")
if host_ffmpeg_ms > 0:
    print(f"guest_exec_ratio: {guest_ffmpeg_ms / host_ffmpeg_ms:.3f}x")
if frame_count > 0:
    print(f"host_ms_per_frame: {host_ffmpeg_ms / frame_count:.3f}")
    print(f"guest_exec_ms_per_frame: {guest_ffmpeg_ms / frame_count:.3f}")
print("")
for key, host_value, guest_value in fields:
    status = "match" if host_value == guest_value else "diff"
    print(f"{key}: {status} host={host_value} guest={guest_value}")

print("")
def parse_metric(text, key):
    m = re.search(rf"\b{key}:(inf|[\d.]+)", text)
    return float(m.group(1)) if m else None

psnr_average = parse_metric(psnr_summary, "average")
psnr_min = parse_metric(psnr_summary, "min")
ssim_all = parse_metric(ssim_summary, "All")

print(f"psnr_summary: {psnr_summary or 'unavailable'}")
if psnr_average is not None:
    print(f"psnr_average_db: {psnr_average:.3f}")
if psnr_min is not None:
    print(f"psnr_min_db: {psnr_min:.3f}")
print(f"ssim_summary: {ssim_summary or 'unavailable'}")
if ssim_all is not None:
    print(f"ssim_all: {ssim_all:.4f}")

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
    raise SystemExit("fidelity gate failed: " + "; ".join(failures))
PY

echo ""
echo "workdir=${WORKDIR}"
