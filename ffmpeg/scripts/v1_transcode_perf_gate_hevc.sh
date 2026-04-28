#!/bin/sh
#
# HEVC variant of v1_transcode_perf_gate.sh.
#
# Same thresholds, same observed-baseline reasoning as the H.264
# transcode gate — both run through the same VT decode/encode
# silicon. HEVC libx265 software baseline is structurally heavier
# than libx264, so the cpu_efficiency cap stays at 0.5 (VT must
# use ≤50% of libx265's CPU); hardware HEVC encode + decode is
# even more of a CPU win than H.264.
#
# Stages the HEVC reference once (via hevc_videotoolbox) and
# caches it for subsequent runs.
#
# Usage:
#   ffmpeg/scripts/v1_transcode_perf_gate_hevc.sh
#   FRAME_LIMIT=600 ffmpeg/scripts/v1_transcode_perf_gate_hevc.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

HOST_FFMPEG_BIN="${HOST_FFMPEG_BIN:-ffmpeg}"
SOURCE_REFERENCE_VIDEO="${SOURCE_REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4}"
HEVC_REFERENCE_VIDEO="${HEVC_REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_hevc.mp4}"
FRAME_LIMIT="${FRAME_LIMIT:-300}"
MAX_GUEST_VS_HOST_TRANSCODE_RATIO="${MAX_GUEST_VS_HOST_TRANSCODE_RATIO:-2.0}"
MIN_GUEST_TRANSCODE_REALTIME="${MIN_GUEST_TRANSCODE_REALTIME:-2.0}"
MAX_GUEST_TRANSCODE_CPU_RATIO="${MAX_GUEST_TRANSCODE_CPU_RATIO:-0.5}"

die() {
  echo "ERROR: $*" >&2
  exit 1
}

command -v "${HOST_FFMPEG_BIN}" >/dev/null 2>&1 || die "missing host ffmpeg"

if [ ! -f "${HEVC_REFERENCE_VIDEO}" ]; then
  if [ ! -f "${SOURCE_REFERENCE_VIDEO}" ]; then
    die "neither HEVC_REFERENCE_VIDEO (${HEVC_REFERENCE_VIDEO}) nor \
SOURCE_REFERENCE_VIDEO (${SOURCE_REFERENCE_VIDEO}) exists"
  fi
  echo "host: staging HEVC reference at ${HEVC_REFERENCE_VIDEO} (one-time)"
  "${HOST_FFMPEG_BIN}" -hide_banner -y \
    -i "${SOURCE_REFERENCE_VIDEO}" \
    -map 0:v:0 -an \
    -c:v hevc_videotoolbox -profile:v main -b:v 6000000 -bf 0 \
    "${HEVC_REFERENCE_VIDEO}" >/dev/null 2>&1 \
    || die "failed to stage HEVC reference via hevc_videotoolbox"
fi

WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/v1-transcode-perf-gate-hevc-$$}"
export WORKDIR

REFERENCE_VIDEO="${HEVC_REFERENCE_VIDEO}" \
FRAME_LIMIT="${FRAME_LIMIT}" \
SOFTWARE_CODEC="${SOFTWARE_CODEC:-libx265}" \
VIDEOTOOLBOX_CODEC="${VIDEOTOOLBOX_CODEC:-hevc_videotoolbox}" \
"${REPO_ROOT}/ffmpeg/scripts/benchmark_host_guest_reference_transcode.sh"

SUMMARY="${WORKDIR}/summary.json"
[ -f "${SUMMARY}" ] || { echo "ERROR: benchmark did not produce ${SUMMARY}" >&2; exit 1; }

python3 - <<PY
import json
import sys

with open("${SUMMARY}") as f:
    summary = json.load(f)

ratio = summary["overhead"]["guest_vs_host_videotoolbox_ratio"]
realtime = summary["guest"]["videotoolbox_x_realtime"]
fps = summary["guest"].get("videotoolbox_fps")
cpu_ratio = summary["value_of_hardware_transcode"]["guest_vt_cpu_efficiency_vs_sw"]

max_ratio = float("${MAX_GUEST_VS_HOST_TRANSCODE_RATIO}")
min_realtime = float("${MIN_GUEST_TRANSCODE_REALTIME}")
max_cpu_ratio = float("${MAX_GUEST_TRANSCODE_CPU_RATIO}")

print()
print(f"v1 HEVC transcode perf gate:")
if ratio is not None:
    print(f"  guest_vs_host_videotoolbox_ratio={ratio:.3f}x (cap {max_ratio:.2f}x)")
print(f"  guest_videotoolbox_x_realtime={realtime:.1f}x (floor {min_realtime:.1f}x)")
if fps is not None:
    print(f"  guest_videotoolbox_fps={fps:.0f}")
if cpu_ratio is not None:
    print(
        f"  guest_vt_cpu_vs_libx265_transcode={cpu_ratio:.3f} "
        f"(cap {max_cpu_ratio:.2f}; lower is better)"
    )

failures = []
if ratio is not None and ratio >= max_ratio:
    failures.append(
        f"guest_vs_host_videotoolbox_ratio={ratio:.3f}x exceeds cap {max_ratio:.2f}x"
    )
if realtime < min_realtime:
    failures.append(
        f"guest_videotoolbox_x_realtime={realtime:.1f}x below floor {min_realtime:.1f}x"
    )
if cpu_ratio is not None and cpu_ratio > max_cpu_ratio:
    failures.append(
        f"guest_vt_cpu_vs_libx265_transcode={cpu_ratio:.3f} exceeds cap {max_cpu_ratio:.2f}"
    )

if failures:
    print()
    print("v1 HEVC transcode perf gate: FAIL")
    for f in failures:
        print(f"  {f}")
    sys.exit(1)

print("v1 HEVC transcode perf gate: pass")
PY
