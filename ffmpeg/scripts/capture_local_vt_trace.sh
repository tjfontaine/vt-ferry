#!/bin/sh
set -eu

OUT_DIR="${1:-artifacts/ffmpeg-trace}"
FFMPEG_BIN="${FFMPEG_BIN:-$(command -v ffmpeg)}"

if [ -z "${FFMPEG_BIN}" ]; then
  echo "ffmpeg not found on PATH" >&2
  exit 1
fi

mkdir -p "${OUT_DIR}"

LIBAVCODEC="$(otool -L "${FFMPEG_BIN}" | awk '/libavcodec\./ { print $1; exit }')"

if [ -z "${LIBAVCODEC}" ]; then
  echo "could not resolve libavcodec from ${FFMPEG_BIN}" >&2
  exit 1
fi

ffmpeg -version > "${OUT_DIR}/ffmpeg-version.txt"
ffmpeg -hide_banner -encoders > "${OUT_DIR}/encoders.txt"
ffmpeg -hide_banner -h encoder=h264_videotoolbox > "${OUT_DIR}/h264_videotoolbox.txt"
ffmpeg -hide_banner -h encoder=hevc_videotoolbox > "${OUT_DIR}/hevc_videotoolbox.txt"
otool -L "${FFMPEG_BIN}" > "${OUT_DIR}/ffmpeg-otool-L.txt"
nm -m "${LIBAVCODEC}" > "${OUT_DIR}/libavcodec-nm-m.txt"
nm -m "${LIBAVCODEC}" | rg "(external|undefined).*(VT|CV|CM|CF)[A-Za-z0-9_]+" > "${OUT_DIR}/apple-media-symbols.txt"
strings -a "${LIBAVCODEC}" | rg "VTCompressionSession|CVPixelBuffer|CMSampleBuffer|CFDictionary|kVT|kCVPixelBuffer|kCM" > "${OUT_DIR}/apple-media-strings.txt"

echo "Wrote FFmpeg VideoToolbox trace files to ${OUT_DIR}"

