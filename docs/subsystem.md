# Subsystem-Targeted Fuzzing & Ablation Guide

In addition to whole-kernel fuzzing (`configs/fuzz/asterinas/kernel.cfg`), **SyzAster** provides pre-configured syscall profiles for targeting key Asterinas subsystems and performing ablation studies (`KCOV=1` vs. `KCOV=0`).

## Subsystem Configurations

Located under `configs/fuzz/asterinas/` and `experiment/configs/asterinas/`:

| Subsystem | Config File | Targeted Syscall Families |
| :--- | :--- | :--- |
| **Whole Kernel** | `configs/fuzz/asterinas/kernel.cfg` | All Asterinas-compatible Linux syscalls (`KCOV=1`) |
| **No-Cover Baseline** | `configs/fuzz/asterinas/nocover.cfg` | All Asterinas-compatible Linux syscalls (`KCOV=0`, `cover: false`) |
| **Filesystem (`fs`)** | `configs/fuzz/asterinas/fs.cfg` | VFS, `ext2`, `exfat`, `ramfs`, `procfs`, `sysfs`, `epoll`, `inotify`, `pipe`, `eventfd`, `timerfd` |
| **Network (`net`)** | `configs/fuzz/asterinas/net.cfg` | TCP/UDP/Unix sockets, `smoltcp` interface, `bind`, `connect`, `sendmsg`, `recvmsg`, `setsockopt` |
| **Process (`process`)** | `configs/fuzz/asterinas/process.cfg` | `clone`, `fork`, `execve`, `wait4`, `futex`, signals, `ptrace`, credentials, POSIX timers |
| **Memory (`mm`)** | `configs/fuzz/asterinas/mm.cfg` | `VMAR`, `Vmo`, page cache, `mmap`, `munmap`, `mprotect`, `mremap`, `madvise`, `brk`, `memfd_create` |

## Merging a Subsystem Config Manually

You can combine the base `kernel.cfg` with any subsystem config using `jq`:
```bash
jq -s 'add' \
    configs/fuzz/asterinas/kernel.cfg \
    configs/fuzz/asterinas/fs.cfg > .workdir/fs-fuzz.cfg
```

Or run the subsystem campaigns directly via Docker Compose (see [`experiment/docs/docker-compose.md`](../experiment/docs/docker-compose.md)).
