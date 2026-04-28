#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
libkrun_root="$repo_root/third_party/smolvm/libkrun"
sysroot="$libkrun_root/linux-sysroot"
llvm_prefix="${LLVM_PREFIX:-}"

"$script_dir/prepare_krun_runtime.sh"
. "$repo_root/artifacts/krun-runtime/macos-arm64/env.sh"

if [ -z "$llvm_prefix" ] && command -v brew >/dev/null 2>&1; then
    llvm_prefix=$(brew --prefix llvm)
fi
if [ -z "$llvm_prefix" ]; then
    llvm_prefix=/opt/homebrew/opt/llvm
fi

libclang="$llvm_prefix/lib/libclang.dylib"
if [ ! -f "$libclang" ]; then
    echo "missing libclang.dylib at $libclang" >&2
    echo "install llvm with: brew install lld llvm" >&2
    exit 1
fi

mkdir -p "$libkrun_root/target/debug/deps" "$libkrun_root/target/release/deps"
ln -sf "$libclang" "$libkrun_root/target/debug/libclang.dylib"
ln -sf "$libclang" "$libkrun_root/target/debug/deps/libclang.dylib"
ln -sf "$libclang" "$libkrun_root/target/release/libclang.dylib"
ln -sf "$libclang" "$libkrun_root/target/release/deps/libclang.dylib"

export LIBCLANG_PATH="$llvm_prefix/lib"
export DYLD_LIBRARY_PATH="$LIBCLANG_PATH:$DYLD_LIBRARY_PATH"

lld_prefix=$(brew --prefix lld 2>/dev/null || echo "/opt/homebrew/opt/lld")
export CC_LINUX="$llvm_prefix/bin/clang -target arm64-linux-gnu --ld-path=$lld_prefix/bin/ld.lld -Wl,-strip-debug --sysroot $sysroot --gcc-toolchain=$sysroot/usr -B$sysroot/usr/lib/gcc/aarch64-linux-gnu/12 -B$sysroot/usr/lib/aarch64-linux-gnu -L$sysroot/usr/lib/gcc/aarch64-linux-gnu/12 -Wno-c23-extensions"

cargo test --manifest-path "$repo_root/third_party/smolvm/Cargo.toml" test_boot_config_shared_regions_round_trip --lib
cargo test --manifest-path "$repo_root/third_party/smolvm/Cargo.toml" test_build_host_path_shared_region --lib
cargo test --manifest-path "$repo_root/third_party/smolvm/Cargo.toml" test_build_vt_ferry_pool_host_path_shared_region --lib
cargo test --manifest-path "$repo_root/third_party/smolvm/Cargo.toml" test_derive_guest_visible_shared_regions --lib
cargo test --manifest-path "$repo_root/third_party/smolvm/Cargo.toml" test_derive_guest_visible_vt_ferry_pool_specs --lib
cargo test --manifest-path "$repo_root/third_party/smolvm/Cargo.toml" test_plan_vt_ferry_launch_shared_regions_keeps_pool_specs_out_of_mapped_regions --lib
cargo test --manifest-path "$repo_root/third_party/smolvm/Cargo.toml" test_load_shared_regions_from_env_loads_pool_specs_json --lib
cargo test --manifest-path "$repo_root/third_party/smolvm/Cargo.toml" test_rewrite_guest_visible_vt_ferry_pool_specs --lib
cargo test --manifest-path "$libkrun_root/src/libkrun/Cargo.toml" test_krun_add_shared_region2_records_context_config
cargo check --manifest-path "$libkrun_root/src/vmm/Cargo.toml" --lib
