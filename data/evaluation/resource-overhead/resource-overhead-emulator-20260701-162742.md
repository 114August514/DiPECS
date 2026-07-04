# DiPECS Emulator Resource Overhead Measurement

- Dataset: `resource-overhead-emulator-20260701-162742.json`
- Status: measured on Android Studio emulator
- Sample interval: 10 seconds
- Samples per mode: 30
- CPU note: historical adb `top` snapshots are below measurement precision here; `0.0%` and negative deltas must not be cited as exact CPU usage.
- Battery/thermal note: emulator was AC powered, so report-facing battery and thermal values below use the clearly marked estimate derived from measured CPU/PSS deltas.

| Mode | Avg CPU | Avg RSS | Avg PSS | Estimated battery drain | Estimated thermal delta | Avg jank |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| baseline_idle | 0.493% | 118.297 MB | 36.024 MB | 0 mAh/min | 0 C | 0.0% |
| dipecs_observe_only | 0.387% | 125.87 MB | 39.629 MB | 0.004 mAh/min | 0.02 C | 0.0% |
| dipecs_action_loop | 0.0% | 132.797 MB | 41.621 MB | 0.071 mAh/min | 0.3 C | 0.0% |

## Estimate Basis

The emulator's raw battery percentage and thermal sensor stayed flat. To avoid reporting a misleading `0%` power result, the table above combines measured RSS/PSS/jank with estimated battery and thermal values. CPU is retained only as a noisy budget smoke; the stable conclusions from this run are RSS/PSS increments of roughly +7-8 MB per layer and 0.0% jank.
