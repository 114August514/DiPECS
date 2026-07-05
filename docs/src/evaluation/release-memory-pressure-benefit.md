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

For the upgraded app-owned volatile memory semantics, seed a bounded in-process
cache before each sample and release that target:

```bash
SAMPLES=20 \
PRESSURE_WINDOW_SECS=8 \
POST_ACTION_WINDOW_SECS=4 \
SEED_VOLATILE_CACHE_MB=64 \
RELEASE_TARGET=cache:volatile \
PRESSURE_COMMAND='adb shell /data/local/tmp/dipecs-mem-pressure-hold 512 20 >/dev/null 2>&1' \
tools/collect/collect-release-memory-pressure-benefit.sh
```

The script writes JSON and Markdown artifacts under
`data/evaluation/action-net-benefit/`.

## What It Measures

The script measures two modes:

| Mode | Meaning |
| --- | --- |
| `baseline_pressure` | Start DiPECS, optionally seed volatile cache, keep the pressure command alive through the measurement window, wait without ReleaseMemory, then sample memory and jank. |
| `release_memory_pressure` | Start DiPECS, optionally seed volatile cache, keep the pressure command alive, dispatch `ReleaseMemory`, wait the post-action window, then sample memory and jank. |

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

## Pixel 6a Results (2026-07-05)

### Old `cache:prefetch` Semantics

Measured artifact:
`data/evaluation/action-net-benefit/release-memory-pressure-benefit-20260705-173505.json`.

Environment:

- device: Pixel 6a (`2B071JEGR05551`);
- package: `com.dipecs.collector`;
- release target: `cache:prefetch`;
- pressure source: non-root `/data/local/tmp/dipecs-mem-pressure-hold 512 20`
  process that allocates and touches 512 MB anonymous memory as Android `shell`;
- samples: n=20 per mode.

The pressure gate passed: baseline mean `MemAvailable` pressure drop was
495721.0 KB and release-arm pressure drop was 502319.8 KB. The benefit gate did
not pass:

| Metric | Value |
| --- | ---: |
| Available-memory gain vs baseline | -3475.4 KB |
| PSS reduction gain vs baseline | -2205.2 KB |
| Jank delta vs baseline | 0.0 pp |
| Available-memory Welch p-value | 0.65937954 |
| Control-plane / dispatch cost | 96.361 ms/action |

Conclusion: `accepted=false`,
`memory_pressure_observed=true`, `release_memory_effective=false`,
`statistically_significant=false`. This result remains important: deleting
prefetch cache files is not a real PSS/available-memory release mechanism under
anonymous memory pressure, so it should not enter the positive results list.

### Upgraded `cache:volatile` Semantics

Latest measured artifact:
`data/evaluation/action-net-benefit/release-memory-pressure-benefit-20260705-185226.json`.

Environment:

- device: Pixel 6a (`2B071JEGR05551`);
- package: `com.dipecs.collector`;
- seed target: `PreWarmProcess own:volatile-cache:64`;
- release target: `cache:volatile`;
- pressure source: non-root `/data/local/tmp/dipecs-mem-pressure-hold 512 20`
  process that allocates and touches 512 MB anonymous memory as Android `shell`;
- samples: n=20 per mode.

The pressure gate passed in both arms. Baseline mean `MemAvailable` pressure
drop was 555226.8 KB; release-arm pressure drop was 550052.2 KB. The upgraded
benefit gate passed:

| Metric | Value |
| --- | ---: |
| Available-memory gain vs baseline | 55158.6 KB |
| PSS reduction gain vs baseline | 64621.3 KB |
| Jank delta vs baseline | 0.0 pp |
| Available-memory Welch p-value | 0.00026891 |
| Control-plane / dispatch cost | 129.376 ms/action |

Conclusion: `accepted=true`, `memory_pressure_observed=true`,
`release_memory_effective=true`, `statistically_significant=true`. This is a
positive #99 result for the upgraded app-owned volatile memory release semantics.
It does not retroactively make `cache:prefetch` a memory-pressure benefit; it
replaces the weak disk-cache deletion semantics with a real app-owned memory
cache that can be safely released.
