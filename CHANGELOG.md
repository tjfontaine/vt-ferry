# Changelog

This project follows [Semantic Versioning](https://semver.org/). Until
the first 1.0 tag the protocol surface and Rust APIs may change
between minor versions; production users should pin a specific tag
and watch this repo for releases.

## 0.1.0 — initial public release

First public release. This is the state of the codebase at the moment
of going public.

### What works (smoke-gated)

- **Hardware encode** through `h264_videotoolbox` and
  `hevc_videotoolbox` (Main profile, 8-bit; Main10 / 10-bit P010 also
  supported).
- **Hardware decode** through `-hwaccel videotoolbox` for H.264 +
  HEVC, with bit-identical output vs host VT (PSNR=∞, SSIM=1.0
  baseline). Decoded NV12 ships inline for ≤720p frames; >720p frames
  drain through a chunked-zero-copy path that holds VT's CVImageBuffer
  on the host and copies stride-aware byte ranges into per-chunk
  responses.
- **Transcode** (VT decode + VT encode in one ffmpeg pipeline) — the
  metric most matched to real workloads. Pipeline parallelism between
  the encode and decode sides hides vsock transport latency.
- **Multi-process concurrent transcode** (multiple ffmpeg processes
  inside the same VM, each on its own worker connection, with isolated
  session ids and independent zero-copy IOSurfaces).
- **Multi-stream encode** (one input → N concurrent VTCompressionSessions
  in the same ffmpeg run; the multistream smoke runs 720p + 480p H.264).
- **Mid-stream format change** (ABR ladder switching) on a single
  VTDecompressionSession through `OP_SET_DECODE_FORMAT` re-issue +
  `VTDecompressionSessionCanAcceptFormatDescription`.
- **Pixel formats**: NV12 (`'420v'` / `'420f'`), BGRA, P010 video
  range (`'x420'`) and full range (`'xf20'`).
- **Containers**: mp4 / mov / mkv / ts.
- **Audio passthrough** (`-c:a copy`) on multi-stream demuxes.
- **VT decode error surfacing**: VT's per-frame OSStatus reaches the
  guest through a sentinel reply on `OP_DEQUEUE_DECODED_FRAME`, so bad
  bitstreams produce real error codes ffmpeg can report instead of
  silent frame drops.
- **Chunked encoded-frame transport** for IDRs >4 MiB
  (`OP_ENQUEUE_ENCODED_FRAME_CHUNK`), forecloses the cap-bump bug
  class for future 8K or grain-heavy 4K content.
- **Docker / TCP-bridge** fallback transport for users not running
  smolvm directly.

### What works (benchmarked, not smoke-gated)

- **Multi-rendition ABR ladder transcode** (one VTDecompressionSession
  + N concurrent VTCompressionSessions) — covered by
  `benchmark_host_guest_reference_transcode_multirendition.sh`.
- **Scale-during-transcode** (4K → 1080p downscale via swscale, then
  VT encode) — covered by
  `benchmark_host_guest_reference_transcode_scale.sh`.

### Performance (1080p H.264, 30fps reference clip)

| workload | guest VT vs host VT | × realtime | guest VT vs software CPU |
|---|---:|---:|---:|
| 1080p H.264 transcode | ~1.66× | 4.1× / ~123 fps | ~26% of libx264 (74% saved) |
| 4K H.264 transcode | ~1.62× | 1.1× / ~33 fps | ~29% of libx264 (71% saved) |
| 1080p HEVC transcode | ~1.60× | 4.1× / ~122 fps | ~3% of libx265 (97% saved) |
| 1080p H.264 encode (alone) | within 2.0× | 5.9× | ~10% of libx264 |

Decode-only and encode-only microbenchmarks (less representative of
real usage; pipeline parallelism can't absorb transport latency)
appear in the README's `## Performance` section. For a real
transcode workload the host worker runs at ~0.3% of host CPU; the
work happens inside the guest VM.

### Test coverage

- 179 in-process tests (sub-second, run via `cargo test --workspace`):
  - 60 unit tests in the `vt-ferry-worker` binary (`vt_real::*` +
    `mock::*` + transport)
  - 91 unit tests in the `vt-ferry-shim` library (CoreFoundation /
    CoreMedia / CoreVideo / VideoToolbox surfaces; AVCC/HVCC
    config-record parser)
  - 25 unit tests in the `vt-ferry-protocol` library (struct stability,
    opcode uniqueness, cap floors, status code disjointness)
  - 3 integration tests:
    - `failure_injection_soak` (worker; 20 cycles × 4 failure modes)
    - `connection_isolation` (worker; per-connection session-id namespace)
    - `supervision` (broker; TCP-mode worker death → child kill)
- 14 end-to-end smolvm-side smokes covering encode / decode / HEVC /
  P010 / filtergraph / containers / audio passthrough / multistream /
  1080p+4K decode / transcode / concurrent transcode / ABR mid-stream
  format change
- 2 Docker/TCP smokes for users not running smolvm directly
- 8 codified perf gates (1080p+4K encode; 1080p H.264+HEVC+4K decode;
  1080p+4K H.264 + 1080p HEVC transcode)
- 3 fidelity gates (encode PSNR/SSIM; H.264 + HEVC decode bit-identical)
- 7 reference benchmarks (no thresholds — produce summary.json)

### Known limits

- Apple Silicon macOS hosts only. No Intel-Mac host support.
- 4K maximum. 8K is intentionally not in scope for v1; the chunked
  encoded-frame transport landed today is the foundation for adding
  it later.
- FFmpeg is the only validated guest application. Other applications
  may work if they use the Apple C ABI in the same way FFmpeg does
  but aren't tested.
- HEVC config records that embed multiple VPS/SPS/PPS arrays are
  parsed, but only the first of each is shipped to the worker. Real
  streams almost never carry alternates here; mid-stream parameter
  changes go through `OP_SET_DECODE_FORMAT` re-issue instead.
- `TASK_PORT_REGISTER_MAX = 3` caps the number of IOSurface pool
  entries the launcher can pre-register. Multi-process concurrent
  transcode that needs more (e.g. two processes × VT-decode +
  VT-encode = 4 entries) drops `-hwaccel videotoolbox` on each
  process and uses libavcodec decode + VT encode instead, fitting in
  2 entries.

### Prerequisites

See the `## Getting started` section in `README.md`. Briefly:
Apple Silicon macOS, Rust toolchain, Xcode command-line tools,
`ffmpeg` on the host, the `third_party/smolvm` submodule (Apache
2.0; fetched from upstream).
