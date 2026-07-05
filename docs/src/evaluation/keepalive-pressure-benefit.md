# KeepAlive Pressure Benefit Evidence (#98)

`KeepAlive` is not considered complete just because Android schedules a
maintenance job. Issue #98 requires pressure evidence:

- at least `n>=20` samples per mode;
- memory pressure must be explicitly produced and observed;
- report process survival, restart count, jank, PSS, and available memory;
- account for displacement cost to the rest of the device;
- compare against `StrongPredictiveActionBaseline` under the same action budget.

## Collection

The repository cannot assume every Android device has the same pressure app, so
the collector script requires an explicit `PRESSURE_COMMAND`. The command should
block for roughly `PRESSURE_WINDOW_SECS` and create memory pressure while the
script samples the DiPECS process.

```bash
SAMPLES=20 \
PRESSURE_WINDOW_SECS=12 \
PRESSURE_COMMAND='<your adb-accessible memory pressure command>' \
EXAMPLES=<test-window-count> \
DIPECS_HIT_RATE_PCT=<dipecs-hit-rate> \
STRONG_HIT_RATE_PCT=<strong-baseline-hit-rate> \
tools/collect/collect-keepalive-pressure-benefit.sh
```

The script writes JSON and Markdown artifacts under
`data/evaluation/action-net-benefit/`.

## What It Measures

The script measures two modes:

| Mode | Meaning |
| --- | --- |
| `baseline_pressure` | Start DiPECS, run the pressure command, then check whether the same process survived. |
| `keepalive_pressure` | Start DiPECS, dispatch `KeepAlive(work:collector_heartbeat)`, run the pressure command, then check survival. |

For each sample it records PID before/after, survival, restart count,
`MemAvailable`, PSS delta, jank, and KeepAlive dispatch latency.

## Acceptance

The generated artifact is accepted only when all gates are true:

- `n_at_least_20_per_mode`;
- `memory_pressure_observed`;
- `measured_inputs_valid`;
- `same_budget_baseline_inputs_present`;
- `net_benefit_positive`;
- `dipecs_beats_strong_predictive`.

If the pressure command or same-budget hit-rate inputs are omitted, the run must
remain `measurement_pending_baseline_gate` and must not be cited as closing #98.
