#!/usr/bin/env bash

# Execute a syzkaller (.syz) reproducer inside a running Asterinas VM over SSH.
#
# usage:
#   REPRO=/path/to/repro.syz SSHKEY=/path/to/asterinas.id_rsa [PORT=10022] ./scripts/asterinas/exec-syz.sh

set -euo pipefail

SYZASTER=$(realpath "$(dirname "$0")/../..")
PORT="${PORT:-10022}"
HOST="${HOST:-127.0.0.1}"

if [[ -z "${REPRO:-}" || -z "${SSHKEY:-}" ]]; then
    echo "usage: REPRO=/path/to/repro.syz SSHKEY=/path/to/asterinas.id_rsa [PORT=10022] $0"
    exit 1
fi

SSH_OPTS=(
    -F /dev/null
    -o UserKnownHostsFile=/dev/null
    -o IdentitiesOnly=yes
    -o BatchMode=yes
    -o StrictHostKeyChecking=no
    -o ConnectTimeout=10
    -i "$SSHKEY"
)

scp -O -P "$PORT" "${SSH_OPTS[@]}" "$SYZASTER"/syzkaller/bin/linux_amd64/* "root@${HOST}:/tmp/"
scp -O -P "$PORT" "${SSH_OPTS[@]}" "$REPRO" "root@${HOST}:/tmp/repro.syz"
ssh -p "$PORT" "${SSH_OPTS[@]}" "root@${HOST}" "cd /tmp && ./syz-execprog -enable=all -repeat=0 -procs=8 ./repro.syz"
