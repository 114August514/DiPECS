# DiPECS KeepAlive Memory Pressure Measurement

- Status: pressure_valid
- Conclusion: not_significant
- Accepted: False
- Device: Pixel 6a / Android 16 (serial 2B071JEGR05551)
- Pressure hold: 1024 MB
- Pressure valid: True (oom_signal_and_memory_competition)

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
