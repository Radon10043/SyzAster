#!/usr/bin/env bash

# QEMU startup script for Asterinas NixOS images using the same VM options as syzkaller.
#
# usage:
#   ./scripts/asterinas/qemu.sh -i /path/to/asterinas.img [-p 10022]

set -euo pipefail

IMAGE=
OVMF="/usr/share/qemu/OVMF.fd"
QEMU_BIN="qemu-system-x86_64"
SSH_PORT=10022
SMP=2
MEM=8192
ENABLE_KVM=1
SNAPSHOT=1

print_help() {
    echo "usage: $0 [ARGS]"
    echo "  args (required):"
    echo "    -i, --image <IMAGE>     path to the asterinas.img file"
    echo "  args (optional):"
    echo "    -p, --port <PORT>       host port forwarded to guest SSH port 22 (default: 10022)"
    echo "    --ovmf <FILE>           OVMF firmware path (default: /usr/share/qemu/OVMF.fd)"
    echo "    --qemu-bin <BIN>        QEMU binary (default: qemu-system-x86_64)"
    echo "    --cpu <N>               vCPU count (default: 2)"
    echo "    --mem <MB>              memory in MB (default: 8192)"
    echo "    --no-kvm                use TCG instead of KVM"
    echo "    --no-snapshot           write changes back to the disk image"
    echo "    -h, --help              print help message"
}

while [[ $# -gt 0 ]]; do
    case $1 in
    -i | --image)
        IMAGE=$(realpath "$2")
        shift 2
        ;;
    -p | --port)
        SSH_PORT="$2"
        shift 2
        ;;
    --ovmf)
        OVMF="$2"
        shift 2
        ;;
    --qemu-bin)
        QEMU_BIN="$2"
        shift 2
        ;;
    --cpu)
        SMP="$2"
        shift 2
        ;;
    --mem)
        MEM="$2"
        shift 2
        ;;
    --no-kvm)
        ENABLE_KVM=0
        shift
        ;;
    --no-snapshot)
        SNAPSHOT=0
        shift
        ;;
    -h | --help)
        print_help
        exit 0
        ;;
    *)
        echo "unknown arg: $1"
        exit 1
        ;;
    esac
done

if [[ -z "$IMAGE" ]]; then
    echo "error: missing required -i/--image argument."
    print_help
    exit 1
fi
if [[ ! -f "$IMAGE" ]]; then
    echo "error: '$IMAGE' does not exist."
    exit 1
fi

QEMU_ARGS=(
    -m "$MEM"
    -smp "$SMP"
    -display none
    -serial stdio
    -no-reboot
    -machine "q35,kernel-irqchip=split"
    -bios "$OVMF"
    -device "virtio-blk-pci,drive=hd0,disable-legacy=on,disable-modern=off"
    -drive "if=none,format=raw,id=hd0,file=$IMAGE"
    -device "isa-debug-exit,iobase=0xf4,iosize=0x04"
    -netdev "user,id=net0,hostfwd=tcp:127.0.0.1:${SSH_PORT}-:22"
    -device "virtio-net-pci,netdev=net0,disable-legacy=on,disable-modern=off,csum=off,ctrl_guest_offloads=off,ctrl_mac_addr=off,ctrl_rx_extra=off,ctrl_rx=off,ctrl_vlan=off,ctrl_vq=off,event_idx=off,guest_announce=off,guest_csum=off,guest_ecn=off,guest_tso4=off,guest_tso6=off,guest_ufo=off,guest_uso4=off,guest_uso6=off,host_ecn=off,host_tso4=off,host_tso6=off,host_ufo=off,host_uso=off,indirect_desc=off,mrg_rxbuf=off,queue_reset=off"
)

if [[ "$ENABLE_KVM" -eq 1 ]]; then
    QEMU_ARGS+=(-enable-kvm -cpu host)
else
    QEMU_ARGS+=(-cpu qemu64)
fi

if [[ "$SNAPSHOT" -eq 1 ]]; then
    QEMU_ARGS+=(-snapshot)
fi

"$QEMU_BIN" "${QEMU_ARGS[@]}" 2>&1 | tee vm.log
