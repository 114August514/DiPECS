# DiPECS ReleaseMemory Pressure Benefit Measurement

- Dataset: `release-memory-pressure-benefit-20260705-173505.json`
- Status: measured_no_significant_benefit
- Release target: `cache:prefetch`
- Samples per mode: 20

## Memory Pressure

| Mode | Mean pressure drop | Mean available recovered | p95 recovered | Mean PSS reduction | Mean jank delta |
| --- | ---: | ---: | ---: | ---: | ---: |
| baseline pressure | 495721.0 KB | -34148.4 KB | -27344.0 KB | -5302.8 KB | 0.0 pp |
| release memory pressure | 502319.8 KB | -37623.8 KB | -25632.0 KB | -7508.0 KB | 0.0 pp |

## Measured Inputs

- Available-memory gain over baseline: -3475.4 KB
- PSS reduction gain over baseline: -2205.2 KB
- Jank delta vs baseline: 0.0 pp
- Control-plane / dispatch cost: 96.361 ms per action

## Statistical Significance (Welch's t-test, alpha=0.05)

| Metric | t-statistic | p-value | df | Required for acceptance |
| --- | ---: | ---: | ---: | --- |
| Available memory gain | -0.446526 | 0.65937954 | 23.08 | Significant positive gain |
| PSS reduction gain | -110.347441 | 0.0 | 37.93 | Non-regression only |
| Jank delta | 0.0 | 1.0 | 19 | Non-regression only |

Available-memory gain statistically significant: **False**.
PSS non-regression: **False**.
Jank non-regression: **True**.

## Acceptance

Accepted: False.

This artifact is accepted for #99 only when n>=20 per mode, memory pressure is observed, measurements are valid, available memory improves over baseline with Welch's t-test p < 0.05, PSS reduction is not worse, and jank does not regress.
