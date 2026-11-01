#!/usr/bin/env bash
# Copyright 2026 syzkaller project authors. All rights reserved.
# Use of this source code is governed by Apache 2 LICENSE that can be found in the LICENSE file.

# create-image.sh builds an Asterinas NixOS disk image, SSH keypair, and staged
# unstripped kernel ELF (vmlinux) usable by syz-manager's QEMU VM backend.

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SYZASTER_DIR=$(cd "${SCRIPT_DIR}/../.." && pwd)

ASTERINAS_DIR=${ASTERINAS_DIR:-}
OUT_DIR=${OUT_DIR:-"${SYZASTER_DIR}/.workdir"}
ASTERINAS_VERSION=${ASTERINAS_VERSION:-0.18.1}
DISK_SIZE_MB=${NIXOS_DISK_SIZE_IN_MB:-16384}
FORCE=0
ALLOW_VERSION_MISMATCH=0
KEEP_NIX_CONFIG=0
COVER=true
KCOV=1

usage() {
	cat <<EOF
Usage: $0 --asterinas DIR [options]

Options:
  --asterinas DIR              Asterinas source tree to build.
  --out DIR                    Output directory (default: ${OUT_DIR}).
  --version VERSION            Required Asterinas VERSION (default: ${ASTERINAS_VERSION}).
  --disk-size-mb MB            Asterinas NixOS disk size (default: ${DISK_SIZE_MB}).
  --no-cover                   Build an uninstrumented image (KCOV=0) without staging vmlinux.
  --force                      Recreate the Asterinas target/nixos image.
  --allow-version-mismatch     Build even if VERSION or the release commit differs.
  --keep-nix-config            Keep the generated NixOS config in the Asterinas tree.
  -h, --help                   Show this help.

The script writes:
  OUT/asterinas.img
  OUT/asterinas.id_rsa
  OUT/kernel/vmlinux           Exact unstripped kernel ELF (unless --no-cover).

Coverage is enabled by default and requires the Asterinas KCOV patch.
For syz-manager configuration templates, see configs/fuzz/asterinas/.
EOF
}

need_arg() {
	if [[ -z "${2:-}" || "${2}" == --* ]]; then
		echo "$1 requires an argument" >&2
		usage >&2
		exit 1
	fi
}

while (($#)); do
	case "$1" in
	--asterinas)
		need_arg "$1" "${2-}"
		ASTERINAS_DIR=$2
		shift 2
		;;
	--out)
		need_arg "$1" "${2-}"
		OUT_DIR=$2
		shift 2
		;;
	--version)
		need_arg "$1" "${2-}"
		ASTERINAS_VERSION=${2#v}
		shift 2
		;;
	--disk-size-mb)
		need_arg "$1" "${2-}"
		DISK_SIZE_MB=$2
		shift 2
		;;
	--no-cover)
		COVER=false
		KCOV=0
		shift
		;;
	--force)
		FORCE=1
		shift
		;;
	--allow-version-mismatch)
		ALLOW_VERSION_MISMATCH=1
		shift
		;;
	--keep-nix-config)
		KEEP_NIX_CONFIG=1
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		echo "unknown option: $1" >&2
		usage >&2
		exit 1
		;;
	esac
done

if [[ -z "${ASTERINAS_DIR}" ]]; then
	echo "--asterinas is required" >&2
	exit 1
fi
ASTERINAS_VERSION=${ASTERINAS_VERSION#v}
if [[ -z "${ASTERINAS_VERSION}" ]]; then
	echo "--version must not be empty" >&2
	exit 1
fi

require_cmd() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "missing required command: $1" >&2
		exit 1
	fi
}

require_uint_range() {
	local name=$1
	local value=$2
	local min=$3
	local max=$4
	if [[ ! "${value}" =~ ^[0-9]+$ ]]; then
		echo "${name} must be an integer, got: ${value}" >&2
		exit 1
	fi
	local number=$((10#${value}))
	if ((number < min || number > max)); then
		echo "${name} must be in [${min}, ${max}], got: ${value}" >&2
		exit 1
	fi
}

require_cmd make
require_cmd realpath
require_cmd ssh-keygen
require_cmd cp

require_uint_range "--disk-size-mb" "${DISK_SIZE_MB}" 128 1048576

ASTERINAS_DIR=$(realpath "${ASTERINAS_DIR}")
OUT_DIR=$(realpath -m "${OUT_DIR}")

if [[ ! -f "${ASTERINAS_DIR}/VERSION" || ! -d "${ASTERINAS_DIR}/distro/etc_nixos" ]]; then
	echo "${ASTERINAS_DIR} does not look like an Asterinas source tree" >&2
	exit 1
fi

actual_version=$(tr -d '[:space:]' <"${ASTERINAS_DIR}/VERSION")
if [[ "${actual_version}" != "${ASTERINAS_VERSION}" ]]; then
	msg="Asterinas VERSION is ${actual_version}, expected ${ASTERINAS_VERSION}"
	if [[ "${ALLOW_VERSION_MISMATCH}" -eq 0 ]]; then
		echo "${msg}. Use --allow-version-mismatch only for bring-up experiments." >&2
		exit 1
	fi
	echo "warning: ${msg}" >&2
fi

if [[ "${ALLOW_VERSION_MISMATCH}" -eq 0 ]]; then
	require_cmd git
	release_commit=""
	case "${ASTERINAS_VERSION}" in
	"0.18.1")
		release_commit="d924a9635a66c7c3bb43e563eaafa2c61d6ee9d5"
		;;
	"0.19.0")
		release_commit="c4b7ed575b7b32773475b194d2ad41fb6d03765b"
		;;
	esac
	if [[ -n "${release_commit}" ]]; then
		source_root=$(git -C "${ASTERINAS_DIR}" rev-parse --show-toplevel 2>/dev/null || true)
		source_commit=$(git -C "${ASTERINAS_DIR}" rev-parse HEAD 2>/dev/null || true)
		if [[ "${source_root}" != "${ASTERINAS_DIR}" || "${source_commit}" != "${release_commit}" ]]; then
			echo "Asterinas v${ASTERINAS_VERSION} requires release commit ${release_commit}; got ${source_commit:-unverified}. Use --allow-version-mismatch only for bring-up experiments." >&2
			exit 1
		fi
	fi
fi

mkdir -p "${OUT_DIR}"
SSH_KEY="${OUT_DIR}/asterinas.id_rsa"
IMAGE="${OUT_DIR}/asterinas.img"

# OSDK builds the generated asterinas-osdk-bin crate, then may strip or
# transform its boot/bundle copy. Always use the original Cargo ELF instead.
KERNEL_ELF="${ASTERINAS_DIR}/target/x86_64-unknown-none/debug/asterinas-osdk-bin"
STAGED_KERNEL_OBJ="${OUT_DIR}/kernel"

stage_kernel() {
	if [[ ! -f "${KERNEL_ELF}" ]]; then
		echo "coverage requested but matching unstripped kernel ELF is missing: ${KERNEL_ELF}. Build with KCOV=1 or use --no-cover for an uninstrumented image." >&2
		exit 1
	fi
	require_cmd readelf
	local sections
	if ! sections=$(readelf -SW "${KERNEL_ELF}") ||
		[[ "${sections}" != *".symtab"* || "${sections}" != *".debug_info"* ]]; then
		echo "coverage requires an unstripped kernel ELF with debug info: ${KERNEL_ELF}" >&2
		exit 1
	fi
	local symbols
	if ! symbols=$(readelf -sW "${KERNEL_ELF}") ||
		! awk '$7 != "UND" && $8 == "__sanitizer_cov_trace_pc" { found = 1 } END { exit !found }' <<<"${symbols}"; then
		echo "coverage requires a kernel ELF defining __sanitizer_cov_trace_pc: ${KERNEL_ELF}. Apply patch/asterinas/v${ASTERINAS_VERSION}/*.patch and build with KCOV=1." >&2
		exit 1
	fi
	mkdir -p "${STAGED_KERNEL_OBJ}"
	if [[ ! "${KERNEL_ELF}" -ef "${STAGED_KERNEL_OBJ}/vmlinux" ]]; then
		cp --reflink=auto "${KERNEL_ELF}" "${STAGED_KERNEL_OBJ}/vmlinux" 2>/dev/null ||
			cp "${KERNEL_ELF}" "${STAGED_KERNEL_OBJ}/vmlinux"
	fi
}

if [[ ! -f "${SSH_KEY}" ]]; then
	ssh-keygen -f "${SSH_KEY}" -t rsa -b 2048 -N '' -C '' >/dev/null
fi
chmod 0600 "${SSH_KEY}"

if [[ ! -f "${SSH_KEY}.pub" ]]; then
	ssh-keygen -y -f "${SSH_KEY}" >"${SSH_KEY}.pub"
fi
PUBLIC_KEY=$(<"${SSH_KEY}.pub")

if ! command -v cargo >/dev/null 2>&1 && [[ -x "${HOME:-/root}/.cargo/bin/cargo" ]]; then
	export PATH="${HOME:-/root}/.cargo/bin:${PATH}"
fi
asterinas_image="${ASTERINAS_DIR}/target/nixos/asterinas.img"
USE_PREBUILT_NIXOS_IMAGE=0
if ! command -v nix-build >/dev/null 2>&1 && [[ "${FORCE}" -eq 0 && -f "${asterinas_image}" ]]; then
	USE_PREBUILT_NIXOS_IMAGE=1
	for cmd in cargo find mount openssl umount; do
		require_cmd "${cmd}"
	done
else
	for cmd in blockdev cargo fallocate grub-mkrescue losetup mformat mkfs.exfat mkfs.ext2 mkfs.fat mknod nix-build openssl parted partprobe partx udevadm xorriso; do
		require_cmd "${cmd}"
	done
fi
if [[ -z "${VDSO_LIBRARY_DIR:-}" ]]; then
	echo "VDSO_LIBRARY_DIR is required to build Asterinas" >&2
	exit 1
fi
if [[ ! -f "${VDSO_LIBRARY_DIR}/vdso_x86_64.so" ]]; then
	echo "VDSO_LIBRARY_DIR does not contain vdso_x86_64.so: ${VDSO_LIBRARY_DIR}" >&2
	exit 1
fi
if [[ ! -f "${ASTERINAS_DIR}/osdk/Cargo.toml" ]]; then
	echo "Asterinas source tree does not contain osdk/Cargo.toml" >&2
	exit 1
fi

nix_config_base=".syzkaller-${actual_version}-$$.nix"
nix_config="${ASTERINAS_DIR}/distro/etc_nixos/${nix_config_base}"
ROOT_PASSWORD_HASH=$(openssl rand -hex 32 | openssl passwd -6 -stdin)
cleanup() {
	if [[ "${KEEP_NIX_CONFIG}" -eq 0 ]]; then
		rm -f "${nix_config}"
	fi
}
trap cleanup EXIT

cat >"${nix_config}" <<EOF
{
  config,
  lib,
  pkgs,
  ...
}:
let
  syzkallerDropbear = pkgs.dropbear.override {
    sftpPath = "\${pkgs.openssh}/libexec/sftp-server";
  };
  syzkallerStage2 = pkgs.writeShellScript "syzkaller-stage2" ''
    export PATH=\${lib.makeBinPath [
      pkgs.coreutils
      pkgs.findutils
      pkgs.openssh
      syzkallerDropbear
      pkgs.util-linux
    ]}:\$PATH

    echo "Configuring Asterinas syzkaller stage 2..."

    mkdir -p /proc /sys /dev/pts /run /tmp /root/.ssh /etc/dropbear
    chmod 1777 /tmp
    mount -t proc proc /proc 2>/dev/null || true
    mount -t sysfs sysfs /sys 2>/dev/null || true
    mount -t devpts devpts /dev/pts 2>/dev/null || true

    # Prefer the Linux KCOV path where sysfs permits it. Do not mount debugfs:
    # sysfs may reject mkdir, in which case the executor opens /dev/kcov directly.
    if [ -e /dev/kcov ]; then
      if mkdir -p /sys/kernel/debug 2>/dev/null; then
        ln -s /dev/kcov /sys/kernel/debug/kcov 2>/dev/null || true
      fi
    fi

    chmod 700 /root/.ssh
    cat > /root/.ssh/authorized_keys <<'AUTHORIZED_KEYS'
${PUBLIC_KEY}
AUTHORIZED_KEYS
    chmod 600 /root/.ssh/authorized_keys

    if [ ! -f /etc/dropbear/dropbear_ed25519_host_key ]; then
      dropbearkey -t ed25519 -f /etc/dropbear/dropbear_ed25519_host_key >/dev/null
    fi

    echo "Starting dropbear for syzkaller..."
    exec \${syzkallerDropbear}/bin/dropbear -F -E -p 10.0.2.15:22 -r /etc/dropbear/dropbear_ed25519_host_key -s -g
  '';
in
{
  imports = [ ./aster_configuration.nix ];

  nixpkgs.overlays = lib.mkForce [ (import ./overlays/systemd/default.nix) ];

  aster_nixos.disable-systemd = "true";
  aster_nixos.stage-2-hook = "exec \${syzkallerStage2}";

  networking.hostName = "asterinas-syzkaller";
  networking.resolvconf.enable = false;
  environment.etc."resolv.conf".text = ''
    nameserver 8.8.8.8
  '';

  services.openssh.enable = true;
  services.openssh.settings = {
    PermitRootLogin = "prohibit-password";
    PasswordAuthentication = false;
    KbdInteractiveAuthentication = false;
  };

  users.users.root.openssh.authorizedKeys.keys = [
    "${PUBLIC_KEY}"
  ];
  users.users.root.hashedPassword = lib.mkForce "${ROOT_PASSWORD_HASH}";

  environment.systemPackages = with pkgs; [
    coreutils
    findutils
    gdb
    iproute2
    iputils
    openssh
    procps
    strace
    util-linux
  ];

  system.nixos.distroName = "Asterinas NixOS syzkaller";
  system.stateVersion = "26.05";
}
EOF

if [[ "${FORCE}" -eq 1 ]]; then
	rm -f "${asterinas_image}" "${IMAGE}"
fi

OSDK_ROOT="${OUT_DIR}/osdk"
(
	cd "${ASTERINAS_DIR}"
	OSDK_LOCAL_DEV=1 CARGO_TARGET_DIR="${OUT_DIR}/osdk-target" \
		cargo install --path osdk --root "${OSDK_ROOT}" --locked --force
)
export PATH="${OSDK_ROOT}/bin:${PATH}"

if [[ "${USE_PREBUILT_NIXOS_IMAGE}" -eq 1 ]]; then
	kcov_features=()
	kcov_env=()
	if [[ "${KCOV}" == "1" ]]; then
		kcov_features=(--features=kcov)
		kcov_env=(
			RUSTC_WRAPPER="${ASTERINAS_DIR}/tools/kcov-rustc.sh"
			CARGO_PROFILE_DEV_LTO=false
			CARGO_PROFILE_RELEASE_LTO=false
		)
	fi
	(
		cd "${ASTERINAS_DIR}/kernel"
		env "${kcov_env[@]}" \
			OSDK_TARGET_ARCH=x86_64 \
			CARGO_TARGET_DIR="${ASTERINAS_DIR}/target" \
			"${OSDK_ROOT}/bin/cargo-osdk" build \
			--kcmd-args="loglevel=error" \
			--kcmd-args="earlycon" \
			--kcmd-args="console=ttyS0" \
			--boot-method="qemu-direct" \
			--grub-boot-protocol="linux" \
			"${kcov_features[@]}"
	)
	rm -f "${ASTERINAS_DIR}/ostd/libs/linux-bzimage/setup/Cargo.lock"
	bzimage="${ASTERINAS_DIR}/target/osdk/asterinas/asterinas-osdk-bin"
	if [[ ! -f "${bzimage}" ]]; then
		echo "Asterinas kernel build did not produce ${bzimage}" >&2
		exit 1
	fi
	cp --reflink=auto "${asterinas_image}" "${IMAGE}" 2>/dev/null || cp "${asterinas_image}" "${IMAGE}"
	mnt_dir=$(mktemp -d "${OUT_DIR}/.mnt.XXXXXX")
	cleanup_mnt() {
		umount "${mnt_dir}" 2>/dev/null || true
		rmdir "${mnt_dir}" 2>/dev/null || true
		cleanup
	}
	trap cleanup_mnt EXIT
	mount -o loop,offset=1048576,sizelimit=510656512 "${IMAGE}" "${mnt_dir}"
	for f in "${mnt_dir}"/kernels/*-asterinas-osdk-bin; do
		if [[ -f "${f}" ]]; then
			cp -f "${bzimage}" "${f}"
		fi
	done
	sync
	umount "${mnt_dir}"

	mount -o loop,offset=511705088 "${IMAGE}" "${mnt_dir}"
	for d in "${mnt_dir}"/nix/store/*-asterinas-osdk-bin; do
		if [[ -d "${d}" && -f "${d}/bzImage" ]]; then
			cp -f "${bzimage}" "${d}/bzImage"
		fi
	done
	mkdir -p "${mnt_dir}/root/.ssh"
	chmod 700 "${mnt_dir}/root/.ssh"
	printf '%s\n' "${PUBLIC_KEY}" >"${mnt_dir}/root/.ssh/authorized_keys"
	chmod 600 "${mnt_dir}/root/.ssh/authorized_keys"
	for f in "${mnt_dir}"/nix/store/*-syzkaller-stage2; do
		if [[ -f "${f}" ]]; then
			python3 - "${f}" "${PUBLIC_KEY}" <<'PY'
import pathlib, re, sys
path = pathlib.Path(sys.argv[1])
pub = sys.argv[2]
text = path.read_text()
text = re.sub(
    r"cat > /root/\.ssh/authorized_keys <<'AUTHORIZED_KEYS'\n.*?\nAUTHORIZED_KEYS",
    f"cat > /root/.ssh/authorized_keys <<'AUTHORIZED_KEYS'\n{pub}\nAUTHORIZED_KEYS",
    text,
    flags=re.DOTALL,
)
if "/dev/kcov" not in text:
    text = text.replace(
        "mount -t devpts devpts /dev/pts 2>/dev/null || true\n",
        "mount -t devpts devpts /dev/pts 2>/dev/null || true\n"
        "if [ -e /dev/kcov ]; then\n"
        "  if mkdir -p /sys/kernel/debug 2>/dev/null; then\n"
        "    ln -s /dev/kcov /sys/kernel/debug/kcov 2>/dev/null || true\n"
        "  fi\n"
        "fi\n",
    )
path.write_text(text)
PY
			chmod +x "${f}"
		fi
	done
	sync
	umount "${mnt_dir}"
	rmdir "${mnt_dir}"
	trap cleanup EXIT
else
	CARGO_TARGET_DIR="${ASTERINAS_DIR}/target" make -C "${ASTERINAS_DIR}" kernel \
		CARGO_OSDK="${OSDK_ROOT}/bin/cargo-osdk" \
		KCOV="${KCOV}" \
		RELEASE=0 RELEASE_LTO=0 \
		TARGET_ARCH=x86_64 \
		BOOT_PROTOCOL=linux-efi-handover64 \
		CONSOLE=ttyS0 \
		LOG_LEVEL=error

	NIXOS_DISK_SIZE_IN_MB="${DISK_SIZE_MB}" \
		NIXOS_DISABLE_SYSTEMD=false \
		NIXOS_STAGE_2_INIT="/bin/sh -l" \
		LOG_LEVEL=error \
		TARGET_ARCH=x86_64 \
		CONSOLE=ttyS0 \
		"${ASTERINAS_DIR}/tools/nixos/build_nixos.sh" "${nix_config_base}"

	if [[ ! -f "${asterinas_image}" ]]; then
		echo "Asterinas build did not produce ${asterinas_image}" >&2
		exit 1
	fi
	cp --reflink=auto "${asterinas_image}" "${IMAGE}" 2>/dev/null || cp "${asterinas_image}" "${IMAGE}"
fi

if [[ "${COVER}" == true ]]; then
	stage_kernel
fi

cat <<EOF
Asterinas syzkaller image is ready.

Image:   ${IMAGE}
SSH key: ${SSH_KEY}
EOF
if [[ "${COVER}" == true ]]; then
	echo "Kernel:  ${STAGED_KERNEL_OBJ}/vmlinux"
fi
