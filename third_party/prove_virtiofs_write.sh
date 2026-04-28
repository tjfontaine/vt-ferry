#!/bin/bash
# Minimal macOS/libkrun virtiofs repro.
#
# This script intentionally avoids the vt-ferry broker, pool metadata, and mmap
# path. It boots a VM with a single writable virtiofs mount and then checks the
# smallest useful guest operations in order:
#   1. mounted path exists
#   2. plain file read (`head -c 1`)
#   3. plain file write (`printf >file`)
#   4. readback
#
# If step 2 already hangs, the blocker is generic virtiofs file I/O on this
# runtime, not the vt-ferry shared-region path.
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
smolvm_root="$repo_root/third_party/smolvm"

SMOLVM_BIN="${SMOLVM_BIN:-$smolvm_root/target/release/smolvm}"
SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS:-$smolvm_root/target/agent-rootfs}"
VT_FERRY_REPRO_VM_NAME="${VT_FERRY_REPRO_VM_NAME:-vt-ferry-virtiofs-write-$$}"
VT_FERRY_MUSL_TARGET="aarch64-unknown-linux-musl"

tmpdir=""
machine_start_log=""
libkrun_debug_log=""

die() {
    echo "ERROR: $*" >&2
    exit 1
}

maybe_add_e2fsprogs_to_path() {
    if command -v mkfs.ext4 >/dev/null 2>&1; then
        return
    fi

    if command -v brew >/dev/null 2>&1; then
        local prefix
        prefix="$(brew --prefix e2fsprogs 2>/dev/null || true)"
        if [ -n "$prefix" ] && [ -x "$prefix/sbin/mkfs.ext4" ]; then
            export PATH="$prefix/sbin:$prefix/bin:$PATH"
        fi
    fi
}

require_rootfs_tool() {
    local tool="$1"
    if ! find "$SMOLVM_AGENT_ROOTFS" -type f -name "$tool" -perm -u+x | grep -q .; then
        die "agent rootfs is missing '$tool' under $SMOLVM_AGENT_ROOTFS; build a full rootfs with third_party/smolvm/scripts/build-agent-rootfs.sh"
    fi
}

ensure_agent_binary() {
    if [ ! -d "$SMOLVM_AGENT_ROOTFS" ]; then
        die "agent rootfs not found at $SMOLVM_AGENT_ROOTFS"
    fi

    if ! rustup target list --installed | grep -qx "$VT_FERRY_MUSL_TARGET"; then
        die "missing Rust target $VT_FERRY_MUSL_TARGET; run: rustup target add $VT_FERRY_MUSL_TARGET"
    fi

    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
        cargo build \
        --manifest-path "$smolvm_root/Cargo.toml" \
        --profile release-small \
        -p smolvm-agent \
        --target "$VT_FERRY_MUSL_TARGET" >/dev/null

    mkdir -p "$SMOLVM_AGENT_ROOTFS/usr/local/bin" "$SMOLVM_AGENT_ROOTFS/sbin"
    cp \
        "$smolvm_root/target/$VT_FERRY_MUSL_TARGET/release-small/smolvm-agent" \
        "$SMOLVM_AGENT_ROOTFS/usr/local/bin/smolvm-agent"
    chmod +x "$SMOLVM_AGENT_ROOTFS/usr/local/bin/smolvm-agent"
    ln -sf /usr/local/bin/smolvm-agent "$SMOLVM_AGENT_ROOTFS/sbin/init"

    [ -L "$SMOLVM_AGENT_ROOTFS/sbin/init" ] || die "agent rootfs is missing /sbin/init symlink"
    require_rootfs_tool "e2fsck"
    require_rootfs_tool "resize2fs"
}

build_host_binaries() {
    cargo build --manifest-path "$smolvm_root/Cargo.toml" --release --bin smolvm >/dev/null
    if [ "$(uname -s)" = "Darwin" ]; then
        codesign --force --sign - --entitlements "$smolvm_root/smolvm.entitlements" "$smolvm_root/target/release/smolvm" >/dev/null
    fi
    [ -x "$SMOLVM_BIN" ] || die "smolvm binary not found at $SMOLVM_BIN"
}

dump_vm_logs() {
    local data_dir
    data_dir="$(SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" "$SMOLVM_BIN" machine data-dir "$VT_FERRY_REPRO_VM_NAME" 2>/dev/null || true)"
    if [ -n "$machine_start_log" ] && [ -f "$machine_start_log" ]; then
        echo "--- machine-start.log ---" >&2
        cat "$machine_start_log" >&2 || true
    fi
    if [ -n "$libkrun_debug_log" ] && [ -f "$libkrun_debug_log" ]; then
        echo "--- libkrun-debug.log ---" >&2
        cat "$libkrun_debug_log" >&2 || true
    fi
    if [ -n "$data_dir" ] && [ -d "$data_dir" ]; then
        echo "--- machine-data-dir ---" >&2
        find "$data_dir" -maxdepth 1 -type f | sort >&2 || true
        if [ -f "$data_dir/agent-console.log" ]; then
            echo "--- agent-console.log ---" >&2
            tail -n 200 "$data_dir/agent-console.log" >&2 || true
        fi
    fi
    if [ -n "$tmpdir" ] && [ -f "$tmpdir/guest-trace.log" ]; then
        echo "--- guest-trace.log ---" >&2
        cat "$tmpdir/guest-trace.log" >&2 || true
    fi
}

cleanup() {
    local status=$?
    set +e
    if [ $status -ne 0 ]; then
        dump_vm_logs
    fi
    SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" "$SMOLVM_BIN" machine stop --name "$VT_FERRY_REPRO_VM_NAME" >/dev/null 2>&1 || true
    SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" "$SMOLVM_BIN" machine delete "$VT_FERRY_REPRO_VM_NAME" -f >/dev/null 2>&1 || true
    if [ -n "$tmpdir" ] && [ -d "$tmpdir" ]; then
        rm -rf "$tmpdir"
    fi
    exit $status
}

trap cleanup EXIT

main() {
    maybe_add_e2fsprogs_to_path
    command -v mkfs.ext4 >/dev/null 2>&1 || die "mkfs.ext4 not found; install e2fsprogs"

    ensure_agent_binary
    build_host_binaries

    "$repo_root/third_party/prepare_krun_runtime.sh" >/dev/null
    # shellcheck disable=SC1090
    . "$repo_root/artifacts/krun-runtime/macos-arm64/env.sh"

    tmpdir="$(mktemp -d)"
    machine_start_log="$tmpdir/machine-start.log"
    libkrun_debug_log="$tmpdir/libkrun-debug.log"
    local backing_file="$tmpdir/virtiofs.bin"
    local trace_log="$tmpdir/guest-trace.log"

    python3 - <<PY
from pathlib import Path
Path("$backing_file").write_bytes(b"\0" * 4096)
PY

    SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" "$SMOLVM_BIN" machine create \
        "$VT_FERRY_REPRO_VM_NAME" \
        --net \
        -v "$tmpdir:/hosttest" \
        >/dev/null

    RUST_LOG="${RUST_LOG:-debug}" \
    VT_FERRY_LIBKRUN_DEBUG_LOG="$libkrun_debug_log" \
    SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" \
    "$SMOLVM_BIN" machine start --name "$VT_FERRY_REPRO_VM_NAME" >"$machine_start_log" 2>&1

    {
        echo "start"
        SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" "$SMOLVM_BIN" machine exec \
            --name "$VT_FERRY_REPRO_VM_NAME" \
            --timeout 15s \
            -- sh -lc 'test -f /hosttest/virtiofs.bin'
        echo "path-exists"

        SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" "$SMOLVM_BIN" machine exec \
            --name "$VT_FERRY_REPRO_VM_NAME" \
            --timeout 15s \
            -- sh -lc 'head -c 1 /hosttest/virtiofs.bin >/dev/null'
        echo "initial-read-ok"

        SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" "$SMOLVM_BIN" machine exec \
            --name "$VT_FERRY_REPRO_VM_NAME" \
            --timeout 15s \
            -- sh -lc 'printf "\x33" >/hosttest/virtiofs.bin'
        echo "write-ok"

        SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" "$SMOLVM_BIN" machine exec \
            --name "$VT_FERRY_REPRO_VM_NAME" \
            --timeout 15s \
            -- sh -lc 'head -c 1 /hosttest/virtiofs.bin | od -An -tx1'
        echo "readback-ok"
    } >>"$trace_log" 2>&1

    xxd -l 16 "$backing_file"
    echo "virtiofs io repro passed"
}

main "$@"
