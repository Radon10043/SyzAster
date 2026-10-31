# Fuzzing Campaign Visualization & Statistical Analysis (`eval-fuzz`)

The statistical evaluation tool is implemented in Rust (`src/pkg/eval.rs` and `src/bin/eval-fuzz.rs`) and built into `bin/eval-fuzz`.

## Usage

```bash
../../bin/eval-fuzz \
    --expdir <EXPERIMENT_OUTPUT_DIR> \
    --master SyzAster \
    --format <text|markdown|csv|tsv|html> \
    [--strict]
```

## Metrics Computed

- **Avg. Coverage**: Mean unique kernel PCs (`__sanitizer_cov_trace_pc` blocks in `aster_core`) covered across trials.
- **CovA12**: Vargha-Delaney $\hat{A}_{12}$ non-parametric effect size comparing `master` coverage against baseline.
- **CovPV**: Two-sided Mann-Whitney U test p-value (with tie correction) for coverage.
- **Avg. Crashes**: Mean number of unique kernel crash buckets discovered across trials.
- **CraA12 / CraPV**: Vargha-Delaney $\hat{A}_{12}$ and Mann-Whitney U test p-value for unique crashes.
