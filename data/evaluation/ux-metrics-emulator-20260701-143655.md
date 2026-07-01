# DiPECS Emulator UX Metrics Measurement

- Dataset: `ux-metrics-emulator-20260701-143655.json`
- Status: measured on Android Studio emulator
- Sample interval: 2 seconds
- Samples per mode: 3

## Startup Latency (am start -W WaitTime)

| Mode | WaitTime avg | RSS avg | PSS avg |
| --- | ---: | ---: | ---: |
| cold_startup | 579.333 ms | 136.84 MB | 32.282 MB |
| prewarm_startup | 570.667 ms | 136.939 MB | 32.354 MB |

**PreWarm effect:** 8.7 ms faster (1.5%)

## Jank / Memory (dumpsys gfxinfo + meminfo)

| Mode | Avg jank | Avg RSS | Avg PSS |
| --- | ---: | ---: | ---: |
| baseline_jank | 0.0% | 137.562 MB | 28.727 MB |
| post_release_jank | 0.0% | 137.538 MB | 28.064 MB |

**ReleaseMemory effect:** jank 0.0 pp, PSS 0.663 MB

## Conclusion

- PreWarm effective: True
- ReleaseMemory effective: True
