#!/bin/sh
set -eu

REF="${1:-n8.1}"
OUT_DIR="${2:-artifacts/ffmpeg-source-trace/${REF}}"
CHECKOUT_DIR="${3:-artifacts/ffmpeg-source/${REF}}"
REPO_URL="${FFMPEG_REPO_URL:-https://github.com/FFmpeg/FFmpeg.git}"
SRC_DIR="${CHECKOUT_DIR}/FFmpeg"

mkdir -p "${OUT_DIR}" "${CHECKOUT_DIR}"

if [ ! -d "${SRC_DIR}/.git" ]; then
  git clone --depth 1 --branch "${REF}" "${REPO_URL}" "${SRC_DIR}"
else
  git -C "${SRC_DIR}" fetch --depth 1 origin "${REF}"
  git -C "${SRC_DIR}" checkout --force FETCH_HEAD
fi

COMMIT="$(git -C "${SRC_DIR}" rev-parse HEAD)"

{
  echo "repo_url=${REPO_URL}"
  echo "ref=${REF}"
  echo "commit=${COMMIT}"
  echo "source_dir=${SRC_DIR}"
} > "${OUT_DIR}/trace-meta.txt"

rg --files "${SRC_DIR}" \
  | rg '(/configure$|/libavcodec/.*videotoolbox.*\.(c|h)$|/libavutil/hwcontext_videotoolbox\.(c|h)$)' \
  | sort \
  > "${OUT_DIR}/source-files.txt"

rg --files "${SRC_DIR}" \
  | rg '(/libavcodec/.*videotoolbox.*\.(c|h)$|/libavutil/hwcontext_videotoolbox\.(c|h)$)' \
  | sort \
  > "${OUT_DIR}/code-files.txt"

rg --files "${SRC_DIR}" \
  | rg '(/libavcodec/.*videotoolbox.*\.c$|/libavutil/hwcontext_videotoolbox\.c$)' \
  | sort \
  > "${OUT_DIR}/impl-files.txt"

if [ -f "${SRC_DIR}/configure" ]; then
  rg -n -C 4 'videotoolbox' "${SRC_DIR}/configure" > "${OUT_DIR}/configure-videotoolbox.txt" || true
fi

if [ -s "${OUT_DIR}/impl-files.txt" ]; then
  xargs rg -n --no-heading '\b(?:VT|CV|CM|CF)[A-Za-z0-9_]+\s*\(' \
    < "${OUT_DIR}/impl-files.txt" \
    > "${OUT_DIR}/apple-media-calls.txt" || true

  xargs rg --no-filename -o '\b(?:VT|CV|CM|CF)[A-Za-z0-9_]+' \
    < "${OUT_DIR}/impl-files.txt" \
    | sort -u \
    > "${OUT_DIR}/apple-media-call-symbols.txt" || true
fi

if [ -s "${OUT_DIR}/code-files.txt" ]; then
  xargs rg --no-filename -o '\bk(?:VT|CV|CM|CF)[A-Za-z0-9_]+' \
    < "${OUT_DIR}/code-files.txt" \
    | sort -u \
    > "${OUT_DIR}/apple-media-constants.txt" || true
fi

echo "Wrote FFmpeg source trace to ${OUT_DIR}"
