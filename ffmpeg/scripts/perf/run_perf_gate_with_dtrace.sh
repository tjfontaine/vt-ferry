#!/bin/bash
#
# Run a vt-ferry perf gate (or any other gate / smoke) under a
# progenyof()-scoped dtrace audit. Captures per-execname syscall
# counts, write/read byte volume, and ustack samples across the
# whole process tree (broker + smolvm + vt-ferry-worker + helpers).
#
# Why progenyof: the gate scripts spawn many processes (broker
# forks the worker; smolvm forks libkrun threads; etc.) and the
# pid provider only attaches to one PID. progenyof($GATE_PID)
# lets the syscall and profile providers see everything in the
# gate's process tree, regardless of restarts.
#
# What you get:
#   - top syscalls by execname during the gate
#   - byte volume on read/write/sendto/recvfrom paths
#   - top user stacks per execname (sampled at 997Hz)
#
# Critical prereq for readable smolvm stacks: rebuild smolvm with
# debug symbols (the vendored release profile sets strip = true).
# One-shot cargo override:
#
#   cd third_party/smolvm && \
#     CARGO_PROFILE_RELEASE_DEBUG=true \
#     CARGO_PROFILE_RELEASE_STRIP=none \
#     cargo build --release --bin smolvm
#
# Usage:
#   ffmpeg/scripts/perf/run_perf_gate_with_dtrace.sh \
#     ffmpeg/scripts/v1_decode_perf_gate_4k.sh
#
# Output:
#   /tmp/perfgate.out    — gate stdout/stderr
#   /tmp/dtrace_out.txt  — dtrace audit summary
#
# Notes:
#   - sudo dtrace is expected to be nopasswd
#   - the audit script self-terminates after 150s (tick-150s),
#     so it never leaks running dtrace processes even if the gate
#     hangs
#   - dtrace's pid provider is intentionally NOT used here. It
#     would require a stable PID and dies if the worker respawns.
#     For per-opcode dispatch attribution you need a separate
#     pid-provider script attached to a known-fresh worker PID.

set -eu

GATE="${1:?usage: $0 path/to/gate-or-smoke.sh}"
shift

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
AUDIT_SCRIPT="${SCRIPT_DIR}/probe_audit.d"
[ -f "${AUDIT_SCRIPT}" ] || { echo "missing: ${AUDIT_SCRIPT}" >&2; exit 1; }

# Pre-flight: kill any zombie workers from prior failed runs so
# pgrep -n actually finds the fresh one.
pkill -f "target/debug/vt-ferry-worker" 2>/dev/null || true
sleep 1

rm -f /tmp/perfgate.out /tmp/dtrace_out.txt

# Run the gate in background, capturing its PID for progenyof scoping.
VT_FERRY_DTRACE_PROBES=1 "${GATE}" "$@" > /tmp/perfgate.out 2>&1 &
GATE_PID=$!
echo "gate pid: ${GATE_PID}"

# Give the gate a moment to start spawning child processes.
sleep 2

# Attach dtrace, scoped to the gate's progeny tree.
sudo dtrace -s "${AUDIT_SCRIPT}" "${GATE_PID}" > /tmp/dtrace_out.txt 2>&1 &
DPID=$!
echo "dtrace started (will self-terminate via tick-150s)"

wait "${GATE_PID}" || true
echo "gate finished"

# dtrace exits on its own via tick-150s.
wait "${DPID}" 2>/dev/null || true

echo ""
echo "=== gate output (tail) ==="
tail -8 /tmp/perfgate.out
echo ""
echo "=== dtrace output: see /tmp/dtrace_out.txt ==="
echo "syscall + cpu summary:"
awk '/=== CPU sample counts/,/=== top user stacks/' /tmp/dtrace_out.txt
