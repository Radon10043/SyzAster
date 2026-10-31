# Setup Environment File (`compose.env`)

> [!WARNING]
> Ensure that `EXPERIMENT_ROOT_CONTAINER` matches the path used when building the Asterinas kernel and staging `kernel/vmlinux` so that `syz-manager` can symbolize PCs and render source-level coverage reports accurately.

(host) Create `compose.env`:
```bash
cat <<EOF > compose.env
EXPERIMENT_CPUS=4
EXPERIMENT_VMCOUNT=2
EXPERIMENT_ROOT_HOST=/vol/workdir-SyzAster
EXPERIMENT_ROOT_CONTAINER=/vol/workdir-SyzAster
EOF
```
