# vt-ferry

`vt-ferry` is an Apple-specific paravirtualized VideoToolbox runtime for Linux guests on Apple Silicon macOS hosts. Linux processes inside a smolvm + libkrun VM call into ABI-compatible `VideoToolbox` / `Core*` libraries and the real Apple media stack drives `VTCompressionSession` / `VTDecompressionSession` on the host.

The core idea is:

- the macOS host owns the real Apple media stack
- the Linux guest loads ABI-compatible `VideoToolbox` / `Core*` compatibility libraries
- raw frame data moves through shared mapped buffers
- FFmpeg uses its existing `videotoolbox` integration path with minimal application-level changes

This is intentionally **not** a generic `virtio-video` or V4L2 portability project. The product goal is to preserve Apple-specific media semantics and feature velocity on macOS hosts.

Released under the [MIT License](./LICENSE).

## Getting started

Prerequisites: macOS on Apple Silicon, Rust toolchain (matching `rust-toolchain.toml` if present), Xcode command-line tools, `ffmpeg` on the host, and Docker Desktop (or any docker daemon running ARM64 Linux containers — used to build the vt-ferry guest runtime image).

```sh
# Clone with the smolvm submodule
git clone --recurse-submodules <repo-url> vt-ferry
cd vt-ferry

# Stage the libkrun runtime that smolvm uses
./third_party/prepare_krun_runtime.sh

# Build the workspace and run the unit tests (no VM required)
cargo test --workspace
cargo build --workspace --all-targets   # asserts no warnings

# Build the vt-ferry guest runtime image (the patched FFmpeg + the
# guest shim libraries, packaged as an OCI image and pushed to a
# local registry on :5005 so smolvm can pull it).
sh ffmpeg/scripts/build_vt_ferry_image.sh

# Source the env vars the build emitted (sets VM_IMAGE, GUEST_FFMPEG_BIN
# pointing at /opt/vt-ferry/bin/ffmpeg in the image, etc.)
. artifacts/vt-ferry-guest-image/env.sh

# Build the broker in release mode and the worker in debug mode
# (the smoke wrappers default to target/release/vt-ferry-broker and
# target/debug/vt-ferry-worker; debug worker keeps panic + dtrace
# probes useful, release broker keeps startup latency low).
cargo build --release -p vt-ferry-broker
cargo build -p vt-ferry-worker

# Smoke-test a real end-to-end encode through smolvm + libkrun:
USE_VT_FERRY_IMAGE=1 sh ffmpeg/scripts/prove_smolvm_videotoolbox_encode.sh
```

If the encode smoke passes, the host worker, broker, smolvm vsock, guest shim, and `h264_videotoolbox` are all wired correctly. The full smoke + gate matrix is documented under `## Smoke and Gate Matrix` below.

Release notes: see [CHANGELOG.md](./CHANGELOG.md).

## Repository Layout

Rust workspace (every member lives under `crates/`):

Shipping crates:
- `crates/vt-ferry-protocol/` — wire protocol types shared by the worker and the guest shim
- `crates/vt-ferry-shim/` — Linux-guest `VideoToolbox` / `Core*` compatibility library (the `cdylib` that ffmpeg links against inside the VM); `include/` co-locates the Apple-style public headers staged for the Linux ffmpeg build
- `crates/vt-ferry-worker/` — macOS host-side worker; owns the real `VTCompressionSession` / `VTDecompressionSession` and Apple media objects
- `crates/vt-ferry-broker/` — host-side launcher; pre-allocates IOSurface pools, registers them via the kernel's `IOSurfacePoolDirectory`, supervises a `smolvm + worker` pair, and selects the vsock-or-TCP transport

Internal harness / probe binaries (not published, not part of the runtime):
- `crates/zero-copy-harness/` — end-to-end proof harness that spawns the worker with a launcher-allocated IOSurface and asserts the encode path took the zero-copy branch
- `crates/hvf-iosurface-probe/` — standalone probe answering "can `hv_vm_map` alias IOSurface pages?" — used during bring-up of the IOSurface fast path

Build / smoke / gate harness:
- `ffmpeg/Dockerfile` — multi-stage build for the `vt-ferry-guest` runtime image
- `ffmpeg/patches/` — the vt-ferry FFmpeg patch (registers the `--enable-vt-ferry-videotoolbox-linux` build seam)
- `ffmpeg/scripts/` — smokes, benchmarks, perf gates, fidelity gates, build wrappers
- `ffmpeg/scripts/perf/` — dtrace-based perf-debug harness
- `ffmpeg/README.md`, `ffmpeg/LOCAL-TRACE.md`, `ffmpeg/SOURCE-TRACE.md` — FFmpeg-integration notes from initial bring-up

Other:
- `third_party/smolvm/` — pinned [`smol-machines/smolvm`](https://github.com/smol-machines/smolvm) submodule providing the libkrun-based microVM
- `third_party/*.sh` — host-side helpers (`prepare_krun_runtime.sh`, etc.)

## Architecture Summary

The system has five pieces:

1. **Host broker** (`vt-ferry-broker`)
   - what users actually launch on the macOS host
   - pre-allocates IOSurface pools and registers them via the kernel's `IOSurfacePoolDirectory` so the guest can claim them by name
   - chooses the transport (vsock for smolvm + libkrun, TCP for Docker) and supervises the `smolvm + worker` pair

2. **Host worker** (`vt-ferry-worker`)
   - owns `VTCompressionSession`, `VTDecompressionSession`, `CVPixelBuffer`, `CMSampleBuffer`, and `IOSurface`
   - spawns a fresh process per guest connection so multi-process concurrent transcodes get isolated session-id namespaces
   - isolates Apple media API failures from the VMM

3. **Paravirtual transport**
   - vsock + libkrun for smolvm; TCP for the Docker bridge
   - maps host-owned frame buffers into the guest (zero-copy via launcher-pre-allocated IOSurfaces, chunked `READ_BUFFER` / `WRITE_BUFFER` otherwise)
   - carries control messages, completions, heartbeats, and error signaling

4. **Guest compatibility libraries** (`vt-ferry-shim`)
   - export the Apple C ABI expected by FFmpeg (`CoreFoundation`, `CoreMedia`, `CoreVideo`, `VideoToolbox`)
   - represent Apple objects as guest-side proxy handles backed by host state, with poisoned-backend semantics for fast failure on worker death

5. **Guest applications**
   - the patched FFmpeg n8.1 build using `h264_videotoolbox` / `hevc_videotoolbox` and `-hwaccel videotoolbox` decode

## Current v1 Scope

- Apple Silicon macOS host (`hvf` via libkrun)
- Linux guest (smolvm + libkrun + Ubuntu 24.04 image)
- **Encode + decode + transcode** all hardware-accelerated through the host's VideoToolbox
- **H.264 + HEVC** (8-bit + 10-bit P010 / Main10)
- **NV12 + BGRA + P010** pixel formats
- Custom FFmpeg build (n8.1) with a thin patch enabling the videotoolbox hwaccels under Linux
- vsock-based protocol transport (libkrun's `krun_add_vsock_port2`); a parallel TCP bridge exists for Docker Desktop

## What works today

End-to-end through real `smolvm` + libkrun vsock + the host worker (smoke-covered):

- Hardware **encode** (`h264_videotoolbox`, `hevc_videotoolbox`, including HEVC Main10 / 10-bit P010)
- Hardware **decode** (`-hwaccel videotoolbox` for H.264 + HEVC, with bit-identical output vs host VT)
- **Transcode** (VT decode + VT encode in one ffmpeg pipeline; the most common real-world workload)
- **Multi-process concurrent transcode** (two ffmpeg processes inside one VM, each transcoding through its own worker connection with isolated session-id namespaces)
- **Multi-stream encode** (one input → N concurrent `VTCompressionSession`s in the same ffmpeg run; the multistream smoke runs 720p + 480p H.264)
- **ABR mid-stream format change** (one VTDecompressionSession crossing a format-description boundary)
- Filter graphs (scale, fps, format conversion) upstream of the encoder
- Multiple containers (mp4 / mov / mkv / ts)
- Audio passthrough (`-c:a copy`) on multi-stream demuxes
- 1080p at >5× realtime, 4K at near-realtime
- IOSurface zero-copy fast path for launcher-pre-allocated pools
- Hardening: heartbeats (`OP_PING`), terminal kill, failure-injection soak (20 cycles × 4 failure modes); guest-shim atomics (`VTF_PROXY_ALIVE` / `VTF_DATA_COPY_BYTES` / `VTF_VT_SESSIONS_CREATED`) drive in-process leak-detection assertions in the unit tests — there is no user-facing readout for these counters

Benchmarked but not smoke-gated:

- **Multi-rendition ABR ladder transcode** (one VTDecompressionSession + N concurrent VTCompressionSessions) — `benchmark_host_guest_reference_transcode_multirendition.sh`
- **Scale-during-transcode** (4K → 1080p swscale downscale, then VT encode) — `benchmark_host_guest_reference_transcode_scale.sh`

Coverage:
- 14 smolvm-side end-to-end smokes
- 2 Docker / TCP-bridge smokes for users not running smolvm directly
- 8 codified perf gates (encode 1080p+4K, decode 1080p H.264+HEVC+4K, transcode 1080p+4K+HEVC)
- 3 fidelity gates (encode PSNR/SSIM, decode H.264 + HEVC bit-identical)
- 7 reference benchmarks for one-off measurement (no thresholds)
- 179 in-process unit + integration tests (sub-second, run on every `cargo test`)
- First-class dtrace audit harness for diagnosing real-workload perf

The multi-process concurrent transcode bullet above is gated by `prove_smolvm_videotoolbox_transcode_concurrent.sh`, which runs two parallel transcodes through the broker's pre-registered IOSurface directory and verifies both outputs. The broker's `--pool` argument fixes the per-VM concurrency ceiling (the launcher-registered IOSurface count); the worker reads it from `VT_FERRY_IOSURFACE_POOL_SPECS_JSON`.

## Performance

All numbers are end-to-end through the smolvm + libkrun + vsock path on Apple Silicon macOS, comparing against host-native VT and against software (libx264 / libx265) baselines. See `ffmpeg/scripts/v1_*_perf_gate*.sh` for the actual gates and `ffmpeg/scripts/benchmark_host_guest_reference_*.sh` for the reference benchmarks they drive.

### Encode (1080p H.264, libx264 baseline)

| metric | guest VT | gate threshold |
|---|---:|---|
| wallclock vs host VT | ~1.0× | `v1_perf_gate.sh` cap: 2.0× |
| × realtime | ~5.9× | `v1_perf_gate.sh` floor: 2.0× |
| **CPU vs libx264** | **~10%** | 90% CPU saved |

### Transcode — the metric that matches real workloads

End-to-end decode + encode in one ffmpeg pipeline. Pipeline parallelism between the encode and decode sides hides vsock transport latency, so transcode is closer to host-VT-native performance than the decode-only or encode-only microbenchmarks suggest.

Real VT decode + VT encode end-to-end means both directions cross the host-guest boundary, which is why the wallclock ratio is meaningfully above 1.0× — the CPU savings on the right column are the headline number.

| workload | guest VT vs host VT | × realtime | guest VT vs software CPU |
|---|---:|---:|---:|
| 1080p H.264 transcode (`v1_transcode_perf_gate.sh`) | **~1.66×** | 4.1× / ~123 fps | **~26% of libx264** (74% saved) |
| 4K H.264 transcode (`v1_transcode_perf_gate_4k.sh`) | **~1.62×** | 1.1× / ~33 fps | **~29% of libx264** (71% saved) |
| 1080p HEVC transcode (`v1_transcode_perf_gate_hevc.sh`) | **~1.60×** | 4.1× / ~122 fps | **~3% of libx265** ¹ (97% saved) |
| 4K → 1080p scale-transcode | 1.08× ² | 3.0× | 54% of libx264 |
| 1080p × 2 ABR renditions (1 decode + 2 encoders) | 1.006× | 3.6× | 0.60 cores avg |

¹ libx265 is far more compute-heavy than libx264 in software, so hardware HEVC transcode wins enormously vs the software baseline. This is the "value of hardware" headline number for HEVC workloads.

² The 8% guest-vs-host overhead in scale-during-transcode is swscale's CPU pressure inside the guest — pipeline parallelism can't hide it the way no-scale transcode can.

### Decode (`-f null -` microbenchmarks; less representative of real usage)

These measure decode-only throughput where pipeline parallelism *can't* absorb transport latency. The CPU efficiency numbers look worse here than in transcode for the same hardware — the transcode numbers above are the right "value of hardware" metric.

The wallclock ratio metric is structurally noisy at sub-second host wallclocks — guest fps and CPU savings stay stable run-to-run while the ratio swings 30%+. Read the right column for the headline.

| workload | guest VT vs host VT | × realtime | guest VT vs libavcodec CPU |
|---|---:|---:|---:|
| 1080p H.264 decode | 1.89–2.65× | 6.4× / ~190 fps | **~50% saved** |
| 1080p HEVC decode | 2.00–2.85× | 6.1–6.4× / ~190 fps | **~76% saved** |
| 4K H.264 decode | 2.88–2.95× | 1.5× / ~45 fps | ~5–7% saved (transport-dominated) |

### Fidelity

- Encode (`long_encode_fidelity.sh`): PSNR/SSIM regression gate against the reference clip
- Decode H.264 (`long_decode_fidelity.sh`): **bit-identical** vs host VT (PSNR=inf, SSIM=1.0)
- Decode HEVC (`long_decode_fidelity_hevc.sh`): **bit-identical** vs host VT

The bit-identical decode baseline is structural — both host and guest paths use the same Apple VideoToolbox implementation; the shim is a transparent wire bridge. Any deviation here is by definition a transport-layer bug, so the gate's PSNR ≥ 80 / SSIM ≥ 0.999 thresholds are tight.

### Where the host CPU goes (dtrace)

For real transcode workloads the **host worker is 0.3% of host CPU**. ~49% is just the VCPU running guest code (which is itself doing the actual decode + encode + shim transport — opaque to host dtrace). ~7% is libkrun overhead (vsock muxer, GIC IRQ injection, block I/O, MMIO traps). The Python test harness eats ~28% during gate runs.

This is why host-side perf optimization buys ~zero on real workloads — the work the system is doing happens inside the guest VM. Wins for guest CPU have to come from guest-side improvements (shim hot paths, avoiding redundant memcpys in the chunked-read drain) or from host-VT-side direct hand-offs (encode-from-decode-output for matched-format transcode, deferred to v2).

## Smoke and Gate Matrix

The end-to-end smokes all share `prove_smolvm_videotoolbox_encode.sh`
(broker + smolvm + libkrun vsock + guest FFmpeg) and differentiate
through env knobs. Each wrapper is a thin shell file that fixes the
relevant variables for one scenario:

| wrapper | what it covers |
| --- | --- |
| `prove_smolvm_videotoolbox_encode.sh` | canonical `h264_videotoolbox` smoke; lavfi or REFERENCE_VIDEO source |
| `prove_smolvm_videotoolbox_hevc.sh` | same path with `hevc_videotoolbox` (Main profile, 8-bit) |
| `prove_smolvm_videotoolbox_hevc_p010.sh` | HEVC + `-profile:v main10` + `PIXEL_FORMAT=0x78343230` (P010 / 10-bit) |
| `prove_smolvm_videotoolbox_filtergraph.sh` | scale + fps filter chain (480x270/30 → 1280x720/24) — exercises swscale upstream of the encoder |
| `prove_smolvm_videotoolbox_containers.sh` | mp4 / mov / mkv / ts; validates the encoder shim is mux-agnostic |
| `prove_smolvm_videotoolbox_audio_passthrough.sh` | bbb_sunflower with `-c:a copy`; validates the shim doesn't disturb FFmpeg's multi-stream demux/mux machinery |
| `prove_smolvm_videotoolbox_multistream.sh` | one input → two concurrent VTCompressionSessions (720p + 480p H.264 in the same ffmpeg run); exercises the worker's session-isolation guarantees and concurrent buffer-pool recycling |
| `prove_smolvm_videotoolbox_decode.sh` | host libx264 produces a 480p H.264 bitstream; guest decodes via `-hwaccel videotoolbox` (the shim's `VTDecompressionSession` path) into raw NV12; verifies output size matches expected NV12 layout. Hits the inline `OP_READ_DECODED_FRAME` path (≤720p) |
| `prove_smolvm_videotoolbox_decode_hevc.sh` | same path with libx265 producing the bitstream; exercises the HEVC parameter-set delivery (VPS+SPS+PPS) and `hevc_videotoolbox_hwaccel` selection |
| `prove_smolvm_videotoolbox_decode_1080p.sh` | 1920×1080 H.264; exercises the pool-bound decode-output path (`OP_BIND_DECODE_OUTPUT_POOL` + chunked `OP_READ_BUFFER`) since per-frame size (~3 MiB) exceeds the 1.5 MiB inline budget |
| `prove_smolvm_videotoolbox_decode_4k.sh` | 3840×2160 H.264; same pool path at 4K (~12 MiB / frame, ~12 chunks per OP_READ_BUFFER round) |
| `prove_smolvm_videotoolbox_decode_abr.sh` | ABR mid-stream format change: encodes 480p + 720p H.264 baseline streams, splices via concat demuxer, decodes through one VTDecompressionSession that crosses the format-description boundary. Exercises the worker's `OP_SET_DECODE_FORMAT` re-issue + `VTDecompressionSessionCanAcceptFormatDescription` branch |
| `prove_smolvm_videotoolbox_transcode.sh` | VT decode + VT encode in the same guest FFmpeg process (480p H.264 → swscale → h264_videotoolbox). Validates both VT session kinds coexist on the broker without state-machine collision and the decoder→encoder hand-off works through FFmpeg's filter graph |
| `prove_smolvm_videotoolbox_transcode_concurrent.sh` | two ffmpeg *processes* inside the same VM, each transcoding 640×480 H.264 in parallel through their own worker connection. Validates the worker's spawn-per-connection accept loop, the `IOSurfacePoolDirectory` entry-claim path (each guest claims one launcher-registered IOSurface), and DESTROY_SESSION-on-CFRelease so ffmpeg's "probe encoder, then real encoder" pattern doesn't leak directory entries within a single connection |
| `prove_docker_videotoolbox_tcp.sh` | parallel Docker / TCP-bridge smoke; same shimmed Linux FFmpeg `h264_videotoolbox` reaches the host worker through `vt-ferry-broker --transport tcp` |
| `prove_docker_videotoolbox_tcp_hevc.sh` | Docker / TCP variant with `hevc_videotoolbox` |

Performance gates (codified thresholds; CI failure if breached):

| gate | what it asserts |
| --- | --- |
| `v1_perf_gate.sh` (1080p) | guest VT encode keeps up with realtime, stays within 2.0× of host VT, CPU ratio ≤ 0.5 vs libx264 |
| `v1_perf_gate_4k.sh` | 4K encode variant; realtime floor relaxed to 1.0× (4K H.264 is materially harder; HW VT is the only path that keeps up) |
| `v1_decode_perf_gate.sh` (1080p H.264) | guest VT decode keeps up with realtime (≥5.0×), stays within 3.0× of host VT decode, CPU ratio ≤ 1.5× vs libavcodec (looser cap than encode because raw NV12 transport dominates the CPU split, and post-Phase-15 the wallclock-ratio noise floor is wider) |
| `v1_decode_perf_gate_hevc.sh` | HEVC variant (1080p); stages a hevc_videotoolbox-encoded reference once and caches it. Wallclock ratio cap is 3.5× (looser than H.264's 3.0×) because HEVC's faster host VT decode produces sub-second wallclocks where 50–100ms host-side noise = 6–12% ratio variance |
| `v1_decode_perf_gate_4k.sh` | 4K decode variant; realtime floor relaxed to 1.0× (raw NV12 transport ~120 MB/s at 4K30), wallclock ratio cap 4.0× (12 MiB/frame transport vs sub-second host wallclock), CPU cap 2.0× (transport CPU grows linearly with frame area) |
| `long_encode_fidelity.sh` | encode-side PSNR/SSIM regression gate against the reference clip |
| `long_decode_fidelity.sh` | decode-side fidelity gate: host-VT-decoded NV12 vs guest-VT-decoded NV12 (same H.264 source). Tighter thresholds than encode (PSNR≥80, SSIM≥0.999) since both paths use the same Apple decoder — any deviation is a transport-layer bug. Baseline observed: bit-identical (PSNR=inf, SSIM=1.0) |
| `long_decode_fidelity_hevc.sh` | HEVC variant of the decode fidelity gate; reuses the cached HEVC reference. Bit-identical baseline holds across codecs |
| `v1_transcode_perf_gate.sh` (1080p H.264) | end-to-end transcode (decode + encode in one ffmpeg pipeline); guest VT must be within 2.0× of host VT, ≥2.0× realtime, and ≤0.5× of guest libx264 CPU. Real-world workload pattern; the decode-only and encode-only gates each move pixels in one direction across vsock and miss the pipeline-parallelism dynamic this captures |
| `v1_transcode_perf_gate_4k.sh` | 4K transcode variant; realtime floor relaxed to 1.0× |
| `v1_transcode_perf_gate_hevc.sh` | HEVC transcode variant; observed CPU ratio is dramatic — guest VT HEVC transcode uses ~6% of libx265's CPU (libx265 is far more compute-heavy than libx264, so hardware HEVC wins big) |

Reference benchmarks (no thresholds — produce summary.json for inspection / one-off measurement):

| benchmark | what it measures |
| --- | --- |
| `benchmark_host_guest_reference_encode.sh` | encode-only 4-way (host SW / host VT / guest SW / guest VT) |
| `benchmark_host_guest_reference_decode.sh` | decode-only 4-way |
| `benchmark_host_guest_reference_transcode.sh` | end-to-end transcode 4-way (the workload users actually run) |
| `benchmark_host_guest_reference_transcode_hevc.sh` | HEVC transcode |
| `benchmark_host_guest_reference_transcode_scale.sh` | 4K→1080p scale-during-transcode (exposes guest-side swscale CPU pressure) |
| `benchmark_host_guest_reference_transcode_multirendition.sh` | one VTDecompressionSession + N concurrent VTCompressionSessions (ABR-ladder pattern) |
| `benchmark_host_guest_cpu_budget.sh` | guest VT encode under host CPU contention (concurrent openssl block-cipher load); validates the worker stays predictable when the host is busy |

### Perf-debug harness

When a perf gate or real workload is slower than expected, the
dtrace-based audit harness in `ffmpeg/scripts/perf/` answers
"where does the time go?" in one run. It scopes via
`progenyof($GATE_PID)` so it sees broker + smolvm + vt-ferry-worker
+ helpers without losing coverage when processes restart. See
`ffmpeg/scripts/perf/README.md` for the full design notes,
methodology footnotes (smolvm symbol-stripping, dtrace
self-termination), and example findings.

```sh
# Survey: where does host CPU go in this gate / smoke / benchmark?
ffmpeg/scripts/perf/run_perf_gate_with_dtrace.sh \
  ffmpeg/scripts/v1_decode_perf_gate_4k.sh
```

## Unit / integration tests

`cargo test --workspace` runs 179 in-process tests in well under a
second. Two modules are worth calling out explicitly because they
codify protocol-level contracts the smokes only check end-to-end:

- `buffer_layout_tests` in `crates/vt-ferry-worker/src/vt_real.rs` —
  exhaustive coverage of `vtf_fill_buffer_layout` and
  `vtf_buffer_total_size` for every supported pixel format (NV12 video /
  full range, P010 video / full range, BGRA). New pixel formats land
  here first; the smoke is the integration check.
- `worker_survives_repeated_failure_injection` in
  `crates/vt-ferry-worker/tests/failure_injection_soak.rs` —
  failure-injection soak; 20 cycles × 4 failure modes against a real
  worker process.

## License

This project is licensed under the [MIT License](./LICENSE).

The `third_party/smolvm` submodule is licensed separately under
Apache 2.0; see its repo at https://github.com/smol-machines/smolvm
for details.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for scope, workflow, and PR
guidance. Security reports go to **tjfontaine@atxconsulting.com** —
see [SECURITY.md](./SECURITY.md). Project conduct: see
[CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md).

