#!/bin/sh
#
# Phase 11 v1 decode-side perf gate.
#
# Decode-side analogue to v1_perf_gate.sh. Drives
# benchmark_host_guest_reference_decode.sh against the bundled 1080p
# reference clip and asserts the guest VideoToolbox *decode* path
# stays within the v1 success bar:
#
#   * guest_vs_host_videotoolbox_ratio < MAX_GUEST_VS_HOST_DECODE_RATIO
#       — guest VT decode wallclock vs host VT decode wallclock for
#         the same input. Default cap is 3.0x at 1080p H.264.
#         Decode is cheaper than encode on the host (~10x faster)
#         so the absolute wallclock is sub-second and the host-guest
#         data crossing for the raw NV12 output dominates the
#         guest's overhead. Run-to-run host wallclock variance can
#         move the ratio by 50%+ even though the guest's absolute
#         fps + CPU savings stay stable. Pre-Phase-15 the cap was
#         2.0x against observed ~0.36x, but those numbers were
#         libavcodec-vs-libavcodec by accident (see Phase 15 commit
#         message). Real VT decode at 1080p H.264 sits between 1.89x
#         and 2.65x across runs at FRAME_LIMIT=300; FRAME_LIMIT=1200
#         shrinks the noise window if a real regression is suspected.
#
#   * guest_videotoolbox_x_realtime >= MIN_GUEST_DECODE_REALTIME
#       — how many times faster than realtime the guest VT decoder
#         runs at 1080p30. Default floor is 5.0x (i.e. ≥ 150 fps at
#         1080p30). Hardware decode is much cheaper than hardware
#         encode, so the floor is correspondingly higher.
#
#   * guest_vt_cpu_efficiency_vs_sw <= MAX_GUEST_DECODE_CPU_RATIO
#       — guest VT decode CPU vs guest libavcodec decode CPU,
#         comparing the two paths *inside the same VM*. Default
#         cap is 1.5x. Decode is structurally different from encode
#         here: the output of decode is raw NV12 (multi-MiB per
#         frame), and the shimmed path has to move those bytes
#         across vsock via chunked OP_READ_BUFFER. That transport
#         cost burns roughly as much CPU as libavcodec's decode
#         work would have. The encode-side equivalent metric
#         (`MAX_GUEST_VT_CPU_RATIO=0.5`) wins big because compressed
#         output is small (~10s of KiB/frame) — decode doesn't have
#         that lever. The real value of guest VT decode is offloading
#         actual decode work from the VM CPU to host GPU/ANE silicon,
#         which the host sees as freed cycles even when the VM's CPU
#         counters look the same.
#
# All three thresholds are deliberately loose — they fail only on a
# real regression, not on benchmark noise (run-to-run variance is
# typically 10-20% on a busy laptop).
#
# Usage:
#   ffmpeg/scripts/v1_decode_perf_gate.sh
#   FRAME_LIMIT=600 ffmpeg/scripts/v1_decode_perf_gate.sh
#   MAX_GUEST_VS_HOST_DECODE_RATIO=1.5 ffmpeg/scripts/v1_decode_perf_gate.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

REFERENCE_VIDEO="${REFERENCE_VIDEO:-${REPO_ROOT}/artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4}"
FRAME_LIMIT="${FRAME_LIMIT:-300}"
MAX_GUEST_VS_HOST_DECODE_RATIO="${MAX_GUEST_VS_HOST_DECODE_RATIO:-3.0}"
MIN_GUEST_DECODE_REALTIME="${MIN_GUEST_DECODE_REALTIME:-5.0}"
MAX_GUEST_DECODE_CPU_RATIO="${MAX_GUEST_DECODE_CPU_RATIO:-1.5}"

if [ ! -f "${REFERENCE_VIDEO}" ]; then
    echo "ERROR: REFERENCE_VIDEO not found: ${REFERENCE_VIDEO}" >&2
    exit 1
fi

WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/v1-decode-perf-gate-$$}"
export WORKDIR

REFERENCE_VIDEO="${REFERENCE_VIDEO}" \
FRAME_LIMIT="${FRAME_LIMIT}" \
"${REPO_ROOT}/ffmpeg/scripts/benchmark_host_guest_reference_decode.sh"

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
cpu_ratio = summary["value_of_hardware_decode"]["guest_vt_cpu_efficiency_vs_sw"]

max_ratio = float("${MAX_GUEST_VS_HOST_DECODE_RATIO}")
min_realtime = float("${MIN_GUEST_DECODE_REALTIME}")
max_cpu_ratio = float("${MAX_GUEST_DECODE_CPU_RATIO}")

print()
print(f"v1 decode perf gate:")
if ratio is not None:
    print(f"  guest_vs_host_videotoolbox_ratio={ratio:.3f}x (cap {max_ratio:.2f}x)")
print(f"  guest_videotoolbox_x_realtime={realtime:.1f}x (floor {min_realtime:.1f}x)")
if fps is not None:
    print(f"  guest_videotoolbox_fps={fps:.0f}")
if cpu_ratio is not None:
    print(
        f"  guest_vt_cpu_vs_sw_decode={cpu_ratio:.3f} "
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
        f"guest_vt_cpu_vs_sw_decode={cpu_ratio:.3f} exceeds cap {max_cpu_ratio:.2f} "
        f"— hardware decode no longer saves enough CPU vs libavcodec"
    )

if failures:
    print()
    print("v1 decode perf gate: FAIL")
    for f in failures:
        print(f"  {f}")
    sys.exit(1)

print("v1 decode perf gate: pass")
PY
