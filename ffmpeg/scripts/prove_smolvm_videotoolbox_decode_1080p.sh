#!/bin/sh
#
# 1080p variant of the decode proof. Exercises the pool-bound
# decode-output path (`OP_BIND_DECODE_OUTPUT_POOL`) — at 1920×1080
# NV12 the per-frame size (~3 MiB) exceeds the 1.5 MiB inline
# `OP_READ_DECODED_FRAME` budget, so the guest-shim auto-allocates
# and binds an output pool at session create time and the drain
# loop fetches each frame via chunked `OP_READ_BUFFER`.
#
# The driver script (`prove_smolvm_videotoolbox_decode.sh`) already
# parameterizes `FRAME_SIZE` and the launcher's pool spec keys off
# it, so the launcher pre-registers a 1080p NV12 IOSurface pool
# the shim's auto-allocated pool then consumes via the worker's
# zero-copy IOSurface fast path.
#
# Inline path (≤720p) stays exercised by the canonical
# `prove_smolvm_videotoolbox_decode.sh`; this wrapper proves the
# pool-binding extension works end-to-end so 1080p / 4K decode is
# unblocked.
#
# Usage: ffmpeg/scripts/prove_smolvm_videotoolbox_decode_1080p.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"

FRAME_SIZE="${FRAME_SIZE:-1920x1080}" \
SLOT_COUNT="${SLOT_COUNT:-4}" \
VM_NAME="${VM_NAME:-vt-ferry-decode-1080p-$$}" \
WORKDIR="${WORKDIR:-${SCRIPT_DIR}/../../artifacts/decode-1080p-proof-$$}" \
exec "${SCRIPT_DIR}/prove_smolvm_videotoolbox_decode.sh" "$@"
