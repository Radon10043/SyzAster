FROM ubuntu:24.04

SHELL [ "/bin/bash", "-c" ]

ARG DEBIAN_FRONTEND=noninteractive
ARG ASTER_RUST_VERSION=nightly-2026-07-21
ARG NIXPKGS_COMMIT=fd1462031fdee08f65fd0b4c6b64e22239a77870

ENV GOROOT=/usr/local/go
ENV GOPATH=/root/go
ENV CARGO_HOME=/root/.cargo
ENV RUSTUP_HOME=/root/.rustup
ENV LLVM_HOME=/llvm-19
ENV VDSO_LIBRARY_DIR=/root/linux_vdso
ENV PATH=$CARGO_HOME/bin:$GOROOT/bin:$GOPATH/bin:$LLVM_HOME/bin:/root/.nix-profile/bin:/nix/var/nix/profiles/default/bin:$PATH
ENV TZ=Asia/Shanghai
ENV LANG=en_US.UTF-8
ENV LC_CTYPE=en_US.UTF-8

# Install basic packages and Asterinas + Syzkaller build/runtime dependencies
RUN apt-get update
RUN DEBIAN_FRONTEND=noninteractive apt-get install -y tzdata
RUN apt-get install -y sudo wget curl git vim build-essential make gcc g++ clang clang-format flex bison bc debootstrap
RUN apt-get install -y qemu-system-x86 ovmf xz-utils cmake gdb libdw-dev libssl-dev sqlite3 libsqlite3-dev
RUN apt-get install -y rsync file devscripts equivs gnupg config-package-dev debhelper-compat dwarves unzip zip
RUN apt-get install -y gettext ccache git-email cpio gawk lsb-release software-properties-common
RUN apt-get install -y cloc openssh-server openssh-client python3 python3-pip python3-dev python3-yaml language-pack-en-base jq
RUN apt-get install -y pkg-config libzstd-dev libxml2-dev zlib1g-dev sshfs netcat-openbsd cpuid socat
RUN apt-get install -y debian-archive-keyring lsd ripgrep repo bubblewrap net-tools pciutils iproute2 iptables bridge-utils strace
RUN apt-get install -y grub-efi-amd64 grub-efi-amd64-bin grub-efi-amd64-dbg grub2-common mtools xorriso
RUN apt-get install -y parted fdisk util-linux udev dosfstools e2fsprogs exfatprogs openssl binutils libnuma1 libpixman-1-dev

# Ensure /usr/share/qemu/OVMF.fd exists for QEMU UEFI boot
RUN mkdir -p /usr/share/qemu && \
    if [ ! -e /usr/share/qemu/OVMF.fd ]; then \
        if [ -f /usr/share/ovmf/OVMF.fd ]; then \
            ln -s /usr/share/ovmf/OVMF.fd /usr/share/qemu/OVMF.fd; \
        elif [ -f /usr/share/OVMF/OVMF_CODE_4M.fd ]; then \
            ln -s /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/qemu/OVMF.fd; \
        fi; \
    fi

# Install Go 1.26.3 (required by syzkaller)
RUN wget -q --show-progress --progress=bar:force:noscroll https://go.dev/dl/go1.26.3.linux-amd64.tar.gz && \
    tar -C /usr/local -xzf go1.26.3.linux-amd64.tar.gz && \
    rm go1.26.3.linux-amd64.tar.gz

# Install FlatBuffers 23.5.26 (required by syzkaller RPC)
RUN wget -q --show-progress --progress=bar:force:noscroll https://github.com/google/flatbuffers/archive/refs/tags/v23.5.26.tar.gz && \
    tar -xzf v23.5.26.tar.gz && \
    pushd flatbuffers-23.5.26 && \
    cmake -G "Unix Makefiles" && \
    make -j"$(nproc)" && \
    make install && \
    popd && \
    rm -rf v23.5.26.tar.gz flatbuffers-23.5.26

# Install LLVM 19
RUN wget -q --show-progress --progress=bar:force:noscroll https://github.com/llvm/llvm-project/releases/download/llvmorg-19.1.7/LLVM-19.1.7-Linux-X64.tar.xz && \
    mkdir /llvm-19 && \
    tar -C /llvm-19 -xf LLVM-19.1.7-Linux-X64.tar.xz --strip-components=1 && \
    rm LLVM-19.1.7-Linux-X64.tar.xz

# Install Rust toolchain (pinned to Asterinas rust-toolchain.toml) and SyzAster dependencies
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- --default-toolchain ${ASTER_RUST_VERSION} -y && \
    rustup component add rust-src rustc-dev llvm-tools-preview && \
    rustup target add \
        x86_64-unknown-none \
        riscv64imac-unknown-none-elf \
        loongarch64-unknown-none-softfloat \
        aarch64-unknown-none-softfloat && \
    cargo install --locked cargo-binutils && \
    rm -rf /root/.cargo/registry /root/.cargo/git

# Download Linux VDSO library required for building Asterinas
RUN git clone --progress https://github.com/asterinas/linux_vdso.git /root/linux_vdso && \
    git -C /root/linux_vdso checkout 8af5717

# Install Nix and nixos-install-tools (pinned to NixOS 26.05 matching distro/nixpkgs.nix)
RUN mkdir -p /etc/nix && \
    printf "build-users-group =\nfilter-syscalls = false\nexperimental-features = nix-command flakes\n" > /etc/nix/nix.conf && \
    sh <(curl -L https://nixos.org/nix/install) --no-daemon --yes && \
    . /root/.nix-profile/etc/profile.d/nix.sh && \
    nix-channel --add https://github.com/NixOS/nixpkgs/archive/${NIXPKGS_COMMIT}.tar.gz nixpkgs && \
    nix-channel --add https://github.com/NixOS/nixpkgs/archive/${NIXPKGS_COMMIT}.tar.gz nixos && \
    nix-channel --update && \
    nix-env -iA nixpkgs.nixos-install-tools

# Install fzf and lazygit for interactive debugging
RUN git clone --progress --depth 1 https://github.com/junegunn/fzf.git /root/.fzf && \
    yes | /root/.fzf/install
RUN go install github.com/jesseduffield/lazygit@latest
