# Experiment Environment Setup

Please replace the following variables according to your actual environment:
- `$EXPERIMENT_ROOT`: directory for saving experiment artifacts (kernels, images, fuzzers, and fuzzing outputs).
- `$JOBS`: number of parallel build jobs (e.g., `16`).
- `$CPUS`: number of CPUs assigned to the build/fuzzing container.

The prefix of each instruction indicates the execution context:
- `(host)`: the host machine.
- `(container.syzaster)`: a container derived from `SyzAster/docker/Dockerfile`.
- `(vm)`: the Asterinas QEMU virtual machine.

## Preparation

(host) Build the self-contained SyzAster Docker image:
```bash
git clone --recurse-submodules https://github.com/Radon10043/SyzAster /tmp/SyzAster
cd /tmp/SyzAster
docker build -t syzaster:latest --network host -f ./docker/Dockerfile .
```

(host) Start the build container:
```bash
docker run \
    -v $EXPERIMENT_ROOT:$EXPERIMENT_ROOT \
    --cpus $CPUS --network host \
    --privileged -d --rm \
    --name syzaster-build \
    syzaster:latest tail -f /dev/null
```

(host) Enter the container:
```bash
docker exec -it syzaster-build bash
```

(container.syzaster) Initialize the experiment workspace structure:
```bash
cd $EXPERIMENT_ROOT
mkdir -p kernel fuzzer image experiment
```

(container.syzaster) Clone SyzAster into `$EXPERIMENT_ROOT/fuzzer/SyzAster`:
```bash
cd $EXPERIMENT_ROOT/fuzzer
git clone --recurse-submodules https://github.com/Radon10043/SyzAster
cd SyzAster
git submodule update --init --recursive
```

## Table of Contents

- [Setup `fuzzer/SyzAster`](setup-fuzzer_SyzAster.md)
- [Setup `kernel/asterinas`](setup-kernel_asterinas.md)
- [Setup `image/asterinas`](setup-image_asterinas.md)
- [Setup `compose.env`](setup-env_file.md)
- [Run Evaluation with Docker Compose](docker-compose.md)

## Run Evaluation & Statistical Analysis

(host) Launch a 24-hour fuzzing campaign (e.g., 5 repeated trials on Asterinas `v0.18.1`):
```bash
docker compose --env-file ./compose.env \
    -f $EXPERIMENT_ROOT/fuzzer/SyzAster/experiment/docker-compose/compose.SyzAster.yaml \
    up asterinas-v0.18.1-kernel --scale asterinas-v0.18.1-kernel=5 -d
```

(container.syzaster) Summarize coverage, unique crashes, Vargha-Delaney $\hat{A}_{12}$, and Mann-Whitney U test p-values using the Rust evaluation tool `bin/eval-fuzz`:
```bash
$EXPERIMENT_ROOT/fuzzer/SyzAster/bin/eval-fuzz \
    --expdir $EXPERIMENT_ROOT/experiment \
    --master SyzAster \
    --format markdown
```
