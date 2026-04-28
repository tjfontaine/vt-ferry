# vt-ferry perf-debug harness

Tools for figuring out where time goes in real vt-ferry workloads —
not for chasing microbenchmark gate metrics. Use these when you
have a workload (transcode, encode, decode) that's slower than
expected and want to know why.

## What's in here

| file | purpose |
| --- | --- |
| `probe_audit.d` | dtrace audit script. Scoped to a process tree via `progenyof($1)`. Captures syscall counts + read/write byte volume + 997Hz user-stack samples per execname, then prints aggregations and exits via `tick-150s`. Reusable across any gate / smoke / benchmark. |
| `run_perf_gate_with_dtrace.sh` | Wrapper. Takes a benchmark / gate / smoke script as `$1`, kills any zombie vt-ferry-worker processes from prior failed runs, starts the workload, attaches dtrace scoped to the workload's process tree, captures audit summary to `/tmp/dtrace_out.txt`. |

## Why progenyof scoping rather than `dtrace -p WORKER_PID`

The vt-ferry perf gates spawn multiple processes:
- the gate script (bash)
- a Python sub-harness for orchestration
- `vt-ferry-broker`
- `smolvm` (the VMM)
- `vt-ferry-worker` (potentially multiple over the gate's lifetime)
- libkrun threads, vsock muxer threads, block I/O threads inside smolvm

Pid-provider tracing (`dtrace -p PID`) attaches to one PID. If the
worker respawns mid-gate, the trace silently goes blind. Using
`progenyof($GATE_PID)` from the syscall and profile providers
catches every descendant regardless of who forks who or when.

The trade-off: pid-provider tracing is needed for USDT-style
per-opcode dispatch attribution (the `vt_ferry_probe_*` hooks we
have on `vt-ferry-worker`). When you need that level of detail,
write a focused script alongside this one and attach it manually
to a known-fresh worker PID. For the "where's the time going"
survey question that motivates most perf work, the scoped
syscall + profile providers are simpler and don't drop coverage.

## Critical methodology footnote: smolvm symbols

The vendored smolvm release profile sets `strip = true`, so by
default smolvm's own user stacks render as raw hex addresses.
libkrun and Hypervisor frames stay symbolicated (those come from
system-installed dylibs), but you'll lose the smolvm-side
context — which thread, which CLI command, which agent path the
sample came from.

To get readable smolvm stacks, rebuild with debug symbols using
cargo's per-profile env-var overrides (no submodule modification):

```sh
cd third_party/smolvm
CARGO_PROFILE_RELEASE_DEBUG=true \
CARGO_PROFILE_RELEASE_STRIP=none \
cargo build --release --bin smolvm
```

The resulting binary at `third_party/smolvm/target/release/smolvm`
keeps the same path as the stripped build, so the perf gates
pick it up automatically. Re-run the audit and smolvm frames
will render symbolicated.

## Usage

```sh
# Survey: where does time go in the 4K decode perf gate?
ffmpeg/scripts/perf/run_perf_gate_with_dtrace.sh \
  ffmpeg/scripts/v1_decode_perf_gate_4k.sh

# Survey: where does time go in the 4K encode perf gate?
ffmpeg/scripts/perf/run_perf_gate_with_dtrace.sh \
  ffmpeg/scripts/v1_perf_gate_4k.sh

# Survey: a transcode benchmark (one of the few "actual usage" workloads)
ffmpeg/scripts/perf/run_perf_gate_with_dtrace.sh \
  ffmpeg/scripts/benchmark_host_guest_reference_transcode.sh
```

The wrapper writes to `/tmp/perfgate.out` (workload output) and
`/tmp/dtrace_out.txt` (dtrace summary). `/tmp/dtrace_out.txt` has:

- CPU sample counts by execname — at-a-glance view of who's busy
- syscall counts by (execname, syscall) — identifies hot syscalls
- read/write byte volume by (execname, syscall) — identifies data-plane volume
- top user stacks per execname — identifies hot code paths

## Reading the output: an example finding

From the v1 4K decode perf gate audit (Phase 12 + dtrace):

```
=== CPU sample counts by execname (997Hz) ===
vt-ferry-worker                    7      ← only 0.3% of host CPU
smolvm                         1440      ← 65% of host CPU
Python                          430      ← 19% — test harness, not workload
python3.14                      197
bash                             79
```

Most of the host CPU is in `smolvm`, and 75% of THAT is the
guest VCPU running guest-side code (which the host can't dtrace
into). Only ~7% of host CPU is libkrun overhead (vsock muxer,
GIC IRQ injection, block I/O, MMIO traps). The host worker
contributed essentially nothing — meaning host-side worker
optimizations buy zero on this metric. To improve real 4K
decode perf, the lever has to be inside the guest VM (shim hot
paths, FFmpeg, etc.).

This is the kind of question the harness answers in one run.
Without it, we'd be guessing at where the bottleneck is.

## Caveats / sharp edges

- `sudo dtrace` must be configured `nopasswd`. If `sudo` prompts
  for a password the script silently produces empty output.
- `tick-150s` is the script's self-terminator. Don't run it
  against gates that take longer than that (e.g. very slow
  network-mediated VM bringup) — you'll get partial output.
  Bump the tick value in `probe_audit.d` if you need more.
- Multiple stale dtrace processes can pile up if a wrapper
  invocation gets killed mid-flight. They're at 0% CPU (the
  worker they were attached to is gone), so they don't actively
  hurt anything, but they pollute `pgrep`. Clean up with
  `sudo pkill dtrace` if needed.
- The harness measures **host-side** perf characteristics. To
  see what the guest is doing (FFmpeg's hwaccel decode loop, the
  shim's chunked-read memcpy, etc.), you'd need to dtrace
  inside the VM, which isn't supported through this harness.
  Use the worker's USDT probes (`vt_ferry_probe_*` in
  `vt-ferry-worker/src/probes.rs`) and a pid-provider script
  attached to a known-fresh worker PID for that level of
  detail.
