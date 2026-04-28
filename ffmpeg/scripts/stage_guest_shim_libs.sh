#!/bin/sh
set -eu

PROFILE="${1:-debug}"
OUT_DIR="${2:-artifacts/ffmpeg-shim-libs/${PROFILE}}"
TARGET_TRIPLE="${3:-${SHIM_TARGET_TRIPLE:-aarch64-unknown-linux-gnu}}"

# Accept "native" as a sentinel meaning "build for the host triple
# without --target". Useful when this script runs inside an aarch64
# Linux container (Docker-based ffmpeg build) where the cross-compile
# linker wrapper would point at macOS-only Homebrew paths.
case "${TARGET_TRIPLE}" in
  native|host)
    TARGET_TRIPLE=""
    ;;
esac

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

detect_lib_ext() {
  case "${1:-}" in
    *-apple-darwin*|darwin)
      echo "dylib"
      ;;
    *-linux-*|linux)
      echo "so"
      ;;
    "")
      case "$(uname -s)" in
        Darwin)
          echo "dylib"
          ;;
        Linux)
          echo "so"
          ;;
        *)
          return 1
          ;;
      esac
      ;;
    *)
      return 1
      ;;
  esac
}

if ! LIB_EXT="$(detect_lib_ext "${TARGET_TRIPLE}")"; then
  echo "unsupported target OS for target '${TARGET_TRIPLE:-$(uname -s)}'" >&2
  exit 1
fi

case "${PROFILE}" in
  debug)
    cargo_args=""
    ;;
  release)
    cargo_args="--release"
    ;;
  *)
    echo "unsupported profile: ${PROFILE}" >&2
    exit 1
    ;;
esac

target_flag=""
build_dir_suffix="${PROFILE}"
if [ -n "${TARGET_TRIPLE}" ]; then
  target_flag="--target ${TARGET_TRIPLE}"
  build_dir_suffix="${TARGET_TRIPLE}/${PROFILE}"
  case "${TARGET_TRIPLE}" in
    aarch64-unknown-linux-gnu)
      : "${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER:=${REPO_ROOT}/ffmpeg/scripts/aarch64-linux-gnu-clang.sh}"
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER
      : "${CC_aarch64_unknown_linux_gnu:=${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER}}"
      export CC_aarch64_unknown_linux_gnu
      ;;
    aarch64-unknown-linux-musl)
      : "${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER:=rust-lld}"
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER
      ;;
  esac
fi

case "${TARGET_TRIPLE:-$(uname -s)}" in
  *-apple-darwin*|Darwin)
    LIB_EXT="dylib"
    ;;
  *-linux-*|Linux)
    LIB_EXT="so"
    ;;
  *)
    echo "unsupported host OS or target: ${TARGET_TRIPLE:-$(uname -s)}" >&2
    exit 1
    ;;
esac

if [ -n "${cargo_args}" ] && [ -n "${target_flag}" ]; then
  cargo build -p vt-ferry-shim ${cargo_args} ${target_flag}
elif [ -n "${cargo_args}" ]; then
  cargo build -p vt-ferry-shim ${cargo_args}
elif [ -n "${target_flag}" ]; then
  cargo build -p vt-ferry-shim ${target_flag}
else
  cargo build -p vt-ferry-shim
fi

BUILD_DIR="${REPO_ROOT}/target/${build_dir_suffix}"
SOURCE_LIB="${BUILD_DIR}/libguest_shim.${LIB_EXT}"

if [ ! -f "${SOURCE_LIB}" ]; then
  echo "guest shim library not found: ${SOURCE_LIB}" >&2
  exit 1
fi

mkdir -p "${REPO_ROOT}/${OUT_DIR}"
cp "${SOURCE_LIB}" "${REPO_ROOT}/${OUT_DIR}/libguest_shim.${LIB_EXT}"

for framework in CoreFoundation CoreMedia CoreVideo VideoToolbox; do
  ln -sf "libguest_shim.${LIB_EXT}" "${REPO_ROOT}/${OUT_DIR}/lib${framework}.${LIB_EXT}"
done

cat <<EOF
Staged guest shim compatibility libraries in:
  ${REPO_ROOT}/${OUT_DIR}

Headers:
  ${REPO_ROOT}/crates/vt-ferry-shim/include

Target:
  ${TARGET_TRIPLE:-native}

EOF
