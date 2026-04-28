# Contributing to vt-ferry

Thanks for your interest in contributing. vt-ferry is a paravirtualized
VideoToolbox runtime for Linux guests on Apple Silicon macOS hosts; the
codebase is opinionated about staying narrow.

## Scope

In scope:
- macOS-host, Linux-guest VideoToolbox encode + decode through the
  `videotoolbox` and `-hwaccel videotoolbox` paths in FFmpeg
- H.264 + HEVC, NV12 + BGRA + P010 (video and full range)
- Up to 4K — 8K is intentionally not supported in v1
- Smolvm + libkrun for the VM stack

Out of scope (deferred past v1):
- HLS / DASH segment output, overlay / concat filter chains driven by
  guest-side ffmpeg
- Non-FFmpeg guest applications
- Generic `virtio-video` or V4L2 portability

A patch that pulls one of the deferred items into v1 is fine in
principle but please open an issue first to align on scope.

## Development workflow

```sh
git clone --recurse-submodules <fork-url> vt-ferry
cd vt-ferry
./third_party/prepare_krun_runtime.sh
cargo test --workspace
cargo build --workspace --all-targets   # asserts no warnings
```

The smoke and gate matrix lives under `ffmpeg/scripts/`. See the
"Smoke and Gate Matrix" section in `README.md` for what each one
covers and which prerequisites it needs.

## Pull requests

- One logical change per PR. The repo's commit history is verbose by
  design; prefer commit messages that explain the WHY (constraints,
  prior bugs, tradeoffs) over the WHAT (the diff already shows that).
- Workspace must build warning-free. Any new warning is a real signal
  worth investigating — the project's "Things To Avoid" rule.
- New capability bits or protocol opcodes require a contract test that
  pins the new shape (see `protocol_surface_tests` for the pattern).
- New smoke wrappers should be ~20-line env-var overrides on the
  existing parents, not 200-line forks.

## Performance regressions

If a change touches a hot path, run the relevant `v1_*_perf_gate*.sh`
on a quiet host before + after and include the numbers in the PR body.
Run-to-run variance is real, especially on sub-second host wallclocks
— rerun to confirm a regression is reproducible before declaring it.

## Reporting bugs

Open a GitHub issue. Useful detail:
- macOS version + chip (M1 / M2 / M3 / etc.)
- smolvm + libkrun versions (`./third_party/check_krun_stack.sh`)
- The exact smoke or gate command you ran
- For decode failures, set `VT_FERRY_HOST_WORKER_STDERR_LOG=/tmp/worker.log`
  in the broker's environment before reproducing — VT's per-frame
  status often has the real story
