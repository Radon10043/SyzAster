# Setup: Linux Host, QEMU VM, Asterinas x86-64 Kernel

This guide explains how **SyzAster** runs coverage-guided `syzkaller` against Asterinas using the `linux/amd64` target. Because Asterinas exposes a Linux-compatible syscall ABI, Linux syscall descriptions and `syz-executor` are reused with targeted adaptations in `patch/syzkaller/`.

## Requirements

- Asterinas release source:
  - `v0.19.0` (commit `c4b7ed575b7b32773475b194d2ad41fb6d03765b`, Rust `nightly-2026-07-21`)
  - `v0.18.1` (commit `d924a9635a66c7c3bb43e563eaafa2c61d6ee9d5`, Rust `nightly-2026-07-21`)
  - `v0.18.0` (commit `253be4750d69810af7b7b020fe2fee40a8547e15`, Rust `nightly-2026-04-03`)
  - `v0.17.2` (commit `23adfdfd72b05cee8d232809caea81a4b33d3488`, Rust `nightly-2026-04-03`)
  - `v0.17.1` (commit `9ea44ed2b60bc81a5efb18af79e41fc07bf3d523`, Rust `nightly-2025-12-06`)
- Asterinas build prerequisites: Nix (`nix-build`, `nixos-install`), QEMU (`qemu-system-x86_64`), OVMF (`/usr/share/qemu/OVMF.fd`), OpenSSL, and `VDSO_LIBRARY_DIR` containing `vdso_x86_64.so` (all pre-installed in `docker/Dockerfile`).
- Rust nightly toolchain matching the target release's `rust-toolchain.toml` (`nightly-2026-07-21`, `nightly-2026-04-03`, or `nightly-2025-12-06`) with `rust-src`, `rustc-dev`, `llvm-tools-preview`, and `x86_64-unknown-none`.
- SyzAster patch series applied to `asterinas` (`patch/asterinas/v0.19.0/`, `v0.18.1/`, `v0.18.0/`, `v0.17.2/`, or `v0.17.1/`) and `syzkaller` (`patch/syzkaller/`).

## Patch Asterinas and Syzkaller

Apply the Asterinas compatibility and KCOV patch series (or use `./scripts/update-kernel.sh -d /vol -v <v0.17.1|v0.17.2|v0.18.0|v0.18.1|v0.19.0>`):
```bash
# Example for Asterinas v0.19.0 (or replace v0.19.0 with v0.18.1 / v0.18.0 / v0.17.2 / v0.17.1):
git -C $ASTERINAS switch --detach v0.19.0
git -C $ASTERINAS am $SYZASTER/patch/asterinas/v0.19.0/*.patch
```

Build SyzAster Rust tools and patched `syzkaller`:
```bash
cd $SYZASTER
make
make syzkaller
```

## Build the Asterinas NixOS Image

Run `scripts/asterinas/create-image.sh`:
```bash
cd $SYZASTER
./scripts/asterinas/create-image.sh \
    --asterinas $ASTERINAS \
    --out $SYZASTER/.workdir \
    --version 0.18.1
```

The script writes:
- `$SYZASTER/.workdir/asterinas.img`
- `$SYZASTER/.workdir/asterinas.id_rsa`
- `$SYZASTER/.workdir/kernel/vmlinux` (coverage mode only)

For an uninstrumented image (`KCOV=0`), pass `--no-cover`:
```bash
./scripts/asterinas/create-image.sh \
    --asterinas $ASTERINAS \
    --out $SYZASTER/.workdir \
    --version 0.18.1 \
    --no-cover
```

### Unstripped ELF Staging

`scripts/asterinas/create-image.sh` pins full builds to the development profile (`RELEASE=0`, `RELEASE_LTO=0`) and `$ASTERINAS/target`. OSDK's original unstripped ELF is `target/x86_64-unknown-none/debug/asterinas-osdk-bin` (or `target/x86_64-unknown-none/debug/aster-kernel-osdk-bin` on `v0.17.1`–`v0.18.0`). The script inspects the ELF section headers and symbol table to verify that `.symtab`, `.debug_info`, and a defined `__sanitizer_cov_trace_pc` symbol exist before staging it byte-for-byte to `kernel/vmlinux`.

## Configure `syz-manager` and Run Fuzzing

- **Prepare `syz-manager` Configuration**:
  Copy [`configs/fuzz/asterinas/kernel.cfg`](../configs/fuzz/asterinas/kernel.cfg) to `$SYZASTER/.workdir/fuzz.cfg` and replace the `__XXX__` placeholders (`__KERNEL_VERSION__`, `__WORKDIR__`, `__IMAGE__`, `__SYZKALLER__`, `__KERNEL__`, `__VMCOUNT__`). For an uninstrumented (`--no-cover`) image, set `"cover": false` and remove `kernel_obj`, `kernel_src` and `kernel_build_src`: such an image stages no `kernel/vmlinux`, which `kernel_obj` would require.
- **Compiler Instrumentation Check**:
  ```bash
  ./bin/check-kcov-inst --asterinas $ASTERINAS
  ```
- **Live Guest KCOV ABI Check**:
  ```bash
  ./bin/check-asterinas-kcov \
      "$ASTERINAS" \
      "$SYZASTER/.workdir/asterinas.id_rsa" \
      "$SSH_PORT"
  ```
- **Smoke Test & Continuous Fuzzing**:
  ```bash
  ./syzkaller/bin/syz-manager -config $SYZASTER/.workdir/fuzz.cfg -mode smoke-test
  ./syzkaller/bin/syz-manager -config $SYZASTER/.workdir/fuzz.cfg
  ```
