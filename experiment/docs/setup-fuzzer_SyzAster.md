# Setup `fuzzer/SyzAster`

## Build SyzAster Rust Toolchain and Patched Syzkaller

(container.syzaster):
```bash
cd $EXPERIMENT_ROOT/fuzzer/SyzAster

# 1. Build all SyzAster Rust tools into ./bin/ and run the Rust test suite
make
make test

# 2. Apply the SyzAster patch series (patch/syzkaller/*.patch) to the syzkaller submodule and build syzkaller
make syzkaller
```

The built Rust binaries are placed in `$EXPERIMENT_ROOT/fuzzer/SyzAster/bin/`:
- `bin/syzaster`: Unified CLI entry point (`check-kcov`, `check-inst`, `kcov-rustc`, `eval-fuzz`)
- `bin/check-asterinas-kcov`: Validates live guest `/dev/kcov` ABI over SSH
- `bin/check-kcov-inst`: Verifies `rustc` sanitizer coverage instrumentation on `aster_core` vs `ostd`
- `bin/kcov-rustc`: Rust compiler wrapper for `KCOV=1` builds
- `bin/eval-fuzz`: Statistical campaign evaluation tool
