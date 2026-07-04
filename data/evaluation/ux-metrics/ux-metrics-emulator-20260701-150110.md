# DiPECS Emulator UX Metrics Measurement

- Dataset: `ux-metrics-emulator-20260701-150110.json`
- Status: measured on Android Studio emulator
- Sample interval: 3 seconds
- Samples per mode: 5

## Startup Latency (am start -W WaitTime)

| Mode | TotalTime avg | RSS avg | PSS avg |
| --- | ---: | ---: | ---: |
| warm_startup | 1470.4 ms | 171.907 MB | 52.828 MB |
| prewarm_startup | 664.6 ms | 171.896 MB | 52.793 MB |

**PreWarm effect:** 805.8 ms faster (54.8%)

## Jank / Memory (dumpsys gfxinfo + meminfo)

| Mode | Avg jank | Avg RSS | Avg PSS |
| --- | ---: | ---: | ---: |
| baseline_jank | 19.05% | 164.233 MB | 43.657 MB |
| post_release_jank | 15.38% | 164.999 MB | 43.967 MB |

**ReleaseMemory effect:** jank 3.67 pp, PSS -0.31 MB

**ReleaseMemory interpretation:** weak idle-scenario evidence only; not cited as a stable positive benefit until memory-pressure retest.

## Conclusion

- PreWarm effective: True
- ReleaseMemory effective: False
- ReleaseMemory status: weak_idle_positive_jank_only
