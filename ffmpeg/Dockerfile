# Multi-stage build for the vt-ferry guest runtime image.
#
# Stage 1 (`builder`) installs the Rust + FFmpeg build toolchain,
# clones upstream FFmpeg at the configured ref, applies the
# vt-ferry patch, builds the guest shim cdylib, and configures +
# builds the patched FFmpeg.
#
# Stage 2 (`runtime`) is a thin Ubuntu-based image carrying only
# the runtime artifacts: the patched `ffmpeg` / `ffprobe` binaries
# and the guest shim shared library (with the libCore* /
# libVideoToolbox symlinks the patched FFmpeg's `-l*` flags
# resolve against). Headers are also copied so downstream consumers
# can build against the same shim ABI.
#
# The image is published as `vt-ferry-guest`. Inside a smolvm /
# libkrun VM whose host runs the vt-ferry host worker, the
# `-hwaccel videotoolbox` and `h264_videotoolbox` / `hevc_videotoolbox`
# code paths in this `ffmpeg` route through to real Apple
# VideoToolbox sessions on the host.

FROM public.ecr.aws/docker/library/ubuntu:24.04 AS builder
ARG DEBIAN_FRONTEND=noninteractive
ARG FFMPEG_REF=n8.1

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        nasm \
        yasm \
        libx264-dev \
        libx265-dev \
        curl \
        ca-certificates \
        git \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --profile minimal
ENV PATH=/root/.cargo/bin:${PATH}

WORKDIR /work
# Clone upstream FFmpeg at the requested ref before copying the
# repo so this layer caches independently of source-tree edits.
RUN mkdir -p /work/artifacts/ffmpeg-source/${FFMPEG_REF} \
    && git clone --depth 1 --branch ${FFMPEG_REF} \
        https://github.com/FFmpeg/FFmpeg.git \
        /work/artifacts/ffmpeg-source/${FFMPEG_REF}/FFmpeg

COPY . /work
RUN sh ffmpeg/scripts/build_inside_container.sh ${FFMPEG_REF}

FROM public.ecr.aws/docker/library/ubuntu:24.04 AS runtime
ARG DEBIAN_FRONTEND=noninteractive
ARG FFMPEG_REF=n8.1

# Runtime deps for the patched FFmpeg (libavformat / libavcodec
# pull these via dynamic linkage). Plus distro `ffmpeg` so smokes
# can run the libavcodec software baseline through `/usr/bin/ffmpeg`
# while the patched build lives at `/opt/vt-ferry/bin/ffmpeg`.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libx264-164 \
        libx265-199 \
        ffmpeg \
    && rm -rf /var/lib/apt/lists/*

# Patched FFmpeg + ffprobe + headers.
COPY --from=builder /work/artifacts/ffmpeg-build/${FFMPEG_REF}-linux-debug/ffmpeg /opt/vt-ferry/bin/ffmpeg
COPY --from=builder /work/artifacts/ffmpeg-build/${FFMPEG_REF}-linux-debug/ffprobe /opt/vt-ferry/bin/ffprobe
COPY --from=builder /work/artifacts/ffmpeg-shim-libs/linux-debug/ /opt/vt-ferry/lib/
COPY --from=builder /work/crates/vt-ferry-shim/include/ /opt/vt-ferry/include/

# Make the shim libs discoverable by the dynamic linker.
RUN echo /opt/vt-ferry/lib > /etc/ld.so.conf.d/vt-ferry.conf && ldconfig

# We deliberately do NOT prepend /opt/vt-ferry/bin to PATH. The
# image carries two ffmpeg builds — the distro one at
# /usr/bin/ffmpeg (libavcodec only) and the patched one at
# /opt/vt-ferry/bin/ffmpeg (libavcodec + the vt-ferry shim path
# that routes -hwaccel videotoolbox / *_videotoolbox encoders to
# real Apple VideoToolbox on the host). Bare `ffmpeg` resolves
# to the distro build (the standard expectation for any tool
# that runs `ffmpeg` from PATH). Code that wants the patched
# build calls /opt/vt-ferry/bin/ffmpeg by absolute path —
# either via $GUEST_FFMPEG_BIN in the smokes, or directly.

LABEL org.opencontainers.image.title="vt-ferry-guest"
LABEL org.opencontainers.image.description="Linux guest runtime for vt-ferry — patched FFmpeg + VideoToolbox shim"
LABEL org.opencontainers.image.source="https://github.com/tjfontaine/vt-ferry"
LABEL org.opencontainers.image.licenses="MIT"
