# DiPECS Emulator UX Metrics Measurement

- Dataset: `ux-metrics-emulator-20260701-145001.json`
- Status: measured on Android Studio emulator
- Sample interval: 2 seconds
- Samples per mode: 3

## Startup Latency (am start -W WaitTime)

| Mode | TotalTime avg | RSS avg | PSS avg |
| --- | ---: | ---: | ---: |
| warm_startup | 667.667 ms | 170.984 MB | 51.843 MB |
| prewarm_startup | 672.0 ms | 173.358 MB | 54.263 MB |

**PreWarm effect:** -4.3 ms faster (-0.6%)

## Jank / Memory (dumpsys gfxinfo + meminfo)

| Mode | Avg jank | Avg RSS | Avg PSS |
| --- | ---: | ---: | ---: |
| baseline_jank | 9.52% | 163.981 MB | 43.439 MB |
| post_release_jank | 7.69% | 164.798 MB | 43.869 MB |

**ReleaseMemory effect:** jank 1.83 pp, PSS -0.43 MB

## Conclusion

- PreWarm effective: False
- ReleaseMemory effective: True
