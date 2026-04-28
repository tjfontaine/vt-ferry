#!/bin/sh
#
# 4K (3840x2160) variant of the decode proof. Sister to
# `prove_smolvm_videotoolbox_decode_1080p.sh` — same pool-bound
# wire path, just bigger slots:
#
#   per-frame NV12: 3840 × 2160 × 1.5 = 12,441,600 bytes (~11.9 MiB)
#   24 frames:      298,598,400 bytes (~284.8 MiB)
#
# Each frame's slot is read from the worker via chunked
# `OP_READ_BUFFER` (chunk size = 1 MiB / `WRITE_BUFFER_CHUNK_BYTES`),
# so a 4K frame takes ~12 round trips to drain. The slot count
# stays at 4 by default — VT decode produces frames roughly in
# order, so a small ring is enough to keep the pipeline moving.
# Bump `SLOT_COUNT` if your workload reorders frames more
# aggressively (B-frames, ABR ladder switches).
#
# This wrapper is a smoke, not a gate — it validates only that
# the byte count matches expected NV12 layout × frame count. For
# perf or fidelity assertions, see `v1_decode_perf_gate.sh` and
# `long_decode_fidelity.sh`.
#
# We force `HOST_BITRATE=60M` + `HOST_PRESET=ultrafast` to match
# the perf gate's reference clip. With default rate-control on
# testsrc2 the IDRs are tiny (sub-32 KiB even at 4K); at 60 Mbps
# ultrafast they exceed 256 KiB. That makes this smoke catch a
# future regression of `VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES`
# below ~300 KiB. The cap was originally 256 KiB and tripped on
# 343 KiB IDRs from libx264 ultrafast at 60 Mbps; it's now 4 MiB.
# Without these flags the testsrc2 default rate-control would
# produce sub-32 KiB IDRs and the smoke would silently fail to
# exercise the cap path.
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_decode_4k.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"

FRAME_SIZE="${FRAME_SIZE:-3840x2160}" \
SLOT_COUNT="${SLOT_COUNT:-4}" \
VM_NAME="${VM_NAME:-vt-ferry-decode-4k-$$}" \
WORKDIR="${WORKDIR:-${SCRIPT_DIR}/../../artifacts/decode-4k-proof-$$}" \
HOST_PRESET="${HOST_PRESET:-ultrafast}" \
HOST_BITRATE="${HOST_BITRATE:-60M}" \
exec "${SCRIPT_DIR}/prove_smolvm_videotoolbox_decode.sh" "$@"
