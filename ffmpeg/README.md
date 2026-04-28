# FFmpeg Integration

This directory is reserved for the custom FFmpeg integration work:

- build notes for the target FFmpeg branch
- Linux platform/configure seam patches for `videotoolbox`
- platform/configure patches needed to enable `videotoolbox` on Linux
- runtime packaging and guest image integration
- compatibility notes for supported workloads
- local binary trace artifacts and notes
- source-level trace workflow for `videotoolbox`

The goal is to keep FFmpeg's existing codec logic as close to upstream as possible and confine project-specific changes to the build and platform seams unless a focused behavior patch is required.

Supporting documents:

- [LOCAL-TRACE.md](./LOCAL-TRACE.md): observed imports and options from the local FFmpeg 8.1 binary
- [SOURCE-TRACE.md](./SOURCE-TRACE.md): source-level tracing workflow for the upstream FFmpeg tree

Current bring-up artifacts:

- `patches/0001-vt-ferry-linux-videotoolbox-build-seam.patch`
  - enables a custom Linux `videotoolbox` build against the guest compatibility libraries
  - adds a small Apple-compat macro shim for FFmpeg source files that currently assume Darwin headers
  - current checked-in seam reaches successful Linux `./configure`, full `ffmpeg` link,
    and encoder enumeration for:
    - `h264_videotoolbox`
    - `hevc_videotoolbox`
    - `prores_videotoolbox`
- `scripts/prepare_linux_videotoolbox_tree.sh`
  - applies the local patch set to the checked-out FFmpeg tree under `artifacts/ffmpeg-source/<ref>/FFmpeg`
  - prints the matching `./configure` invocation against a staged guest-shim library directory
- `scripts/stage_guest_shim_libs.sh`
  - builds `vt-ferry-shim`
  - stages framework-named linker aliases like `libVideoToolbox.so` / `libCoreMedia.so`
  - gives FFmpeg a concrete local library directory instead of relying on the stale `guest-shim/build` artifacts
  - defaults to the guest runtime target (`aarch64-unknown-linux-gnu`) and
    uses the vendored `libkrun` Linux sysroot to cross-link the guest `.so`
- `scripts/prove_smolvm_videotoolbox_encode.sh`
  - boots a real `smolvm` guest with launch-owned pool backing
  - runs guest FFmpeg against the staged Linux shim libraries
  - drives the brokered `vt-real` path over smolvm/libkrun vsock by default
  - validates that the produced MP4 is non-empty and reports H.264 video with the expected dimensions
  - supports `HOST_WORKER_BACKEND=mock` or `HOST_WORKER_BACKEND=vt-real`
- `scripts/compare_host_guest_videotoolbox_smoke.sh`
  - generates a small host-only FFmpeg `h264_videotoolbox` reference clip
    (or reads `${REFERENCE_VIDEO}` when set, e.g. the 1080p clip in
    `artifacts/reference-videos/`)
  - runs the real guest `smolvm` FFmpeg smoke path with `vt-real`
  - compares key `ffprobe` metadata fields between host and guest outputs
  - runs FFmpeg `psnr` and `ssim` lavfi passes between host and guest output
    so frame-content fidelity is reported alongside the metadata diff
  - optional `MIN_PSNR_AVERAGE` / `MIN_PSNR_MIN` / `MIN_SSIM_ALL` env vars
    fail the run when fidelity drops below the supplied thresholds
  - reports host FFmpeg wall time, guest FFmpeg wall time, and guest-to-host ratio for the current smoke workload
- `scripts/benchmark_host_guest_reference_encode.sh`
  - uses the downloaded reference clip in `artifacts/reference-videos/`
  - measures full-reference host `libx264` and host `h264_videotoolbox`
  - measures full-reference guest `libx264` with the distro FFmpeg inside the VM
  - measures full-reference guest `h264_videotoolbox` with the shimmed FFmpeg over vsock to the host worker
  - writes a JSON summary plus per-run FFmpeg `-benchmark` logs into a dedicated workdir under `artifacts/`
- `scripts/benchmark_host_guest_cpu_budget.sh`
  - runs the bounded reference clip inside a real `smolvm` guest
  - measures guest `libx264` vs guest `h264_videotoolbox` for:
    - FFmpeg `-benchmark` wall time
    - guest `/usr/bin/time -v` user/sys CPU time
    - host `smolvm` + `vt-ferry-host-worker` CPU deltas
  - contention headroom via fixed-block `openssl speed`
  - restarts the VM between software and `videotoolbox` scenarios so worker state from one encode run does not leak into the next benchmark phase
  - records separate OpenSSL baselines for the software and `videotoolbox` VM instances
  - writes a JSON summary plus per-run logs into a dedicated workdir under `artifacts/`
- `scripts/prove_docker_videotoolbox_tcp.sh`
  - runs the shimmed Linux FFmpeg binary with `docker run`, not
    `smolvm machine run`
  - exposes the host `vt-real` worker through `vt-ferry-broker --transport tcp`
    and a host TCP-to-worker-UDS bridge reachable from the container as
    `host.docker.internal`
  - uses the same staged guest shim libraries and broker-created IOSurface pool
    declarations as the comparison path, then writes an MP4 and JSON summary
    under `WORKDIR`
- `scripts/prepare_smolvm_vt_bench_image.sh`
  - builds a reusable linux/arm64 Ubuntu 24.04 guest image with FFmpeg and the
    runtime libraries needed by the shimmed `h264_videotoolbox` path already
    installed
  - pushes that image to a local Docker registry and writes
    `artifacts/vt-ferry-vt-bench-image/env.sh`
  - source that env file before benchmark/profiling runs to set `VM_IMAGE` and
    skip per-VM `apt-get` setup:
    - `. artifacts/vt-ferry-vt-bench-image/env.sh`
Current transport snapshot:

- 900-frame Docker TCP bridge proof:
  - Docker TCP `4.379s` in `artifacts/docker-vt-tcp-900/summary.json`
  - 120-frame Docker TCP `0.739s` versus the existing 120-frame smolvm-vsock
    artifact at `0.698s`
- 900-frame smolvm TCP bridge proof:
  - smolvm TCP `4.155s` in `artifacts/smolvm-vt-tcp-900/summary.json`
  - output probes as H.264 1920x1080, 30.0s, 900 decoded frames

Current runtime proof state:

- real `smolvm` guest one-frame `h264_videotoolbox` encode passes with:
  - `HOST_WORKER_BACKEND=mock`
  - `HOST_WORKER_BACKEND=vt-real`
- real `smolvm` guest short multi-frame `h264_videotoolbox` encode also passes with both backends
- this is still smoke-level validation:
  - container/codec metadata are sane
  - file production works
  - longer-clip fidelity coverage and a fidelity-regression gate are still
    future work
- current host-FFmpeg-vs-guest smoke comparison now shows:
  - matching codec, profile, level, dimensions, pixel format, frame count, extradata size, and bit_rate
  - PSNR=inf and SSIM=1.0 on the bounded `testsrc2` smoke (decoded YUV is bit-identical between host and guest output)
  - PSNR=inf and SSIM=1.0 with `REFERENCE_VIDEO=artifacts/reference-videos/bbb_sunflower_1080p_30fps_normal.mp4 FRAME_COUNT=30` (decoded YUV is bit-identical on real-world 1080p content too)
- current bounded reference CPU-budget benchmark in `artifacts/ffmpeg-cpu-budget-per-vm-baseline/summary.json` shows the strongest current value signal:
  - isolated 300-frame guest `libx264`: `6.081s` wall, `19.45` guest CPU-seconds
  - isolated 300-frame guest `h264_videotoolbox`: `3.571s` wall, `1.13` guest CPU-seconds
  - contention uses one SHA-256 block size and per-VM baselines, but the retention numbers remain noisy
  - the current value proposition is guest CPU reduction first; contention headroom needs more stable measurement before it should be treated as a hard claim
