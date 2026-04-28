#!/bin/sh
#
# v1 transcode perf gate.
#
# Drives benchmark_host_guest_reference_transcode.sh against the
# bundled 1080p reference clip and asserts the guest VideoToolbox
# transcode path stays within the v1 success bar:
#
#   * guest_vs_host_videotoolbox_ratio < MAX_GUEST_VS_HOST_TRANSCODE_RATIO
#       — guest VT transcode wallclock vs host VT transcode wallclock
#         for the same input. Default cap is 2.0x. Pre-Phase-15 the
#         cap was 1.5x against observed ~0.99x at 300 frames of
#         1080p, but those numbers measured libavcodec decode + VT
#         encode by accident (Phase 15 commit message). With real
#         VT decode + VT encode end-to-end, both directions cross
#         the host-guest boundary, raising the floor to ~1.68x at
#         1080p H.264.
#
#   * guest_videotoolbox_x_realtime >= MIN_GUEST_TRANSCODE_REALTIME
#       — how many times faster than realtime the guest VT
#         transcoder runs at 1080p30. Default floor is 2.0x.
#         Stable baseline observed: ~6.75x.
#
#   * guest_vt_cpu_efficiency_vs_sw <= MAX_GUEST_TRANSCODE_CPU_RATIO
#       — total transcode CPU (decode + encode + transport) for
#         the guest VT path vs guest libx264 software transcode
#         (libavcodec decode + libx264 encode). Default cap is
#         0.5: VT must use at most half of libx264's total
#         transcode CPU. Stable baseline observed: ~0.39 (~60.7%
#         CPU saved at 1080p).
#
# All thresholds are deliberately set well below observed
# baselines so they only fire on real regressions, not on
# benchmark noise (run-to-run variance is typically 10-20%).
#
# Usage:
#   ffmpeg/scripts/v1_transcode_perf_gate.sh
#   FRAME_LIMIT=600 ffmpeg/scripts/v1_transcode_perf_gate.sh
#   MAX_GUEST_VS_HOST_TRANSCODE_RATIO=1.2 ffmpeg/scripts/v1_transcode_perf_gate.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

REFERENCE_VIDEO="${REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4}"
FRAME_LIMIT="${FRAME_LIMIT:-300}"
MAX_GUEST_VS_HOST_TRANSCODE_RATIO="${MAX_GUEST_VS_HOST_TRANSCODE_RATIO:-2.0}"
MIN_GUEST_TRANSCODE_REALTIME="${MIN_GUEST_TRANSCODE_REALTIME:-2.0}"
MAX_GUEST_TRANSCODE_CPU_RATIO="${MAX_GUEST_TRANSCODE_CPU_RATIO:-0.5}"

if [ ! -f "${REFERENCE_VIDEO}" ]; then
    echo "ERROR: REFERENCE_VIDEO not found: ${REFERENCE_VIDEO}" >&2
    exit 1
fi

WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/v1-transcode-perf-gate-$$}"
export WORKDIR

REFERENCE_VIDEO="${REFERENCE_VIDEO}" \
FRAME_LIMIT="${FRAME_LIMIT}" \
"${REPO_ROOT}/ffmpeg/scripts/benchmark_host_guest_reference_transcode.sh"

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
fps = summary["guest"].get("videotoolbox_fps")
cpu_ratio = summary["value_of_hardware_transcode"]["guest_vt_cpu_efficiency_vs_sw"]

max_ratio = float("${MAX_GUEST_VS_HOST_TRANSCODE_RATIO}")
min_realtime = float("${MIN_GUEST_TRANSCODE_REALTIME}")
max_cpu_ratio = float("${MAX_GUEST_TRANSCODE_CPU_RATIO}")

print()
print(f"v1 transcode perf gate:")
if ratio is not None:
    print(f"  guest_vs_host_videotoolbox_ratio={ratio:.3f}x (cap {max_ratio:.2f}x)")
print(f"  guest_videotoolbox_x_realtime={realtime:.1f}x (floor {min_realtime:.1f}x)")
if fps is not None:
    print(f"  guest_videotoolbox_fps={fps:.0f}")
if cpu_ratio is not None:
    print(
        f"  guest_vt_cpu_vs_libx264_transcode={cpu_ratio:.3f} "
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
        f"guest_vt_cpu_vs_libx264_transcode={cpu_ratio:.3f} exceeds cap {max_cpu_ratio:.2f} "
        f"— hardware transcode no longer saves enough CPU vs libx264"
    )

if failures:
    print()
    print("v1 transcode perf gate: FAIL")
    for f in failures:
        print(f"  {f}")
    sys.exit(1)

print("v1 transcode perf gate: pass")
PY
