# DiPECS PrefetchFile Benefit Measurement

- Dataset: `prefetch-file-benefit-20260705-201053.json`
- Status: measured_android_device
- Target: `url:https://raw.githubusercontent.com/114August514/DiPECS/main/docs/src/slides/final/package-lock.json`
- Samples per mode: 20

## Latency

| Mode | Mean total | p95 total | Mean read | p95 read | Mean prefetch wait |
| --- | ---: | ---: | ---: | ---: | ---: |
| prefetched read | 79.993 ms | 101.332 ms | 79.993 ms | 101.332 ms | 1824.15 ms |
| miss fetch then read | 1860.332 ms | 2276.297 ms | 84.532 ms | 102.156 ms | 1775.8 ms |

## Measured Inputs

- Hit saved latency: 1780.339 ms
- Miss action cost: 1775.8 ms
- Control-plane / dispatch cost: 8.916 ms per action

## Same-Budget Baseline

Same-budget comparison inputs present: True.
Accepted: True.

This artifact is accepted for #97 only when n>=20 per mode, measured inputs are positive, same-budget hit-rate inputs are present for DiPECS and StrongPredictiveActionBaseline, DiPECS net benefit is positive, and DiPECS beats the strong baseline.
