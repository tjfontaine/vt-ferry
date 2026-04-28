#!/bin/sh
#
# Phase 9 v1 perf success-bar gate.
#
# Drives benchmark_host_guest_reference_encode.sh against the bundled
# 1080p reference clip and asserts the guest VideoToolbox path stays
# within the v1 success bar:
#
#   * guest_vs_host_videotoolbox_ratio < MAX_GUEST_VS_HOST_VT_RATIO
#       — guest VT encode wallclock vs host VT encode wallclock for
#         the same input. Default cap is 2.0x (the guest path may not
#         be more than twice as slow as the native host path). Stable
#         baseline observed: ~1.08x at 120 frames of 1080p (i.e. the
#         guest is ~8% slower than running natively).
#
#   * guest_videotoolbox_x_realtime >= MIN_GUEST_VT_REALTIME
#       — how many times faster than realtime the guest VT encoder
#         runs at 1080p30. Default floor is 2.0x (i.e. ≥ 60 fps at
#         1080p30). Stable baseline observed: ~5.9x (~177 fps).
#
#   * guest_vt_cpu_efficiency_vs_libx264 <= MAX_GUEST_VT_CPU_RATIO
#       — VideoToolbox should burn dramatically less CPU than libx264
#         to encode the same content. This is the *whole point* of
#         routing the guest through hardware encode rather than
#         shipping libx264 in the container. Default cap is 0.5
#         (VT must use at most 50% of libx264's CPU). Stable
#         baseline observed: ~0.10 (VT uses ~10% of libx264's CPU).
#
# All three thresholds are deliberately set well below the observed
# baseline so they only fire on a real regression, not on benchmark
# noise (run-to-run variance is typically 10-20%).
#
# Usage:
#   ffmpeg/scripts/v1_perf_gate.sh
#   FRAME_LIMIT=600 ffmpeg/scripts/v1_perf_gate.sh
#   MAX_GUEST_VS_HOST_VT_RATIO=1.5 ffmpeg/scripts/v1_perf_gate.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

REFERENCE_VIDEO="${REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4}"
FRAME_LIMIT="${FRAME_LIMIT:-300}"
MAX_GUEST_VS_HOST_VT_RATIO="${MAX_GUEST_VS_HOST_VT_RATIO:-2.0}"
MIN_GUEST_VT_REALTIME="${MIN_GUEST_VT_REALTIME:-2.0}"
MAX_GUEST_VT_CPU_RATIO="${MAX_GUEST_VT_CPU_RATIO:-0.5}"

if [ ! -f "${REFERENCE_VIDEO}" ]; then
    echo "ERROR: REFERENCE_VIDEO not found: ${REFERENCE_VIDEO}" >&2
    exit 1
fi

WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/v1-perf-gate-$$}"
export WORKDIR

REFERENCE_VIDEO="${REFERENCE_VIDEO}" \
FRAME_LIMIT="${FRAME_LIMIT}" \
"${REPO_ROOT}/ffmpeg/scripts/benchmark_host_guest_reference_encode.sh"

SUMMARY="${WORKDIR}/summary.json"
if [ ! -f "${SUMMARY}" ]; then
    echo "ERROR: benchmark did not produce ${SUMMARY}" >&2
    exit 1
fi

python3 - <<PY
import json
import sys

with open("${SUMMARY}") as f:
    summary = json.load(f)

ratio = summary["overhead"]["guest_vs_host_videotoolbox_ratio"]
realtime = summary["guest"]["videotoolbox_x_realtime"]
fps = summary["guest"]["videotoolbox_fps"]
cpu_ratio = summary["value_of_hardware_encode"]["guest_vt_cpu_efficiency_vs_libx264"]

max_ratio = float("${MAX_GUEST_VS_HOST_VT_RATIO}")
min_realtime = float("${MIN_GUEST_VT_REALTIME}")
max_cpu_ratio = float("${MAX_GUEST_VT_CPU_RATIO}")

print()
print(f"v1 perf gate:")
print(f"  guest_vs_host_videotoolbox_ratio={ratio:.3f}x (cap {max_ratio:.2f}x)")
print(f"  guest_videotoolbox_x_realtime={realtime:.1f}x (floor {min_realtime:.1f}x)")
print(f"  guest_videotoolbox_fps={fps:.0f}")
if cpu_ratio is not None:
    print(
        f"  guest_vt_cpu_vs_libx264={cpu_ratio:.3f} "
        f"(cap {max_cpu_ratio:.2f}; lower is better)"
    )

failures = []
if ratio >= max_ratio:
    failures.append(
        f"guest_vs_host_videotoolbox_ratio={ratio:.3f}x exceeds cap {max_ratio:.2f}x"
    )
if realtime < min_realtime:
    failures.append(
        f"guest_videotoolbox_x_realtime={realtime:.1f}x below floor {min_realtime:.1f}x"
    )
if cpu_ratio is not None and cpu_ratio > max_cpu_ratio:
    failures.append(
        f"guest_vt_cpu_vs_libx264={cpu_ratio:.3f} exceeds cap {max_cpu_ratio:.2f} "
        f"— hardware encode no longer saves enough CPU vs libx264"
    )

if failures:
    print()
    print("v1 perf gate: FAIL")
    for f in failures:
        print(f"  {f}")
    sys.exit(1)

print("v1 perf gate: pass")
PY
