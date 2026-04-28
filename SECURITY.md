# Security policy

## Supported versions

Only the latest tagged release receives security fixes. vt-ferry is
pre-1.0 and may break compatibility between minor versions; if you
need stability across upgrades, pin a specific tag and watch this
repo for releases.

## Reporting a vulnerability

Email **tjfontaine@atxconsulting.com** with the details. Please do
not open a public issue for security-sensitive reports — let me
respond first so we can coordinate disclosure.

A good report includes:
- The vt-ferry version (`git rev-parse HEAD` from your checkout) and the
  smolvm + libkrun versions if relevant
- macOS version + chip
- A minimal reproduction (a smoke or gate command, or a small Rust /
  shell snippet that exercises the affected code path)
- Your view of the impact (data exposure, sandbox escape, DOS, etc.)

I aim to acknowledge reports within 72 hours and have an initial
assessment within a week.

## Scope

In scope:
- Memory safety bugs in the host worker, broker, or guest shim
- Sandbox / isolation escapes between guest and host
- Bugs in the protocol surface that let a malicious guest cause
  host-side crashes, OOM, or arbitrary code execution
- Bugs in the IOSurface / Mach port handoff path that let a guest
  read or write memory it shouldn't

Out of scope:
- Vulnerabilities in `smolvm`, `libkrun`, FFmpeg, or other upstream
  dependencies — please report those to the relevant projects
- Performance regressions or denial-of-service via legitimately-shaped
  but expensive inputs (decoding 4K at full bitrate is allowed to be
  slow); these are bug reports, not security issues
