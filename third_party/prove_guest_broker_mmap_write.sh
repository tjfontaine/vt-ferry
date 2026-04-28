#!/bin/bash
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
smolvm_root="$repo_root/third_party/smolvm"

SMOLVM_BIN="${SMOLVM_BIN:-$smolvm_root/target/release/smolvm}"
SMOLVM_AGENT_ROOTFS="${SMOLVM_AGENT_ROOTFS:-$smolvm_root/target/agent-rootfs}"
VT_FERRY_REPRO_VM_NAME="${VT_FERRY_REPRO_VM_NAME:-vt-ferry-broker-mmap-write-$$}"
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
        if [ -f "$data_dir/agent-startup-error.log" ]; then
            echo "--- agent-startup-error.log ---" >&2
            cat "$data_dir/agent-startup-error.log" >&2 || true
        fi
        if [ -f "$data_dir/agent-console.log" ]; then
            echo "--- agent-console.log ---" >&2
            tail -n 200 "$data_dir/agent-console.log" >&2 || true
        fi
    fi
    if [ -n "$tmpdir" ] && [ -f "$tmpdir/guest-trace.log" ]; then
        echo "--- guest-trace.log ---" >&2
        tail -n 200 "$tmpdir/guest-trace.log" >&2 || true
    fi
    if [ -n "$tmpdir" ] && [ -f "$tmpdir/guest-e2e.log" ]; then
        echo "--- guest-e2e.log ---" >&2
        tail -n 200 "$tmpdir/guest-e2e.log" >&2 || true
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
    local backing_file="$tmpdir/shared.mem"
    local helper_file="$tmpdir/repro.py"
    local helper_b64
    local guest_helper_cmd
    local pool_specs_json
    local gpa=9663676416
    local region_size=49152

    python3 - <<PY
from pathlib import Path
Path("$backing_file").write_bytes(b"\0" * $region_size)
PY

    pool_specs_json="$(python3 - <<PY
import json
print(json.dumps([{
    "guest_phys_addr": $gpa,
    "slot_count": 1,
    "width": 128,
    "height": 256,
    "pixel_format": 875704438,
    "path": "$backing_file",
    "offset": 0,
    "writable": True
}]))
PY
)"

    cat >"$helper_file" <<'PY'
import array
import ctypes
import mmap
import os
import socket

TRACE_PATH = os.environ["VT_FERRY_E2E_TRACE_PATH"]
BROKER_SOCKET = "/tmp/vt-ferry/shared-region-broker.sock"
BROKER_PROTOCOL_VERSION = 1
BROKER_FLAG_REQUIRE_WRITABLE = 1 << 0
BROKER_STATUS_OK = 0
REGION_SIZE = 49152
EXPECTED_SOURCE_PATH = "/mnt/vt-ferry-pools/0/shared.mem"


def progress(message: str) -> None:
    with open(TRACE_PATH, "a", encoding="utf-8") as handle:
        handle.write(message + "\n")


class BrokerRequest(ctypes.Structure):
    _fields_ = [
        ("version", ctypes.c_uint32),
        ("flags", ctypes.c_uint32),
        ("minimum_region_size", ctypes.c_uint64),
        ("source_path", ctypes.c_char * 256),
    ]


class BrokerResponse(ctypes.Structure):
    _fields_ = [
        ("version", ctypes.c_uint32),
        ("status", ctypes.c_uint32),
        ("region_size", ctypes.c_uint64),
        ("source_offset", ctypes.c_uint64),
        ("flags", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32),
    ]


progress("start")
assert os.path.exists(BROKER_SOCKET), BROKER_SOCKET
progress("socket-exists")
assert os.path.exists(EXPECTED_SOURCE_PATH), EXPECTED_SOURCE_PATH
progress("path-exists")

with open(EXPECTED_SOURCE_PATH, "r+b", buffering=0) as direct_file:
    direct_file.write(b"\x24")
    progress("direct-write-ok")
    direct_file.seek(0)
    assert direct_file.read(1) == b"\x24"
    progress("direct-read-ok")

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(BROKER_SOCKET)
progress("broker-connected")
sock.sendall(
    bytes(
        BrokerRequest(
            version=BROKER_PROTOCOL_VERSION,
            flags=BROKER_FLAG_REQUIRE_WRITABLE,
            minimum_region_size=REGION_SIZE,
            source_path=EXPECTED_SOURCE_PATH.encode(),
        )
    )
)
progress("request-sent")
ancbuf = socket.CMSG_SPACE(array.array("i", [0]).itemsize)
data, ancdata, _, _ = sock.recvmsg(ctypes.sizeof(BrokerResponse), ancbuf)
response = BrokerResponse.from_buffer_copy(data)
assert response.status == BROKER_STATUS_OK
progress("response-ok")

fds = array.array("i")
for level, ctype, cdata in ancdata:
    if level == socket.SOL_SOCKET and ctype == socket.SCM_RIGHTS:
        fds.frombytes(cdata[: len(cdata) - (len(cdata) % fds.itemsize)])
assert len(fds) == 1, ancdata
fd = fds[0]
progress("fd-received")

written = os.pwrite(fd, b"\x41", 0)
assert written == 1, written
progress("pwrite-ok")
assert os.pread(fd, 1, 0) == b"\x41"
progress("pread-ok")

mapping = mmap.mmap(
    fd,
    response.region_size,
    flags=mmap.MAP_SHARED,
    prot=mmap.PROT_READ | mmap.PROT_WRITE,
    offset=response.source_offset,
)
progress("mmap-ok")
mapping[0] = 0x5A
progress("write-ok")
PY

    helper_b64="$(base64 <"$helper_file" | tr -d '\n')"
    guest_helper_cmd="$(cat <<EOF
python3 -c 'import base64, pathlib; pathlib.Path("/tmp/vt_ferry_broker_write_repro.py").write_bytes(base64.b64decode("$helper_b64"))'
rc=\$?
if [ \$rc -eq 0 ]; then
  VT_FERRY_E2E_TRACE_PATH=/tmp/guest-trace.log \\
  python3 /tmp/vt_ferry_broker_write_repro.py >/tmp/guest-e2e.log 2>&1
  rc=\$?
fi
[ -f /tmp/guest-trace.log ] && cp /tmp/guest-trace.log /hosttest/guest-trace.log
[ -f /tmp/guest-e2e.log ] && cp /tmp/guest-e2e.log /hosttest/guest-e2e.log
exit \$rc
EOF
)"

    SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" "$SMOLVM_BIN" machine create \
        "$VT_FERRY_REPRO_VM_NAME" \
        --net \
        -v "$tmpdir:/hosttest" \
        >/dev/null

    RUST_LOG="${RUST_LOG:-debug}" \
    VT_FERRY_LIBKRUN_DEBUG_LOG="$libkrun_debug_log" \
    VT_FERRY_POOL_SPECS_JSON="$pool_specs_json" \
    SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" \
    "$SMOLVM_BIN" machine start --name "$VT_FERRY_REPRO_VM_NAME" >"$machine_start_log" 2>&1

    SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" "$SMOLVM_BIN" machine exec \
        --name "$VT_FERRY_REPRO_VM_NAME" \
        -- sh -lc 'command -v python3 >/dev/null 2>&1 || apk add --no-cache python3 >/dev/null'

    SMOLVM_AGENT_ROOTFS="$SMOLVM_AGENT_ROOTFS" "$SMOLVM_BIN" machine exec \
        --name "$VT_FERRY_REPRO_VM_NAME" \
        --timeout 15s \
        -- sh -lc "$guest_helper_cmd"

    xxd -l 16 "$backing_file"
    echo "brokered mmap write repro passed"
}

main "$@"
