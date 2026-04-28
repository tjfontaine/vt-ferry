#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"
SYSROOT="${REPO_ROOT}/third_party/smolvm/libkrun/linux-sysroot"
LLVM_PREFIX="${LLVM_PREFIX:-}"
LLD_PREFIX="${LLD_PREFIX:-}"

if [ -z "${LLVM_PREFIX}" ] && command -v brew >/dev/null 2>&1; then
  LLVM_PREFIX="$(brew --prefix llvm)"
fi
if [ -z "${LLD_PREFIX}" ] && command -v brew >/dev/null 2>&1; then
  LLD_PREFIX="$(brew --prefix lld)"
fi
if [ -z "${LLVM_PREFIX}" ]; then
  LLVM_PREFIX="/opt/homebrew/opt/llvm"
fi
if [ -z "${LLD_PREFIX}" ]; then
  LLD_PREFIX="/opt/homebrew/opt/lld"
fi

CLANG_BIN="${CLANG_BIN:-${LLVM_PREFIX}/bin/clang}"
LLD_BIN="${LLD_BIN:-${LLD_PREFIX}/bin/ld.lld}"
GCC_ROOT="${SYSROOT}/usr/lib/gcc/aarch64-linux-gnu"
GCC_VERSION="$(find "${GCC_ROOT}" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort -V | tail -n 1)"
GCC_LIB_DIR="${GCC_ROOT}/${GCC_VERSION}"

[ -d "${SYSROOT}" ] || {
  echo "missing Linux sysroot: ${SYSROOT}" >&2
  exit 1
}
[ -x "${CLANG_BIN}" ] || {
  echo "missing clang at ${CLANG_BIN}" >&2
  exit 1
}
[ -x "${LLD_BIN}" ] || {
  echo "missing ld.lld at ${LLD_BIN}" >&2
  exit 1
}
[ -n "${GCC_VERSION}" ] || {
  echo "missing gcc runtime under ${GCC_ROOT}" >&2
  exit 1
}

exec "${CLANG_BIN}" \
  -target aarch64-linux-gnu \
  --ld-path="${LLD_BIN}" \
  --sysroot "${SYSROOT}" \
  --gcc-toolchain="${SYSROOT}/usr" \
  -B"${GCC_LIB_DIR}" \
  -B"${SYSROOT}/usr/lib/aarch64-linux-gnu" \
  -B"${SYSROOT}/lib/aarch64-linux-gnu" \
  -L"${GCC_LIB_DIR}" \
  -L"${SYSROOT}/usr/lib/aarch64-linux-gnu" \
  -L"${SYSROOT}/lib/aarch64-linux-gnu" \
  -Wno-c23-extensions \
  "$@"
