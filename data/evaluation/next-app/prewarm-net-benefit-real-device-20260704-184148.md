# DiPECS PreWarm Net-Benefit Measurement

- Dataset: `prewarm-net-benefit-real-device-20260704-184148.json`
- Status: measured on Pixel 6a real device
- Samples per mode: 20
- Prediction report: `data/evaluation/next-app/lsapp-standard.report.json`

## Startup Measurements

| Mode | Mean TotalTime | p95 TotalTime |
| --- | ---: | ---: |
| collector cold | 710.75 ms | 733.0 ms |
| collector after PreWarm hit | 201.55 ms | 213.0 ms |
| Settings cold | 163.65 ms | 177.0 ms |
| Settings after wrong PreWarm | 164.15 ms | 171.0 ms |

## Measured Inputs

- Hit saved latency: 509.2 ms
- Miss startup delta: 0.5 ms
- Mean PreWarm dispatch latency: 8.394 ms
- Miss action cost: 0.5 ms
- Control-plane / dispatch cost: 8.394 ms per action

## Net Benefit

| Ranker | hit@1 | gross saved | gross wasted | control cost | net benefit |
| --- | ---: | ---: | ---: | ---: | ---: |
| DiPECS ensemble | 56.509% | 78415660.263 ms | 59260.619 ms | 2287524.486 ms | 76068875.158 ms |
| StrongPredictiveActionBaseline | 53.784% | 74634268.374 ms | 62973.691 ms | 2287524.486 ms | 72283770.198 ms |

DiPECS minus strong baseline: 3785104.96 ms.

## Scope

This artifact closes the #90 standard-split gate for Android-safe `PreWarmProcess own:*` evidence: LSApp standard hit@1 is reused from the committed prediction report, while hit/miss startup deltas and dispatch cost are measured on Pixel 6a with n=20 per mode. It does not claim silent third-party app prewarm on normal Android installs.
