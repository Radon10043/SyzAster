#!/usr/bin/env bash

# Compile and execute a C reproducer inside a running Asterinas VM over SSH.
#
# usage:
#   REPRO=/path/to/repro.c SSHKEY=/path/to/asterinas.id_rsa [PORT=10022] ./scripts/asterinas/exec-c.sh

set -euo pipefail

PORT="${PORT:-10022}"
HOST="${HOST:-127.0.0.1}"

if [[ -z "${REPRO:-}" || -z "${SSHKEY:-}" ]]; then
    echo "usage: REPRO=/path/to/repro.c SSHKEY=/path/to/asterinas.id_rsa [PORT=10022] $0"
    exit 1
fi

TMP_BIN=$(mktemp /tmp/repro-bin.XXXXXX)
trap 'rm -f "$TMP_BIN"' EXIT

gcc -static -pthread -O2 "$REPRO" -o "$TMP_BIN"

SSH_OPTS=(
    -F /dev/null
    -o UserKnownHostsFile=/dev/null
    -o IdentitiesOnly=yes
    -o BatchMode=yes
    -o StrictHostKeyChecking=no
    -o ConnectTimeout=10
    -i "$SSHKEY"
)

scp -O -P "$PORT" "${SSH_OPTS[@]}" "$TMP_BIN" "root@${HOST}:/tmp/repro.out"
ssh -p "$PORT" "${SSH_OPTS[@]}" "root@${HOST}" "chmod +x /tmp/repro.out && /tmp/repro.out"
