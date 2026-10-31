# SyzAster

**SyzAster** is an end-to-end coverage-guided kernel fuzzing prototype tool designed for the [Asterinas](https://github.com/asterinas/asterinas) Rust framekernel, built on top of [syzkaller](https://github.com/google/syzkaller) and a suite of purpose-built Rust verification, compiler-instrumentation, and evaluation tools.

This document explains how to set up, build, and use **SyzAster** to fuzz the Asterinas kernel (`v0.18.1`). Detailed documentation is also available for specific topics:
- [Asterinas Kernel & NixOS VM Setup](docs/asterinas.md)
- [Framekernel-Aware KCOV Architecture & Verification](docs/kcov.md)
- [Subsystem-Targeted Fuzzing & Ablation](docs/subsystem.md)
- [Experiment & Statistical Evaluation Suite](experiment/README.md)

> [!NOTE]
> Please replace the following environment variables according to your workspace:
> - `$SYZASTER`: directory for saving the SyzAster source code.
> - `$ASTERINAS`: directory for saving the Asterinas kernel source code.
> - `$IMAGE`: directory for saving the Asterinas NixOS VM image, SSH key, and staged `vmlinux`.

## Architecture Overview

Asterinas adopts a **framekernel** architecture that divides the operating system into a minimal, unsafe Trusted Computing Base (**`ostd`**) and a safe OS services layer (**`aster_core`**) written in safe Rust. Directly applying traditional monolithic kernel fuzzing or global SanitizerCoverage instrumentation to Asterinas leads to infinite recursion in the coverage callback, lock-ordering deadlocks, missing debugfs/kallsyms interfaces, and unparsed Rust panic reports.

**SyzAster** addresses these challenges across four tightly integrated components:

- **Framekernel-Selective KCOV Instrumentation (`bin/kcov-rustc`, `bin/check-kcov-inst`)**:
  - Uses a Rust compiler wrapper (`kcov-rustc`) to selectively inject LLVM SanitizerCoverage (`-C passes=sancov-module -C llvm-args=-sanitizer-coverage-level=3 -C llvm-args=-sanitizer-coverage-trace-pc`) **only** when compiling the safe OS services crate (`--crate-name aster_core`) for `x86_64-unknown-none`.
  - Keeps the low-level framekernel runtime (`ostd`), page table manager, interrupt handlers, spinlocks, and the KCOV ring-buffer recorder itself uninstrumented, and disables cross-crate LTO (`CARGO_PROFILE_DEV_LTO=false`, `CARGO_PROFILE_RELEASE_LTO=false`) so LLVM cannot inline instrumented callbacks across the TCB boundary.
- **Task-Scoped `/dev/kcov` & `/proc/kallsyms` (`patch/asterinas/v0.18.1/`)**:
  - Implements a naked `__sanitizer_cov_trace_pc` trampoline and non-reentrant `record_pc` runtime in `ostd::kcov` that records caller return addresses only in task context (`InterruptLevel::current().is_task_context()`) using `try_lock()` and shared anonymous `Vmo` pages.
  - Exposes a Linux-compatible `/dev/kcov` character device (`KCOV_INIT_TRACE`, `KCOV_ENABLE`, `KCOV_DISABLE`, `mmap`) in `aster_core` and `/proc/kallsyms` (`_stext`, `_etext`) for `syz-manager` symbolization.
- **Syzkaller Adaptations for Asterinas (`patch/syzkaller/`)**:
  - `0001`: Adds `open_kcov()` in `syz-executor` to fall back from `/sys/kernel/debug/kcov` to `/dev/kcov` on `ENOENT`, and relaxes optional netfilter/netns checks unsupported by Asterinas.
  - `0002`: Extends `pkg/cover/backend` and `pkg/vminfo` to fall back to `__stext` when `_stext` is absent and tolerate missing `/proc/kallsyms` when `cover: false`.
  - `0003`: Extends `pkg/report` to recognize Asterinas `Uncaught panic:` stack traces and extract the panic message and source location (`file:LINE`) into deduplicated crash titles.
  - `0004`: Adds Asterinas QEMU `mgrconfig` test fixtures and setup documentation.
  - `0005`: Adds adaptive snapshot restoration with four actions (continue, checkpoint save, soft reboot, hard reboot), random snapshot selection on soft reboot, and TSC calibration.
- **Rust Verification & Statistical Evaluation Suite (`src/` -> `bin/`)**:
  - Zero-external-dependency Rust library (`src/pkg/`) and CLI binaries (`syzaster`, `check-asterinas-kcov`, `check-kcov-inst`, `kcov-rustc`, `eval-fuzz`) paired with standalone operational shell scripts (`scripts/`) for image creation, QEMU launching, crash triage, and reproduction.

## Build the Docker Image

We recommend running **SyzAster** inside its self-contained Docker container, which pre-installs Ubuntu 24.04, Rust `nightly-2026-07-21`, Go `1.26.3`, LLVM 19, FlatBuffers, Nix + `nixos-install-tools`, OVMF UEFI firmware, QEMU, and `linux_vdso`:

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
    --name syzaster-test \
    syzaster:latest tail -f /dev/null
docker exec -it syzaster-test bash
```

We recommend storing fuzzers, kernels, VM images, and fuzzing workdirs under the mounted `/vol` directory for persistence.

## Build and Run SyzAster

This section walks through building SyzAster, patching Asterinas `v0.18.1`, building the NixOS VM image, verifying KCOV instrumentation, and launching coverage-guided fuzzing.

### Download SyzAster

Clone SyzAster together with its `syzkaller` submodule:
```bash
export SYZASTER=/vol/SyzAster
git clone --recurse-submodules https://github.com/asterinas/SyzAster $SYZASTER
# If cloned without --recurse-submodules, run:
git -C $SYZASTER submodule update --init --recursive
```

### Build SyzAster and Patched Syzkaller

Build the SyzAster Rust tools into `./bin/` and run the Rust regression test suite:
```bash
cd $SYZASTER
make
make test
```

Apply the SyzAster patch series (`patch/syzkaller/*.patch`) to the `syzkaller` submodule and build `syzkaller`:
```bash
cd $SYZASTER
make syzkaller
```

> [!TIP]
> You can also apply or revert the `syzkaller` patch series independently at any time via `make patch-syzkaller` and `make unpatch-syzkaller`.

### Prepare and Patch Asterinas Kernel (`v0.18.1`)

Use `scripts/update-kernel.sh` to clone Asterinas, check out `v0.18.1` (`d924a9635a66c7c3bb43e563eaafa2c61d6ee9d5`), and apply `patch/asterinas/v0.18.1/*.patch`:
```bash
export ASTERINAS=/vol/asterinas-v0.18.1
$SYZASTER/scripts/update-kernel.sh -d /vol
```

Or apply the patches manually to an existing Asterinas checkout:
```bash
git -C $ASTERINAS checkout --detach d924a9635a66c7c3bb43e563eaafa2c61d6ee9d5
git -C $ASTERINAS apply $SYZASTER/patch/asterinas/v0.18.1/*.patch
```

Before building the full kernel, verify that the Rust compiler and `kcov-rustc` wrapper emit `__sanitizer_cov_trace_pc` callbacks for `aster_core` while leaving `ostd` uninstrumented:
```bash
$SYZASTER/bin/check-kcov-inst --asterinas $ASTERINAS
```

### Build the Asterinas NixOS Image

Use `scripts/asterinas/create-image.sh` to compile the KCOV-instrumented Asterinas kernel (`KCOV=1`), build the NixOS disk image with Dropbear SSH and stage-2 init, and stage the unstripped `vmlinux` ELF (`target/x86_64-unknown-none/debug/asterinas-osdk-bin`):
```bash
export IMAGE=/vol/images/asterinas-v0.18.1
$SYZASTER/scripts/asterinas/create-image.sh \
    --asterinas $ASTERINAS \
    --out $IMAGE \
    --version 0.18.1
```

The script produces:
- `$IMAGE/asterinas.img`: Bootable UEFI/GRUB NixOS disk image with passwordless root SSH.
- `$IMAGE/asterinas.id_rsa`: SSH private key matching the guest's `/root/.ssh/authorized_keys`.
- `$IMAGE/kernel/vmlinux`: Unstripped kernel ELF verified to contain `.symtab`, `.debug_info`, and `__sanitizer_cov_trace_pc`.

> [!IMPORTANT]
> OSDK's boot bundle may strip or transform the kernel binary. `create-image.sh` always validates and copies the original unstripped Cargo ELF (`target/x86_64-unknown-none/debug/asterinas-osdk-bin`) to `$IMAGE/kernel/vmlinux` for `syz-manager` symbolization and coverage mapping.

### Start Coverage-Guided Fuzzing

Create a `syz-manager` configuration file (or copy and edit [`configs/fuzz/asterinas/kernel.cfg`](configs/fuzz/asterinas/kernel.cfg) by replacing the `__XXX__` placeholders):
```bash
cd $SYZASTER
mkdir -p .workdir
cat <<__EOF__ > .workdir/fuzz.cfg
{
	"name": "asterinas-v0.18.1",
	"target": "linux/amd64",
	"http": "127.0.0.1:56741",
	"workdir": "$SYZASTER/.workdir",
	"image": "$IMAGE/asterinas.img",
	"sshkey": "$IMAGE/asterinas.id_rsa",
	"syzkaller": "$SYZASTER/syzkaller",
	"tag": "asterinas-v0.18.1",
	"sandbox": "none",
	"cover": true,
	"kernel_obj": "$IMAGE/kernel",
	"kernel_src": "$ASTERINAS",
	"kernel_build_src": "$ASTERINAS",
	"experimental": {
		"remote_cover": false
	},
	"reproduce": false,
	"snapshot": false,
	"procs": 2,
	"disable_syscalls": [
		"add_key",
		"bpf",
		"io_uring_enter",
		"io_uring_register",
		"io_uring_setup",
		"keyctl",
		"openat\$auto_fuse_conn_congestion_threshold_ops_control",
		"perf_event_open",
		"request_key",
		"syz_init_net_socket",
		"syz_io_uring_*",
		"syz_kvm_*",
		"syz_mount_image",
		"syz_read_part_table",
		"syz_socket_connect_nvme_tcp",
		"syz_usb_*"
	],
	"type": "qemu",
	"vm": {
		"count": 1,
		"cpu": 2,
		"mem": 8192,
		"qemu": "qemu-system-x86_64",
		"qemu_args": "-enable-kvm -cpu host -machine q35,kernel-irqchip=split -bios /usr/share/qemu/OVMF.fd -device virtio-blk-pci,drive=hd0,disable-legacy=on,disable-modern=off -device isa-debug-exit,iobase=0xf4,iosize=0x04",
		"image_device": "drive if=none,format=raw,id=hd0,file=",
		"network_device": "virtio-net-pci,disable-legacy=on,disable-modern=off,csum=off,ctrl_guest_offloads=off,ctrl_mac_addr=off,ctrl_rx_extra=off,ctrl_rx=off,ctrl_vlan=off,ctrl_vq=off,event_idx=off,guest_announce=off,guest_csum=off,guest_ecn=off,guest_tso4=off,guest_tso6=off,guest_ufo=off,guest_uso4=off,guest_uso6=off,host_ecn=off,host_tso4=off,host_tso6=off,host_ufo=off,host_uso=off,indirect_desc=off,mrg_rxbuf=off,queue_reset=off",
		"snapshot": true
	}
}
__EOF__
```

Run the `syz-manager` smoke test first to confirm VM boot and executor communication:
```bash
./syzkaller/bin/syz-manager -config=./.workdir/fuzz.cfg -mode=smoke-test
```

Then start continuous coverage-guided fuzzing:
```bash
./syzkaller/bin/syz-manager -config=./.workdir/fuzz.cfg
```

### Subsystem-Targeted Fuzzing

SyzAster provides pre-configured syscall profiles under [`configs/fuzz/asterinas/`](configs/fuzz/asterinas/) for targeting specific Asterinas subsystems (`fs`, `net`, `process`, `mm`) as well as an uninstrumented baseline (`nocover.cfg`). You can merge the base config with any subsystem profile using `jq`:
```bash
jq -s 'add' ./.workdir/fuzz.cfg configs/fuzz/asterinas/fs.cfg > ./.workdir/fs-fuzz.cfg
./syzkaller/bin/syz-manager -config=./.workdir/fs-fuzz.cfg
```

See [docs/subsystem.md](docs/subsystem.md) for the complete list of targeted syscall families.

## Utility Tools

### Rust Tools (`./bin/`)

All core verification, compiler-wrapping, and statistical evaluation tools are written in Rust (`src/`) and built into `./bin/` via `make`:

- **`bin/syzaster`**: Unified CLI entry point dispatching to `check-kcov`, `check-inst`, `kcov-rustc`, and `eval-fuzz`:
  ```bash
  ./bin/syzaster --help
  ```
- **`bin/check-kcov-inst`**: Verifies that the Rust compiler and `kcov-rustc` wrapper emit `__sanitizer_cov_trace_pc` instrumentation for `aster_core` while keeping `ostd` uninstrumented:
  ```bash
  ./bin/check-kcov-inst --asterinas $ASTERINAS
  ```
- **`bin/check-asterinas-kcov`**: Compiles the static KCOV ABI test (`tools/kcov-smoke.c`), uploads it over SSH to a booted Asterinas VM, and verifies nonzero kernel PCs within `[_stext, _etext)`, buffer bounds, task ownership, and exit cleanup:
  ```bash
  ./bin/check-asterinas-kcov $ASTERINAS $IMAGE/asterinas.id_rsa 10022
  ```
- **`bin/kcov-rustc`**: Rust compiler wrapper used during `KCOV=1` kernel builds to selectively instrument `aster_core`.
- **`bin/eval-fuzz`**: Statistical evaluation tool that parses fuzzing campaign logs and crash directories, computing min/max/median/mean coverage, unique crashes, Vargha-Delaney $\hat{A}_{12}$ effect sizes, and Mann-Whitney U test $p$-values:
  ```bash
  ./bin/eval-fuzz --expdir /vol/experiment --master SyzAster --format markdown
  ```
  Flags of `eval-fuzz`:
  - `-e`, `--expdir`: directory containing fuzzing experiment outputs (default: `/vol/experiment`)
  - `-m`, `--master`: primary fuzzer name for statistical comparison (default: `SyzAster`)
  - `-f`, `--format`: output table format (`csv` or `markdown`, default: `csv`)
  - `--filter`: filter out noisy crashes prior to counting unique bug titles

### Helper Shell Scripts (`./scripts/`)

- **`scripts/update-kernel.sh`**: Clones/checks out Asterinas `v0.18.1` and applies `patch/asterinas/v0.18.1/*.patch`:
  ```bash
  ./scripts/update-kernel.sh -d /vol
  ```
- **`scripts/asterinas/create-image.sh`**: Builds the Asterinas NixOS VM image, SSH keypair, and staged `kernel/vmlinux`.
- **`scripts/asterinas/qemu.sh`**: Boots an Asterinas NixOS image in QEMU with OVMF and SSH port forwarding:
  ```bash
  ./scripts/asterinas/qemu.sh -i $IMAGE/asterinas.img -p 10022
  ```
- **`scripts/asterinas/exec-c.sh` & `scripts/asterinas/exec-syz.sh`**: Compiles/uploads and executes a `.c` or `.syz` reproducer inside a running Asterinas VM over SSH:
  ```bash
  REPRO=/path/to/repro.c SSHKEY=$IMAGE/asterinas.id_rsa PORT=10022 ./scripts/asterinas/exec-c.sh
  REPRO=/path/to/repro.syz SSHKEY=$IMAGE/asterinas.id_rsa PORT=10022 ./scripts/asterinas/exec-syz.sh
  ```
- **`scripts/csum.sh` & `scripts/logs.sh` & `scripts/repro.sh`**: Summarize crash descriptions into CSV, locate crash logs by title, and run `syz-repro` jobs in parallel:
  ```bash
  ./scripts/csum.sh -d ./.workdir/crashes -o ./.workdir/crashes.csv
  ./scripts/logs.sh -d ./.workdir/crashes -i interest.txt
  ./scripts/repro.sh -c configs/repro/reprosh.cfg -j 4
  ```

## Reproduce Evaluation

Please follow [`experiment/docs/setup-env.md`](experiment/docs/setup-env.md) to set up the evaluation directory structure (`fuzzer/`, `kernel/`, `image/`, `experiment/`). You can then launch multi-instance whole-kernel, subsystem (`fs`, `net`, `process`, `mm`), and ablation (`KCOV=1` vs. `KCOV=0`) fuzzing campaigns using our [Docker Compose configurations](experiment/docs/docker-compose.md).
