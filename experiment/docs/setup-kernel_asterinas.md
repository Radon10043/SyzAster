# Setup `kernel/asterinas`

SyzAster supports official Asterinas releases `v0.19.0` (`c4b7ed575b7b32773475b194d2ad41fb6d03765b`), `v0.18.1` (`d924a9635a66c7c3bb43e563eaafa2c61d6ee9d5`), `v0.18.0` (`253be4750d69810af7b7b020fe2fee40a8547e15`), `v0.17.2` (`23adfdfd72b05cee8d232809caea81a4b33d3488`), and `v0.17.1` (`9ea44ed2b60bc81a5efb18af79e41fc07bf3d523`).

## Automated Checkout & Patching via `scripts/update-kernel.sh`

(container.syzaster):
```bash
mkdir -p $EXPERIMENT_ROOT/kernel/asterinas
# Pass -v <v0.17.1|v0.17.2|v0.18.0|v0.18.1|v0.19.0> (defaults to v0.18.1):
$EXPERIMENT_ROOT/fuzzer/SyzAster/scripts/update-kernel.sh \
    --dir $EXPERIMENT_ROOT/kernel/asterinas \
    --version v0.18.1
```

This creates:
- `$EXPERIMENT_ROOT/kernel/asterinas/asterinas-v0.18.1` (patched with `patch/asterinas/v0.18.1/*.patch`)

## Manual Checkout & Patch Application

(container.syzaster):
```bash
git clone https://github.com/asterinas/asterinas.git $EXPERIMENT_ROOT/kernel/asterinas/asterinas-v0.18.1
cd $EXPERIMENT_ROOT/kernel/asterinas/asterinas-v0.18.1
git checkout --detach d924a9635a66c7c3bb43e563eaafa2c61d6ee9d5

# Apply SyzAster compatibility & KCOV patch series
git am $EXPERIMENT_ROOT/fuzzer/SyzAster/patch/asterinas/v0.18.1/*.patch
```

## Verify Rust Compiler Instrumentation (`check-kcov-inst`)

Before building the full kernel image, verify that the pinned Rust compiler (`nightly-2026-07-21`) and `kcov-rustc` wrapper emit `__sanitizer_cov_trace_pc` callbacks for `aster_core` while leaving `ostd` uninstrumented:

```bash
$EXPERIMENT_ROOT/fuzzer/SyzAster/bin/check-kcov-inst \
    --asterinas $EXPERIMENT_ROOT/kernel/asterinas/asterinas-v0.18.1
```
