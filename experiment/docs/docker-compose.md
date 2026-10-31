# Docker Compose Evaluation Guide

## Preparation

Ensure `compose.env` is configured before launching evaluation containers (see [`setup-env_file.md`](setup-env_file.md)):
- `EXPERIMENT_ROOT_HOST`: root directory of the experiment on the host machine
- `EXPERIMENT_ROOT_CONTAINER`: root directory mounted inside the container
- `EXPERIMENT_CPUS`: number of CPUs allocated per container
- `EXPERIMENT_VMCOUNT`: number of QEMU VMs per `syz-manager` instance

## Running Fuzzing Campaigns

Launch a coverage-guided fuzzing campaign with `compose.SyzAster.yaml`:
```bash
docker compose --env-file ./compose.env \
    -f ./experiment/docker-compose/compose.SyzAster.yaml \
    up asterinas-v0.18.1-kernel --scale asterinas-v0.18.1-kernel=5 -d
```

Launch a subsystem-focused fuzzing campaign (`fs`, `net`, `process`, or `mm`):
```bash
docker compose --env-file ./compose.env \
    -f ./experiment/docker-compose/compose.SyzAster.yaml \
    up asterinas-v0.18.1-fs --scale asterinas-v0.18.1-fs=5 -d
```

Launch the baseline uninstrumented (`KCOV=0`) ablation campaign with `compose.SyzAster-nocover.yaml`:
```bash
docker compose --env-file ./compose.env \
    -f ./experiment/docker-compose/compose.SyzAster-nocover.yaml \
    up asterinas-v0.18.1-kernel --scale asterinas-v0.18.1-kernel=5 -d
```

## Statistical Evaluation (`bin/eval-fuzz`)

After campaigns finish, evaluate coverage, unique crashes, Vargha-Delaney $\hat{A}_{12}$ effect sizes, and Mann-Whitney U test p-values:
```bash
./bin/eval-fuzz \
    --expdir $EXPERIMENT_ROOT/experiment \
    --master SyzAster \
    --format markdown
```
