# Sourceable helper that lets a smoke / benchmark / gate driver
# pick up either:
#
#   - host-artifact mode (default): the patched ffmpeg + guest
#     shim were built into artifacts/ on the host. Smoke
#     bind-mounts the repo into smolvm via /repo and points the
#     guest at the under-/repo paths.
#
#   - image mode (USE_VT_FERRY_IMAGE=1): the patched ffmpeg + the
#     guest shim are baked into the VM_IMAGE under
#     /opt/vt-ferry/{bin,lib,include}/. ldconfig in the image
#     handles dynamic linker resolution. No host-side build
#     required. build_vt_ferry_image.sh's
#     artifacts/vt-ferry-guest-image/env.sh sets this for you.
#
# Caller pre-sets (subset depending on the script):
#   REPO_ROOT FFMPEG_BIN SHIM_LIBDIR SHIM_TARGET_TRIPLE
#   STAGE_SHIM_LIBS USE_VT_FERRY_IMAGE
#
# Caller must define guest_repo_path() before sourcing (each
# smoke already does — the function maps host paths under
# REPO_ROOT to /repo/* paths inside the VM).

USE_VT_FERRY_IMAGE="${USE_VT_FERRY_IMAGE:-0}"

# Stage the guest shim libs (host-artifact mode only) and verify
# the patched ffmpeg + shim are reachable on the host. No-op in
# image mode where those artifacts live inside the VM_IMAGE.
#
# Usage: smoke_validate_host_artifacts
smoke_validate_host_artifacts() {
  if [ "${USE_VT_FERRY_IMAGE}" = "1" ]; then
    return 0
  fi
  if [ -n "${FFMPEG_BIN:-}" ] && [ ! -e "${FFMPEG_BIN}" ]; then
    echo "ERROR: missing ${FFMPEG_BIN}" >&2
    echo "  Build it with: sh ffmpeg/scripts/build_vt_ferry_image.sh" >&2
    echo "  then source artifacts/vt-ferry-guest-image/env.sh and set USE_VT_FERRY_IMAGE=1." >&2
    exit 1
  fi
  if [ "${STAGE_SHIM_LIBS:-1}" = "1" ] && [ -n "${SHIM_LIBDIR:-}" ]; then
    "${REPO_ROOT}/ffmpeg/scripts/stage_guest_shim_libs.sh" \
      debug \
      "${SHIM_LIBDIR#${REPO_ROOT}/}" \
      "${SHIM_TARGET_TRIPLE:-}"
  fi
  if [ -n "${SHIM_LIBDIR:-}" ] && [ ! -e "${SHIM_LIBDIR}/libguest_shim.so" ]; then
    echo "ERROR: missing ${SHIM_LIBDIR}/libguest_shim.so" >&2
    exit 1
  fi
}

# Resolve the GUEST_FFMPEG_BIN + GUEST_SHIM_LIBDIR variables.
# In host-artifact mode they're computed from FFMPEG_BIN and
# SHIM_LIBDIR via the caller's guest_repo_path() (host → /repo).
# In image mode they're absolute paths inside the VM image.
#
# Usage: smoke_resolve_guest_paths
smoke_resolve_guest_paths() {
  if [ "${USE_VT_FERRY_IMAGE}" = "1" ]; then
    GUEST_FFMPEG_BIN="${GUEST_FFMPEG_BIN:-/opt/vt-ferry/bin/ffmpeg}"
    GUEST_SHIM_LIBDIR="${GUEST_SHIM_LIBDIR:-}"
    return 0
  fi
  GUEST_FFMPEG_BIN="$(guest_repo_path "${FFMPEG_BIN}")" || {
    echo "ERROR: FFMPEG_BIN must be under ${REPO_ROOT}" >&2
    exit 1
  }
  GUEST_SHIM_LIBDIR="$(guest_repo_path "${SHIM_LIBDIR}")" || {
    echo "ERROR: SHIM_LIBDIR must be under ${REPO_ROOT}" >&2
    exit 1
  }
}

# Build the env-prefix string for invoking ffmpeg inside the VM.
# Includes LD_LIBRARY_PATH only when GUEST_SHIM_LIBDIR is set —
# in image mode ldconfig already wired the shim libs, and an
# empty LD_LIBRARY_PATH has surprising side effects on the
# search order.
#
# Usage: env_prefix=$(smoke_guest_env_prefix "$transport_env")
smoke_guest_env_prefix() {
  if [ -n "${GUEST_SHIM_LIBDIR}" ]; then
    printf '%s LD_LIBRARY_PATH=%s' "$1" "${GUEST_SHIM_LIBDIR}"
  else
    printf '%s' "$1"
  fi
}
