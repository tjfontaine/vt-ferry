#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

IMAGE_NAME="${IMAGE_NAME:-vt-ferry-vt-bench}"
IMAGE_TAG="${IMAGE_TAG:-ubuntu-24.04-arm64}"
REGISTRY_NAME="${REGISTRY_NAME:-vt-ferry-vt-bench-registry}"
REGISTRY_PORT="${REGISTRY_PORT:-5005}"
PLATFORM="${PLATFORM:-linux/arm64}"
OUT_DIR="${OUT_DIR:-${REPO_ROOT}/artifacts/vt-ferry-vt-bench-image}"

case "${OUT_DIR}" in
  /*) ;;
  *) OUT_DIR="${REPO_ROOT}/${OUT_DIR}" ;;
esac

die() {
  echo "ERROR: $*" >&2
  exit 1
}

command -v docker >/dev/null 2>&1 || die "docker is required to build the prepared smolvm image"
docker info >/dev/null 2>&1 || die "docker is installed, but the Docker daemon is not reachable"

mkdir -p "${OUT_DIR}"
dockerfile="${OUT_DIR}/Dockerfile"
local_tag="${IMAGE_NAME}:${IMAGE_TAG}"
registry_image="localhost:${REGISTRY_PORT}/${IMAGE_NAME}:${IMAGE_TAG}"

cat >"${dockerfile}" <<'DOCKERFILE'
FROM public.ecr.aws/docker/library/ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        ffmpeg \
        libxau6 \
        libxcb-shm0 \
        libxcb1 \
        libxdmcp6 \
        openssl \
        time \
    && rm -rf /var/lib/apt/lists/*
DOCKERFILE

if ! docker ps --format '{{.Names}}' | grep -qx "${REGISTRY_NAME}"; then
  if docker ps -a --format '{{.Names}}' | grep -qx "${REGISTRY_NAME}"; then
    docker start "${REGISTRY_NAME}" >/dev/null
  else
    docker run -d \
      --restart unless-stopped \
      -p "${REGISTRY_PORT}:5000" \
      --name "${REGISTRY_NAME}" \
      registry:2 >/dev/null
  fi
fi

docker build --platform "${PLATFORM}" -t "${local_tag}" -f "${dockerfile}" "${OUT_DIR}"
docker tag "${local_tag}" "${registry_image}"
docker push "${registry_image}"

cat >"${OUT_DIR}/env.sh" <<EOF
# Source this before vt-ferry FFmpeg guest benchmark/profiling scripts.
export VM_IMAGE="${registry_image}"
export INSTALL_GUEST_PACKAGES=0
EOF

cat >"${OUT_DIR}/README.txt" <<EOF
Prepared vt-ferry VideoToolbox benchmark image:

  ${registry_image}

Use it with:

  . ${OUT_DIR}/env.sh
  sh ffmpeg/scripts/compare_uio_vs_vsock_videotoolbox.sh

The local registry container is:

  ${REGISTRY_NAME}
EOF

echo "prepared image: ${registry_image}"
echo "env: ${OUT_DIR}/env.sh"
