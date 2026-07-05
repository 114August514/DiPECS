# DiPECS ReleaseMemory Pressure Benefit Measurement

- Dataset: `release-memory-pressure-benefit-20260705-185226.json`
- Status: measured_android_device
- Release target: `cache:volatile`
- Samples per mode: 20

## Memory Pressure

| Mode | Mean pressure drop | Mean available recovered | p95 recovered | Mean PSS reduction | Mean jank delta |
| --- | ---: | ---: | ---: | ---: | ---: |
| baseline pressure | 555226.8 KB | 50430.4 KB | 163812.0 KB | -3868.25 KB | 0.0 pp |
| release memory pressure | 550052.2 KB | 105589.0 KB | 147004.0 KB | 60753.05 KB | 0.0 pp |

## Measured Inputs

- Available-memory gain over baseline: 55158.6 KB
- PSS reduction gain over baseline: 64621.3 KB
- Jank delta vs baseline: 0.0 pp
- Control-plane / dispatch cost: 129.376 ms per action

## Statistical Significance (Welch's t-test, alpha=0.05)

| Metric | t-statistic | p-value | df | Required for acceptance |
| --- | ---: | ---: | ---: | --- |
| Available memory gain | 4.211443 | 0.00026891 | 26.0 | Significant positive gain |
| PSS reduction gain | 1157.502208 | 0.0 | 21.24 | Non-regression only |
| Jank delta | 0.0 | 1.0 | 19 | Non-regression only |

Available-memory gain statistically significant: **True**.
PSS non-regression: **True**.
Jank non-regression: **True**.

## Acceptance

Accepted: True.

This artifact is accepted for #99 only when n>=20 per mode, memory pressure is observed, measurements are valid, available memory improves over baseline with Welch's t-test p < 0.05, PSS reduction is not worse, and jank does not regress.
