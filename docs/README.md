# SyzAster

**SyzAster** is an end-to-end coverage-guided kernel fuzzing prototype tool designed for the [Asterinas](https://github.com/asterinas/asterinas) Rust framekernel, built on top of [syzkaller](https://github.com/google/syzkaller) and a suite of purpose-built Rust orchestration, verification, and evaluation tools.

This document explains how to set up, build, and run **SyzAster** to fuzz Asterinas (`v0.18.1`). Detailed documentation is also available for:
- [Asterinas Kernel & NixOS VM Setup](asterinas.md)
- [Framekernel-Aware KCOV Architecture & Verification](kcov.md)
- [Subsystem-Targeted Fuzzing & Ablation](subsystem.md)
- [Experiment & Statistical Evaluation Suite](../experiment/README.md)

Please replace the following variables according to your environment:
- `$SYZASTER`: directory for saving the SyzAster source code.
- `$ASTERINAS`: directory for saving the Asterinas kernel source code.
- `$WORKDIR`: directory for saving the Asterinas VM image, staged ELF, and fuzzing artifacts.

## Architecture Overview

1. **Framekernel-Selective KCOV Instrumentation**: Using the `kcov-rustc` compiler wrapper, SyzAster selectively instruments the safe OS services layer (`aster_core`) with LLVM SanitizerCoverage (`-C passes=sancov-module -C llvm-args=-sanitizer-coverage-trace-pc`) while keeping the trusted framekernel base (`ostd`), memory allocator, and scheduler uninstrumented and isolated from cross-crate LTO.
2. **Task-Scoped `/dev/kcov` & `/proc/kallsyms`**: Implements Linux-compatible per-thread PC coverage (`KCOV_INIT_TRACE`, `KCOV_ENABLE`, `KCOV_DISABLE`) backed by shared anonymous `Vmo` pages in `ostd::kcov`, alongside `/proc/kallsyms` exporting `_stext` and `_etext` for `syz-manager` symbolization.
3. **Patched Syzkaller (`patch/syzkaller/`)**: Adapts `syz-executor`, `pkg/vminfo`, `pkg/cover`, and `pkg/report` to support `/dev/kcov` fallback without `debugfs`, `__stext` ELF symbol resolution, optional network namespaces, and structured crash parsing for Asterinas `Uncaught panic:` stack traces.
4. **Rust Tooling (`src/` -> `bin/`) & Helper Scripts (`scripts/`)**: Provides a zero-external-dependency Rust CLI suite (`syzaster`, `check-asterinas-kcov`, `check-kcov-inst`, `kcov-rustc`, `eval-fuzz`) alongside standalone shell scripts (`scripts/`) for image creation, kernel checkout, QEMU VM startup, crash triage, and reproduction.

## Directory Structure

```text
SyzAster/
├── Cargo.toml                  # Rust crate manifest for SyzAster tools
├── Makefile                    # Builds Rust tools (bin/) and patched syzkaller
├── syzkaller/                  # Git submodule (google/syzkaller)
├── patch/
│   ├── syzkaller/              # Patch series for syzkaller (0001..0004)
│   └── asterinas/v0.18.1/      # Patch series for Asterinas v0.18.1 (0001..0002)
├── src/                        # Rust library (src/pkg/) and CLI binaries (src/bin/)
├── tests/                      # Rust unit and integration test suites
├── configs/                    # Fuzzing, kernel, and crash reproduction configs
├── scripts/                    # Standalone shell scripts for kernel setup, QEMU, and crash triage
├── docker/                     # Self-contained Dockerfile (Ubuntu 24.04 + Go + Rust + Nix + QEMU)
├── docs/                       # Documentation
└── experiment/                 # Kernel build, Docker Compose, and statistical evaluation suite
```

## Build the Docker Image

We recommend running SyzAster inside its self-contained Docker container:
```bash
docker build -t syzaster:latest --network host -f ./docker/Dockerfile .
```

Start a container and enter it:
```bash
docker run \
    -d \
    -v ./vol:/vol \
    --cpus 16 \
    --network host \
    --privileged \
    --name syzaster-dev \
    syzaster:latest tail -f /dev/null
docker exec -it syzaster-dev bash
```

## Build and Run SyzAster

### 1. Download SyzAster

Clone SyzAster together with its `syzkaller` submodule:
```bash
export SYZASTER=/vol/SyzAster
git clone --recurse-submodules https://github.com/asterinas/SyzAster $SYZASTER
cd $SYZASTER
git submodule update --init --recursive
```

### 2. Build SyzAster Rust Tools and Patched Syzkaller

Build the Rust toolchain into `./bin/` and run the Rust verification suite:
```bash
cd $SYZASTER
make
make test
```

Apply the SyzAster patch series (`patch/syzkaller/*.patch`) to `syzkaller/` and build `syzkaller`:
```bash
cd $SYZASTER
make syzkaller
```

### 3. Prepare and Patch Asterinas Kernel

Use `scripts/update-kernel.sh` (or apply `patch/asterinas/v0.18.1/*.patch` manually) to prepare the Asterinas source tree:
```bash
export ASTERINAS=/vol/asterinas-v0.18.1
$SYZASTER/scripts/update-kernel.sh -d /vol
```

Verify that the Rust compiler and KCOV wrapper instrument `aster_core` and leave `ostd` uninstrumented:
```bash
$SYZASTER/bin/check-kcov-inst --asterinas $ASTERINAS
```

### 4. Build Asterinas NixOS Image

Use `scripts/asterinas/create-image.sh` to build the KCOV-instrumented kernel, generate the NixOS disk image with Dropbear SSH, and stage the unstripped `vmlinux` ELF:
```bash
export WORKDIR=$SYZASTER/.workdir
$SYZASTER/scripts/asterinas/create-image.sh \
    --asterinas $ASTERINAS \
    --out $WORKDIR \
    --version 0.18.1
```

### 5. Configure `syz-manager` and Start Fuzzing

Copy `configs/fuzz/asterinas/kernel.cfg` to `$WORKDIR/fuzz.cfg` and fill in the `__XXX__` placeholders (`__KERNEL_VERSION__`, `__WORKDIR__`, `__IMAGE__`, `__SYZKALLER__`, `__KERNEL__`, `__VMCOUNT__`):
```bash
cp $SYZASTER/configs/fuzz/asterinas/kernel.cfg $WORKDIR/fuzz.cfg
# Edit the __XXX__ placeholders in $WORKDIR/fuzz.cfg, then run smoke test:
$SYZASTER/syzkaller/bin/syz-manager \
    -config $WORKDIR/fuzz.cfg \
    -mode smoke-test
```

Start continuous coverage-guided fuzzing:
```bash
$SYZASTER/syzkaller/bin/syz-manager \
    -config $WORKDIR/fuzz.cfg
```

## Crash Triage, Reproduction, and Evaluation Tools

- **Summarize Crash Reports (`scripts/csum.sh`)**:
  ```bash
  $SYZASTER/scripts/csum.sh -d $WORKDIR/workdir/crashes -o $WORKDIR/crashes.csv
  ```
- **Locate Crash Logs by Title (`scripts/logs.sh`)**:
  ```bash
  $SYZASTER/scripts/logs.sh -d $WORKDIR/workdir/crashes -i interest.txt
  ```
- **Reproduce Crashes in Parallel (`scripts/repro.sh`)**:
  ```bash
  $SYZASTER/scripts/repro.sh -c $SYZASTER/configs/repro/reprosh.cfg -j 4
  ```
- **Launch Asterinas QEMU VM (`scripts/asterinas/qemu.sh`)**:
  ```bash
  $SYZASTER/scripts/asterinas/qemu.sh -i $WORKDIR/asterinas.img -p 10022
  ```
- **Execute C or `.syz` Reproducer in Guest (`scripts/asterinas/exec-c.sh`, `exec-syz.sh`)**:
  ```bash
  REPRO=repro.c SSHKEY=$WORKDIR/asterinas.id_rsa PORT=10022 $SYZASTER/scripts/asterinas/exec-c.sh
  REPRO=repro.syz SSHKEY=$WORKDIR/asterinas.id_rsa PORT=10022 $SYZASTER/scripts/asterinas/exec-syz.sh
  ```
- **Statistical Campaign Evaluation (`bin/eval-fuzz`)**:
  ```bash
  $SYZASTER/bin/eval-fuzz -expdir /vol/experiment -master SyzAster -format markdown
  ```
