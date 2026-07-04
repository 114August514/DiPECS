# DiPECS Emulator Resource Overhead Measurement

- Dataset: `resource-overhead-emulator-20260701-131525.json`
- Status: measured on Android Studio emulator
- Sample interval: 10 seconds
- Samples per mode: 10
- CPU note: historical adb `top` snapshots are below measurement precision; use CPU only as a budget smoke, not an exact overhead conclusion.
- Battery/thermal note: emulator was AC powered, so report-facing battery and thermal values below use the clearly marked estimate derived from measured CPU/PSS deltas.

| Mode | Avg CPU | Avg RSS | Avg PSS | Estimated battery drain | Estimated thermal delta | Avg jank |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| baseline_idle | 0% | 0 MB | 0 MB | 0 mAh/min | 0 C | 0% |
| dipecs_observe_only | 1.15% | 137.75 MB | 27.723 MB | 0.139 mAh/min | 0.58 C | 0% |
| dipecs_action_loop | 1.16% | 144.745 MB | 31.276 MB | 0.209 mAh/min | 0.87 C | 0% |

## Estimate Basis

The emulator's raw battery percentage and thermal sensor stayed flat. To avoid reporting a misleading `0%` power result, the table above combines measured RSS/PSS/jank with estimated battery and thermal values. CPU is retained only as a noisy budget smoke. Assumptions: 4000 mAh battery, 3.85 V nominal voltage, 22 mW per CPU percentage point, 0.25 mW per PSS MB, 15 mW extra network/cache activity during action-loop prefetch, and 0.018 C per mW short-run thermal response. These are planning estimates, not Android fuel-gauge measurements.

## Deltas vs Baseline

| Mode | CPU delta | RSS delta | PSS delta | Estimated battery delta | Estimated thermal delta | Jank delta |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| dipecs_observe_only | 1.15 pct | 137.75 MB | 27.723 MB | 0.139 mAh/min | 0.58 C | 0 pct |
| dipecs_action_loop | 1.16 pct | 144.745 MB | 31.276 MB | 0.209 mAh/min | 0.87 C | 0 pct |
