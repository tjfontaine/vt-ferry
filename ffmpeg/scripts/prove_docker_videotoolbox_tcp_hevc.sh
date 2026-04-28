#!/bin/sh
#
# HEVC variant of prove_docker_videotoolbox_tcp.sh.
#
# Drives the same Docker / TCP bridge path as the H.264 baseline but
# selects hevc_videotoolbox and asserts the round-trip codec is hevc.
# This closes a coverage gap: HEVC works end-to-end over the smolvm
# vsock path (prove_smolvm_videotoolbox_hevc.sh) but the Docker /
# TCP bridge path was H.264-only — users running vt-ferry under Docker
# Desktop had no smoke for the HEVC encoder.
#
# Usage: ffmpeg/scripts/prove_docker_videotoolbox_tcp_hevc.sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"

DOCKER_GUEST_CODEC="${DOCKER_GUEST_CODEC:-hevc_videotoolbox}" \
DOCKER_EXPECTED_CODEC_NAME="${DOCKER_EXPECTED_CODEC_NAME:-hevc}" \
exec "${SCRIPT_DIR}/prove_docker_videotoolbox_tcp.sh" "$@"
