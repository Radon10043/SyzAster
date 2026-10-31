# Setup `kernel/asterinas`

SyzAster targets the official Asterinas `v0.18.1` release (`d924a9635a66c7c3bb43e563eaafa2c61d6ee9d5`).

## Automated Checkout & Patching via `scripts/update-kernel.sh`

(container.syzaster):
```bash
mkdir -p $EXPERIMENT_ROOT/kernel/asterinas
$EXPERIMENT_ROOT/fuzzer/SyzAster/scripts/update-kernel.sh \
    --dir $EXPERIMENT_ROOT/kernel/asterinas
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
