# DiPECS KeepAlive Memory Pressure Measurement

- Status: emulator_dry_run
- Conclusion: not_significant
- Accepted: False
- Device: sdk_gphone64_x86_64 / Android 15 (serial emulator-5554)
- Pressure hold: 16 MB
- Pressure valid: False (emulator_dry_run)

## Summary

| Metric | Value |
| --- | ---: |
| formal n>=20 per mode | False |
| native baseline hit | False |
| survival delta | 0.000 pp |
| restart delta | 0.000 pp |
| return p95 delta | 0.000 ms |
| jank delta | 0.000 pp |
| available-after delta | 0.000 MB |
| net benefit score | 0.000 |

## Strong Baseline

| Model | hit@1 | action budget | net benefit score |
| --- | ---: | ---: | ---: |
| DiPECS ensemble | 56.509% | 272519 | 0.0 |
| StrongPredictiveActionBaseline | 53.784% | 272519 | 0.0 |

## Interpretation

KeepAlive did not pass measured pressure benefit gates
