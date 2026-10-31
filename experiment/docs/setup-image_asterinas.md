# Setup `image/asterinas`

Use `scripts/asterinas/create-image.sh` to build the KCOV-instrumented Asterinas kernel, generate the NixOS disk image with Dropbear SSH and stage-2 init, and stage the exact unstripped `vmlinux` ELF (`target/x86_64-unknown-none/debug/asterinas-osdk-bin`).

## 1. Coverage-Guided Image (`KCOV=1`) for `v0.18.1`

(container.syzaster):
```bash
$EXPERIMENT_ROOT/fuzzer/SyzAster/scripts/asterinas/create-image.sh \
    --asterinas $EXPERIMENT_ROOT/kernel/asterinas/asterinas-v0.18.1 \
    --out $EXPERIMENT_ROOT/image/asterinas/v0.18.1 \
    --version 0.18.1
```

This produces:
- `$EXPERIMENT_ROOT/image/asterinas/v0.18.1/asterinas.img`
- `$EXPERIMENT_ROOT/image/asterinas/v0.18.1/asterinas.id_rsa`
- `$EXPERIMENT_ROOT/image/asterinas/v0.18.1/kernel/vmlinux`

## 2. Baseline Uninstrumented Image (`KCOV=0`, `--no-cover`)

For ablation experiments comparing `SyzAster` (`KCOV=1`) against uninstrumented fuzzing (`KCOV=0`):

```bash
$EXPERIMENT_ROOT/fuzzer/SyzAster/scripts/asterinas/create-image.sh \
    --asterinas $EXPERIMENT_ROOT/kernel/asterinas/asterinas-v0.18.1 \
    --out $EXPERIMENT_ROOT/image/asterinas/v0.18.1-nocover \
    --version 0.18.1 \
    --no-cover
```

## 3. Validate Live Guest KCOV ABI

Start the VM in the background using `scripts/asterinas/qemu.sh` and run `bin/check-asterinas-kcov`:

```bash
$EXPERIMENT_ROOT/fuzzer/SyzAster/scripts/asterinas/qemu.sh \
    --image $EXPERIMENT_ROOT/image/asterinas/v0.18.1/asterinas.img \
    --port 10022 &
QEMU_PID=$!

# Wait for VM SSH to come up, then validate KCOV ABI
$EXPERIMENT_ROOT/fuzzer/SyzAster/bin/check-asterinas-kcov \
    $EXPERIMENT_ROOT/kernel/asterinas/asterinas-v0.18.1 \
    $EXPERIMENT_ROOT/image/asterinas/v0.18.1/asterinas.id_rsa \
    10022
kill $QEMU_PID
```
