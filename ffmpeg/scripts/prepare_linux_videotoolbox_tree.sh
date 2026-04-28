#!/bin/sh
set -eu

REF="${1:-n8.1}"
TREE_DIR="${2:-artifacts/ffmpeg-source/${REF}/FFmpeg}"

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"
PATCH_DIR="${REPO_ROOT}/ffmpeg/patches"
SHIM_INCLUDE="${REPO_ROOT}/crates/vt-ferry-shim/include"
SHIM_LIBDIR="${REPO_ROOT}/artifacts/ffmpeg-shim-libs/linux-debug"
SHIM_TARGET_TRIPLE="${SHIM_TARGET_TRIPLE:-}"

if [ ! -d "${TREE_DIR}/.git" ]; then
  echo "ffmpeg tree not found: ${TREE_DIR}" >&2
  exit 1
fi

for patch in "${PATCH_DIR}"/*.patch; do
  if git -C "${TREE_DIR}" apply --reverse --check "${patch}" >/dev/null 2>&1; then
    echo "already applied: $(basename "${patch}")"
    continue
  fi

  git -C "${TREE_DIR}" apply --check "${patch}"
  git -C "${TREE_DIR}" apply "${patch}"
  echo "applied: $(basename "${patch}")"
done

cat <<EOF

Suggested configure command:
  cd "${TREE_DIR}"
  "${REPO_ROOT}/ffmpeg/scripts/stage_guest_shim_libs.sh" debug "${SHIM_LIBDIR#${REPO_ROOT}/}" "${SHIM_TARGET_TRIPLE}"
  ./configure \\
    --enable-vt-ferry-videotoolbox-linux \\
    --enable-videotoolbox \\
    --extra-cflags="-I${SHIM_INCLUDE}" \\
    --extra-ldflags="-L${SHIM_LIBDIR}" \\
    --extra-libs="-lVideoToolbox -lCoreVideo -lCoreMedia -lCoreFoundation"

Runtime library path inside the guest or test env should include:
  ${SHIM_LIBDIR}

EOF
