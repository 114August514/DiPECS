# DiPECS Emulator UX Metrics Measurement

- Dataset: `ux-metrics-emulator-20260701-151856.json`
- Status: measured on Android Studio emulator
- Sample interval: 3 seconds
- Samples per mode: 5

## Startup Latency (am start -W WaitTime)

| Mode | TotalTime avg | RSS avg | PSS avg |
| --- | ---: | ---: | ---: |
| warm_startup | 1551.6 ms | 172.319 MB | 53.175 MB |
| prewarm_startup | 872.6 ms | 175.762 MB | 56.666 MB |

**PreWarm effect:** 679.0 ms faster (43.8%)

## Jank / Memory (dumpsys gfxinfo + meminfo)

| Mode | Avg jank | Avg RSS | Avg PSS |
| --- | ---: | ---: | ---: |
| baseline_jank | 30.0% | 164.374 MB | 43.804 MB |
| post_release_jank | 30.0% | 165.113 MB | 44.135 MB |

**ReleaseMemory effect:** jank 0.0 pp, PSS -0.331 MB

## Conclusion

- PreWarm effective: True
- ReleaseMemory effective: True
