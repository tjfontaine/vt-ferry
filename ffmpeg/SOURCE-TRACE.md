# Source Trace

This document covers the **source-level** tracing workflow for FFmpeg's `videotoolbox` integration.

The local binary trace in [LOCAL-TRACE.md](./LOCAL-TRACE.md) tells us what the built FFmpeg dylib imports from Apple's media frameworks. This source trace complements that by showing:

- which FFmpeg source files participate in the `videotoolbox` path
- which Apple framework calls appear directly in those files
- which framework constants and property keys are referenced in source
- where `configure` gates or detects `videotoolbox`

## Why This Exists

The guest ABI target should not be driven only by intuition or broad framework documentation. It should be bounded by:

1. what FFmpeg source actually references
2. what the built FFmpeg binary actually imports
3. what the targeted workload actually executes

This source trace addresses item 1.

## Script

Use:

```bash
sh ffmpeg/scripts/trace_videotoolbox_source.sh n8.1
```

Arguments:

- first argument: FFmpeg ref or tag, default `n8.1`
- second argument: output directory, default `artifacts/ffmpeg-source-trace/<ref>`
- third argument: checkout directory, default `artifacts/ffmpeg-source/<ref>`

Environment:

- `FFMPEG_REPO_URL` overrides the default upstream source URL

## Generated Artifacts

The script writes:

- `source-files.txt`: traced FFmpeg files relevant to the `videotoolbox` path
- `configure-videotoolbox.txt`: `configure` excerpts mentioning `videotoolbox`
- `apple-media-calls.txt`: source-level `VT*`, `CV*`, `CM*`, and `CF*` call sites
- `apple-media-call-symbols.txt`: unique Apple media function identifiers seen in the traced source files
- `apple-media-constants.txt`: unique `kVT*`, `kCV*`, `kCM*`, and `kCF*` constants referenced in those files
- `trace-meta.txt`: repo URL, ref, and resolved commit

## Initial Target

The initial source-trace target is `n8.1`, because the local Homebrew FFmpeg on this host reports version `8.1`.

This should be treated as the first bounded source target for Phase 0. Once the project has its own FFmpeg branch, the same script should be rerun against that exact branch and the guest ABI target should be updated from those results.

## Initial `n8.1` Findings

The current traced implementation files are:

- `libavcodec/videotoolbox.c`
- `libavcodec/videotoolboxenc.c`
- `libavcodec/videotoolbox_av1.c`
- `libavcodec/videotoolbox_vp9.c`
- `libavutil/hwcontext_videotoolbox.c`

Relevant headers also appear in the trace set:

- `libavcodec/videotoolbox.h`
- `libavutil/hwcontext_videotoolbox.h`

The current `configure` trace confirms that `videotoolbox` is treated as a first-class dependency and that multiple decoder hwaccels are gated on it, including:

- `h264_videotoolbox_hwaccel`
- `hevc_videotoolbox_hwaccel`
- `mpeg2_videotoolbox_hwaccel`
- `mpeg4_videotoolbox_hwaccel`
- `prores_videotoolbox_hwaccel`
- `vp9_videotoolbox_hwaccel`
- `av1_videotoolbox_hwaccel`

### What The Files Do

- `libavcodec/videotoolboxenc.c`
  - encode-session creation
  - VideoToolbox property mapping
  - encoder-property discovery
  - compressed sample extraction

- `libavcodec/videotoolbox.c`
  - decode-session creation
  - extradata and format-description creation
  - decoder specification and buffer-attribute setup
  - asynchronous decode result handling

- `libavutil/hwcontext_videotoolbox.c`
  - `CVPixelBufferPool` creation
  - IOSurface-backed attribute dictionaries
  - `CVPixelBuffer` mapping and plane access
  - attachment propagation for colorspace and pixel aspect ratio

- `libavcodec/videotoolbox_av1.c` and `libavcodec/videotoolbox_vp9.c`
  - codec-specific extradata packaging for VideoToolbox

### First Concrete Source-Level Conclusions

- The guest ABI target must include both **session APIs** and **pool-backed pixel-buffer APIs**. This is confirmed directly by `VTCompressionSessionGetPixelBufferPool`, `CVPixelBufferPoolCreate`, and `CVPixelBufferPoolCreatePixelBuffer` usage in source.
- The FFmpeg path is explicitly **planar-buffer aware**. This is confirmed by `CVPixelBufferIsPlanar`, `CVPixelBufferGetPlaneCount`, `CVPixelBufferGetBaseAddressOfPlane`, and `CVPixelBufferGetBytesPerRowOfPlane`.
- The encode path relies on **sample-buffer and block-buffer extraction** rather than raw NAL callbacks alone. This is confirmed by `CMSampleBufferGetDataBuffer`, `CMBlockBufferCopyDataBytes`, and parameter-set extraction helpers.
- The source tree also confirms that decode support materially expands the ABI target beyond encode. The initial v1 decision to keep decode out of scope remains correct.

### Artifact Paths

For the current run against `n8.1`, see:

- `artifacts/ffmpeg-source-trace/n8.1/source-files.txt`
- `artifacts/ffmpeg-source-trace/n8.1/configure-videotoolbox.txt`
- `artifacts/ffmpeg-source-trace/n8.1/apple-media-calls.txt`
- `artifacts/ffmpeg-source-trace/n8.1/apple-media-call-symbols.txt`
- `artifacts/ffmpeg-source-trace/n8.1/apple-media-constants.txt`
