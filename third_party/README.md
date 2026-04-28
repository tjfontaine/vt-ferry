# Third-Party Dependencies

## `smolvm`

`third_party/smolvm` is a pinned submodule of the public
[`smol-machines/smolvm`](https://github.com/smol-machines/smolvm) repo.

It is present so `vt-ferry` can develop against a real microVM/VMM codebase
instead of keeping the VMM bridge purely theoretical.

### Checkout note

At the time this submodule was added, the upstream repo's Git LFS budget was
exhausted for some bundled native libraries. To initialize or update the
submodule reliably, skip LFS smudge:

```bash
GIT_LFS_SKIP_SMUDGE=1 git submodule update --init --recursive
```

That is enough for source inspection and integration work. If a future task
needs the large LFS-managed binaries, we should either:

- use an upstream checkout after the LFS budget is restored
- point the submodule at a fork that republishes the required artifacts

### Relevant areas

The most relevant `smolvm` code for `vt-ferry` bridge work is currently:

- `third_party/smolvm/src/agent/launcher.rs`
- `third_party/smolvm/src/agent/manager.rs`
- `third_party/smolvm/src/vm/backend/libkrun.rs`
- `third_party/smolvm/crates/smolvm-network/`

Those are the likely integration points for host-side VM launch, guest/host
communication, and any future shared-memory aperture work.

### Local Runtime Bundle

For this project we want a mixed runtime:

- `libkrun`: our locally modified source build under `third_party/smolvm/libkrun`
- `libkrunfw`: the Homebrew-installed runtime from `slp/krun`

That lets `vt-ferry` keep using the patched local `libkrun` without accidentally
falling back to Homebrew's unmodified `libkrun`.

Prepare that bundle with:

```bash
./third_party/prepare_krun_runtime.sh
source ./artifacts/krun-runtime/macos-arm64/env.sh
```

The generated `env.sh` exports:

- `LIBKRUN_BUNDLE`
- `LIBKRUN_DIR`
- `SMOLVM_LIB_DIR`
- `DYLD_LIBRARY_PATH`

Use that environment for local `smolvm` builds and runs when the intent is to
exercise our modified `libkrun`.

### Local Verification

To run the shared-region checks against the local `libkrun` plus Homebrew
`libkrunfw` stack on macOS, use:

```bash
./third_party/check_krun_stack.sh
```

That script prepares the local runtime bundle, exposes Homebrew LLVM's
`libclang.dylib` to Cargo build scripts, sets the absolute Linux sysroot used
by the vendored `libkrun` Makefile/build scripts, and runs the current
`smolvm`/`libkrun` shared-region checks.
