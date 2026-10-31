# SyzAster: Coverage-Guided Kernel Fuzzing Prototype Tool for Asterinas Framekernel

**SyzAster** is a Rust-based coverage-guided kernel fuzzing prototype tool tailored for the [Asterinas](https://github.com/asterinas/asterinas) framekernel. It integrates framekernel-selective SanitizerCoverage (`KCOV=1`), a task-scoped `/dev/kcov` device and `/proc/kallsyms` symbol interface, a modular patch series for `syzkaller` and `asterinas`, and a complete suite of Rust CLI tools for image creation, ELF verification, live KCOV ABI testing, crash reproduction, and statistical campaign evaluation.

For full documentation, see [`docs/README.md`](docs/README.md):
- [Main Documentation & Quick Start](docs/README.md)
- [Asterinas Kernel & NixOS VM Setup Guide](docs/asterinas.md)
- [Framekernel-Aware KCOV Design & Verification](docs/kcov.md)
- [Subsystem Fuzzing & Ablation Guide](docs/subsystem.md)
- [Experiment & Evaluation Suite](experiment/README.md)

## Quick Start

```bash
# 1. Build all SyzAster Rust tools (into ./bin/) and run the Rust test suite
make
make test

# 2. Apply the syzkaller patch series (patch/syzkaller/*.patch) and build syzkaller
make syzkaller

# 3. Build Asterinas NixOS image + SSH key + staged vmlinux
./scripts/asterinas/create-image.sh \
    --asterinas /path/to/asterinas \
    --out ./.workdir \
    --version 0.18.1

# 4. Fill in configs/fuzz/asterinas/kernel.cfg and start fuzzing
cp configs/fuzz/asterinas/kernel.cfg ./.workdir/fuzz.cfg
# (Edit __WORKDIR__, __IMAGE__, __SYZKALLER__, __KERNEL__, __VMCOUNT__ in ./.workdir/fuzz.cfg)
./syzkaller/bin/syz-manager -config ./.workdir/fuzz.cfg -mode smoke-test
./syzkaller/bin/syz-manager -config ./.workdir/fuzz.cfg
```
