#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)

bundle_root="$repo_root/artifacts/krun-runtime/macos-arm64"
bundle_lib="$bundle_root/lib"
env_file="$bundle_root/env.sh"

if command -v brew >/dev/null 2>&1; then
    homebrew_prefix=$(brew --prefix)
else
    homebrew_prefix=/opt/homebrew
fi

homebrew_libkrun_dir="$homebrew_prefix/opt/libkrun/lib"
homebrew_libkrunfw_dir="$homebrew_prefix/opt/libkrunfw/lib"
homebrew_libkrunfw="$homebrew_libkrunfw_dir/libkrunfw.5.dylib"
homebrew_libkrun_versioned=""
if [ -d "$homebrew_libkrun_dir" ]; then
    homebrew_libkrun_versioned=$(find "$homebrew_libkrun_dir" -maxdepth 1 -name 'libkrun.*.dylib' ! -name 'libkrun.dylib' ! -name 'libkrun.1.dylib' | head -n 1)
fi

# Default to Homebrew libkrun. The vendored submodule under
# third_party/smolvm/libkrun is a smol-machines fork; building it locally
# pulled in a regression that crashed the agent fork on macOS 26.5
# (SIGSEGV in libsystem_c's child-side fork pre-exec hook). Homebrew's
# slp/krun bottle is the upstream containers/libkrun build and works.
# Set VT_FERRY_LIBKRUN_SOURCE=local to opt back into the submodule build
# (you'll also want to wire up libclang via DYLD_FALLBACK_LIBRARY_PATH).
libkrun_source_mode="${VT_FERRY_LIBKRUN_SOURCE:-homebrew}"

case "$libkrun_source_mode" in
    homebrew)
        if [ -z "$homebrew_libkrun_versioned" ] || [ ! -f "$homebrew_libkrun_versioned" ]; then
            echo "missing Homebrew libkrun under: $homebrew_libkrun_dir" >&2
            echo "install it with: brew tap slp/krun && brew install libkrun" >&2
            exit 1
        fi
        krun_versioned="$homebrew_libkrun_versioned"
        ;;
    local)
        local_krun_dir="$repo_root/third_party/smolvm/libkrun/target/release"
        local_krun_lib="$local_krun_dir/libkrun.dylib"
        local_krun_versioned=$(find "$local_krun_dir" -maxdepth 1 -name 'libkrun.*.dylib' ! -name 'libkrun.dylib' ! -name 'libkrun.1.dylib' | head -n 1)
        if [ ! -f "$local_krun_lib" ]; then
            echo "missing local libkrun build: $local_krun_lib" >&2
            echo "build it first from third_party/smolvm/libkrun" >&2
            exit 1
        fi
        if [ -z "$local_krun_versioned" ] || [ ! -f "$local_krun_versioned" ]; then
            echo "missing local versioned libkrun dylib under: $local_krun_dir" >&2
            exit 1
        fi
        krun_versioned="$local_krun_versioned"
        ;;
    *)
        echo "unknown VT_FERRY_LIBKRUN_SOURCE: $libkrun_source_mode (use 'homebrew' or 'local')" >&2
        exit 1
        ;;
esac

if [ ! -f "$homebrew_libkrunfw" ]; then
    echo "missing Homebrew libkrunfw: $homebrew_libkrunfw" >&2
    echo "install it with: brew tap slp/krun && brew install libkrunfw" >&2
    exit 1
fi
krunfw_source="$homebrew_libkrunfw"

mkdir -p "$bundle_lib"

# Clean stale libkrun symlinks from prior runs that might have used a
# different versioned dylib.
find "$bundle_lib" -maxdepth 1 -name 'libkrun.*.dylib' -type l -delete 2>/dev/null || true

ln -sf "$krun_versioned" "$bundle_lib/$(basename "$krun_versioned")"
ln -sf "$bundle_lib/$(basename "$krun_versioned")" "$bundle_lib/libkrun.1.dylib"
ln -sf "$bundle_lib/libkrun.1.dylib" "$bundle_lib/libkrun.dylib"

ln -sf "$krunfw_source" "$bundle_lib/libkrunfw.5.dylib"
ln -sf "$bundle_lib/libkrunfw.5.dylib" "$bundle_lib/libkrunfw.dylib"

cat >"$env_file" <<EOF
#!/bin/sh
export LIBKRUN_BUNDLE="$bundle_lib"
export LIBKRUN_DIR="$bundle_lib"
export SMOLVM_LIB_DIR="$bundle_lib"
export DYLD_LIBRARY_PATH="$bundle_lib\${DYLD_LIBRARY_PATH:+:\$DYLD_LIBRARY_PATH}"
# Required for zero-copy IOSurface pools: smolvm's fork-child allocates
# IOSurfaces after fork. Apple's ObjC fork-safety guard would abort the
# child on first class init post-fork unless this flag is present in the
# environment before dyld loads libobjc (libobjc caches the flag at image
# load, so setenv from inside main() is too late).
export OBJC_DISABLE_INITIALIZE_FORK_SAFETY=YES
EOF
chmod +x "$env_file"

# Re-sign smolvm with the Hypervisor entitlement if the binary exists.
# `cargo build --release --bin smolvm` strips the codesignature and with it
# the com.apple.security.hypervisor entitlement declared in
# smolvm.entitlements. Without that entitlement, hv_vm_create returns
# HV_DENIED (0xfae94007) and krun_start_enter fails with -22
# ("agent process exited during startup"). Run this script AFTER any
# cargo rebuild of smolvm to restore the entitlement.
smolvm_bin="$repo_root/third_party/smolvm/target/release/smolvm"
smolvm_entitlements="$repo_root/third_party/smolvm/smolvm.entitlements"
if [ -f "$smolvm_bin" ] && [ -f "$smolvm_entitlements" ]; then
    codesign --force --sign - --entitlements "$smolvm_entitlements" "$smolvm_bin" \
        >/dev/null 2>&1 \
        || printf 'warning: failed to codesign %s with %s\n' "$smolvm_bin" "$smolvm_entitlements" >&2
    printf 'Signed smolvm with Hypervisor entitlement\n'
fi

printf 'Prepared runtime bundle at %s\n' "$bundle_root"
printf 'Source %s before building or running smolvm against libkrun.\n' "$env_file"
printf 'Using libkrun (%s) from %s\n' "$libkrun_source_mode" "$krun_versioned"
printf 'Using libkrunfw from %s\n' "$krunfw_source"
