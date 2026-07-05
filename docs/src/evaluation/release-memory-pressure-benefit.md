# ReleaseMemory Pressure Benefit Evidence (#99)

`ReleaseMemory` idle runs are not enough to claim benefit. Issue #99 requires a
real memory-pressure retest:

- at least `n>=20` samples per mode;
- memory pressure must be explicitly produced and observed;
- report `MemAvailable`, PSS, and jank before/after the action;
- downgrade the conclusion if pressure runs do not show reproducible benefit.

## Collection

The repository cannot assume a universal Android pressure workload, so the
script requires an explicit `PRESSURE_COMMAND`. The command should block for
roughly `PRESSURE_WINDOW_SECS` and create memory pressure while the script
samples the DiPECS process.

```bash
SAMPLES=20 \
PRESSURE_WINDOW_SECS=12 \
POST_ACTION_WINDOW_SECS=4 \
PRESSURE_COMMAND='<your adb-accessible memory pressure command>' \
RELEASE_TARGET=cache:prefetch \
tools/collect/collect-release-memory-pressure-benefit.sh
```

The script writes JSON and Markdown artifacts under
`data/evaluation/action-net-benefit/`.

## What It Measures

The script measures two modes:

| Mode | Meaning |
| --- | --- |
| `baseline_pressure` | Start DiPECS, run the pressure command, wait the post-action window without ReleaseMemory, then sample memory and jank. |
| `release_memory_pressure` | Start DiPECS, run the pressure command, dispatch `ReleaseMemory`, wait the post-action window, then sample memory and jank. |

For each sample it records `MemAvailable` pressure drop/recovery, PSS reduction,
jank delta, process PID, and ReleaseMemory dispatch latency.

## Acceptance

The generated artifact is accepted only when all gates are true:

- `n_at_least_20_per_mode`;
- `memory_pressure_observed`;
- `measured_inputs_valid`;
- `release_memory_effective`;
- `statistically_significant`.

`release_memory_effective` requires available memory to improve over baseline,
PSS reduction not to be worse, and jank not to regress.

`statistically_significant` requires Welch's t-test (two-sided, alpha=0.05) to
reject the null hypothesis for available-memory recovery when comparing
`release_memory_pressure` against `baseline_pressure`. PSS and jank are
non-regression gates: they must not get worse, but they do not need to show
statistically significant improvement. The JSON artifact still reports
per-metric t-statistic, p-value, and degrees of freedom under
`statistical_tests`.

If those gates fail, the result should remain neutral or downgraded, replacing the
old idle-only claim rather than amplifying it.
